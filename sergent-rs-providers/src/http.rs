//! Private typed HTTP POST crossing for provider transport. Production crosses
//! external systems exactly once through the fixed reqwest implementation.
//! Framework tests use the same interface through the in-memory
//! `ScriptedHttpClient`.

use std::future::Future;
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::HeaderMap;

/// One complete JSON POST request whose immutable body bytes are shared by
/// every retry attempt.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HttpPostRequest {
    pub(crate) url: reqwest::Url,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
    pub(crate) timeout: Duration,
}

/// One HTTP response whose metadata is available before its body is consumed.
pub(crate) struct HttpResponse {
    pub(crate) status: u16,
    pub(crate) headers: HeaderMap,
    body_length: Option<u64>,
    body: ResponseBody,
}

impl HttpResponse {
    /// Collects body chunks under the caller's exact byte ceiling. An
    /// advertised overflow fails before the collection buffer is allocated.
    pub(crate) async fn collect_body(
        self,
        byte_limit: usize,
    ) -> Result<Vec<u8>, BodyCollectionFailure> {
        if self
            .body_length
            .is_some_and(|length| length > byte_limit as u64)
        {
            return Err(BodyCollectionFailure::without_partial(
                HttpFailure::BodyTooLarge,
            ));
        }
        let mut collected = Vec::with_capacity(byte_limit);
        match self.body {
            ResponseBody::Reqwest(mut response) => loop {
                let chunk = match response.chunk().await {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        return Err(BodyCollectionFailure::with_partial(
                            classify_reqwest_failure(&error, RequestIoPhase::ReadBody),
                            collected,
                        ));
                    }
                };
                let Some(chunk) = chunk else {
                    break;
                };
                append_bounded(&mut collected, &chunk, byte_limit)
                    .map_err(BodyCollectionFailure::without_partial)?;
            },
            #[cfg(test)]
            ResponseBody::Scripted(ScriptedBody::Chunks(chunks)) => {
                for chunk in chunks {
                    let chunk = match chunk {
                        Ok(chunk) => chunk,
                        Err(failure) => {
                            return Err(BodyCollectionFailure::with_partial(failure, collected));
                        }
                    };
                    append_bounded(&mut collected, &chunk, byte_limit)
                        .map_err(BodyCollectionFailure::without_partial)?;
                }
            }
            #[cfg(test)]
            ResponseBody::Scripted(ScriptedBody::MustNotRead) => {
                panic!("scripted response body must not be consumed")
            }
        }
        Ok(collected)
    }

    /// Builds one scripted response from exact body chunks.
    #[cfg(test)]
    pub(crate) fn scripted(
        status: u16,
        headers: HeaderMap,
        chunks: impl IntoIterator<Item = Result<Bytes, HttpFailure>>,
    ) -> Self {
        Self {
            status,
            headers,
            body_length: None,
            body: ResponseBody::Scripted(ScriptedBody::Chunks(chunks.into_iter().collect())),
        }
    }

    /// Builds a status-only scripted response that fails loudly if its body is
    /// consumed, representing an irrelevant body that cannot complete.
    #[cfg(test)]
    pub(crate) fn status_only(status: u16, headers: HeaderMap) -> Self {
        Self {
            status,
            headers,
            body_length: None,
            body: ResponseBody::Scripted(ScriptedBody::MustNotRead),
        }
    }
}

#[cfg(test)]
impl HttpPostRequest {
    /// Decodes native request bytes only for provider-shape assertions.
    pub(crate) fn json_body(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("recorded native request is JSON")
    }
}

/// The closed production or test body source behind response metadata.
enum ResponseBody {
    Reqwest(reqwest::Response),
    #[cfg(test)]
    Scripted(ScriptedBody),
}

/// Exact byte chunks or a deterministic proof that a status-only path never
/// asks for the irrelevant body.
#[cfg(test)]
enum ScriptedBody {
    Chunks(Vec<Result<Bytes, HttpFailure>>),
    MustNotRead,
}

/// The closed failures the reqwest crossing can produce for one POST.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HttpFailure {
    Timeout,
    Connection,
    Send,
    BodyRead,
    BodyTooLarge,
}

/// A response-body failure paired with only the bounded bytes observed before
/// that read failed. Byte-limit failures expose no partial body because the
/// complete admitted body contract was exceeded.
pub(crate) struct BodyCollectionFailure {
    pub(crate) failure: HttpFailure,
    pub(crate) partial: Vec<u8>,
}

impl BodyCollectionFailure {
    /// Retain bytes observed before a body read failed.
    fn with_partial(failure: HttpFailure, partial: Vec<u8>) -> Self {
        Self { failure, partial }
    }

    /// Close a body failure for which no partial text is admissible.
    fn without_partial(failure: HttpFailure) -> Self {
        Self {
            failure,
            partial: Vec::new(),
        }
    }
}

/// The private, statically dispatched HTTP interface used by provider code.
pub(crate) trait HttpClient: Send + Sync {
    /// Sends the complete typed JSON request and returns response metadata
    /// without consuming its body.
    fn post(
        &self,
        request: &HttpPostRequest,
    ) -> impl Future<Output = Result<HttpResponse, HttpFailure>> + Send;
}

/// Production HTTP transport with the complete fixed reqwest policy.
pub(crate) struct ReqwestHttpClient {
    client: reqwest::Client,
}

impl ReqwestHttpClient {
    /// Constructs production HTTP using the complete fixed reqwest policy.
    pub(crate) fn production() -> Self {
        Self::from_builder(reqwest::Client::builder())
    }

    /// Applies no redirects, protocol retries, or system proxies before
    /// building the client; invalid fixed policy is a process failure.
    fn from_builder(builder: reqwest::ClientBuilder) -> Self {
        let client = builder
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .build()
            .expect("the fixed reqwest client policy is valid");
        Self { client }
    }
}

impl HttpClient for ReqwestHttpClient {
    /// Performs one POST and returns status, headers, and the unread body.
    async fn post(&self, request: &HttpPostRequest) -> Result<HttpResponse, HttpFailure> {
        let response = self
            .client
            .post(request.url.clone())
            .headers(request.headers.clone())
            .body(request.body.clone())
            .timeout(request.timeout)
            .send()
            .await
            .map_err(|error| classify_reqwest_failure(&error, RequestIoPhase::Send))?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body_length = response.content_length();
        Ok(HttpResponse {
            status,
            headers,
            body_length,
            body: ResponseBody::Reqwest(response),
        })
    }
}

/// Test-only HTTP transport whose typed outcomes and request record live
/// entirely in memory.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct ScriptedHttpClient {
    state: std::sync::Arc<std::sync::Mutex<ScriptedState>>,
}

#[cfg(test)]
/// Mutable in-memory state shared by scripted client clones.
struct ScriptedState {
    outcomes: std::collections::VecDeque<Result<HttpResponse, HttpFailure>>,
    requests: Vec<HttpPostRequest>,
    timing: Option<ScriptedTiming>,
}

#[cfg(test)]
/// Binds a manual evidence clock and one deterministic timing effect to each
/// queued outcome.
struct ScriptedTiming {
    clock: crate::evidence::ManualClock,
    steps: std::collections::VecDeque<ScriptedTimingStep>,
}

/// One closed, deterministic elapsed-time effect attached to a scripted HTTP
/// completion. It can only advance or set the existing manual evidence clock;
/// the fake never reads time and the HTTP outcome remains typed.
#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct ScriptedTimingStep {
    elapsed_ms: u64,
    finished_wall_ms: Option<u64>,
}

#[cfg(test)]
impl ScriptedTimingStep {
    /// Creates a step that advances elapsed and wall readings together.
    pub(crate) fn elapsed(elapsed_ms: u64) -> Self {
        Self {
            elapsed_ms,
            finished_wall_ms: None,
        }
    }

    /// Overrides the finishing wall reading after elapsed time advances,
    /// leaving monotonic duration unchanged.
    pub(crate) fn finishing_wall_at(mut self, wall_ms: u64) -> Self {
        self.finished_wall_ms = Some(wall_ms);
        self
    }
}

#[cfg(test)]
impl ScriptedHttpClient {
    /// Queues typed outcomes, starts an empty request record, and applies no
    /// timing effects.
    pub(crate) fn new(
        outcomes: impl IntoIterator<Item = Result<HttpResponse, HttpFailure>>,
    ) -> Self {
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(ScriptedState {
                outcomes: outcomes.into_iter().collect(),
                requests: Vec::new(),
                timing: None,
            })),
        }
    }

    /// Pairs every queued outcome with exactly one deterministic timing effect.
    pub(crate) fn new_timed(
        outcomes: impl IntoIterator<Item = Result<HttpResponse, HttpFailure>>,
        clock: crate::evidence::ManualClock,
        timing_steps: impl IntoIterator<Item = ScriptedTimingStep>,
    ) -> Self {
        let outcomes = outcomes
            .into_iter()
            .collect::<std::collections::VecDeque<_>>();
        let steps = timing_steps
            .into_iter()
            .collect::<std::collections::VecDeque<_>>();
        assert_eq!(
            outcomes.len(),
            steps.len(),
            "every scripted HTTP outcome must have exactly one timing step"
        );
        Self {
            state: std::sync::Arc::new(std::sync::Mutex::new(ScriptedState {
                outcomes,
                requests: Vec::new(),
                timing: Some(ScriptedTiming { clock, steps }),
            })),
        }
    }

    /// Returns an isolated snapshot of all requests in call order.
    pub(crate) fn recorded_requests(&self) -> Vec<HttpPostRequest> {
        self.state
            .lock()
            .expect("scripted HTTP client mutex")
            .requests
            .clone()
    }
}

#[cfg(test)]
impl HttpClient for ScriptedHttpClient {
    /// Records the request before consuming its outcome, applies any paired
    /// timing effect, and returns an immediately ready future.
    fn post(
        &self,
        request: &HttpPostRequest,
    ) -> impl Future<Output = Result<HttpResponse, HttpFailure>> + Send {
        let (outcome, timing) = {
            let mut state = self.state.lock().expect("scripted HTTP client mutex");
            state.requests.push(request.clone());
            let request_count = state.requests.len();
            let outcome = state.outcomes.pop_front().unwrap_or_else(|| {
                panic!(
                    "scripted HTTP outcomes exhausted after request {}",
                    request_count
                )
            });
            let timing = state.timing.as_mut().map(|timing| {
                let step = timing.steps.pop_front().unwrap_or_else(|| {
                    panic!(
                        "scripted HTTP timing exhausted after request {}",
                        request_count
                    )
                });
                (timing.clock.clone(), step)
            });
            (outcome, timing)
        };
        if let Some((clock, timing)) = timing {
            clock.advance(timing.elapsed_ms);
            if let Some(wall_ms) = timing.finished_wall_ms {
                clock.set_wall(wall_ms);
            }
        }
        std::future::ready(outcome)
    }
}

#[derive(Clone, Copy)]
enum RequestIoPhase {
    Send,
    ReadBody,
}

/// Appends one exact chunk only when it fits within the fixed collection
/// allocation and byte ceiling.
fn append_bounded(
    collected: &mut Vec<u8>,
    chunk: &[u8],
    byte_limit: usize,
) -> Result<(), HttpFailure> {
    if chunk.len() > byte_limit.saturating_sub(collected.len()) {
        return Err(HttpFailure::BodyTooLarge);
    }
    collected.extend_from_slice(chunk);
    Ok(())
}

/// Preserves timeout and connection classifications across phases, then maps
/// other failures according to whether sending or body reading failed.
fn classify_reqwest_failure(error: &reqwest::Error, phase: RequestIoPhase) -> HttpFailure {
    if error.is_timeout() {
        HttpFailure::Timeout
    } else if error.is_connect() {
        HttpFailure::Connection
    } else {
        match phase {
            RequestIoPhase::Send => HttpFailure::Send,
            RequestIoPhase::ReadBody => HttpFailure::BodyRead,
        }
    }
}
