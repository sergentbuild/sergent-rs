//! Whole provider invocation lifecycle: selection, discovery, Ollama
//! preflight, one native request build and serialization, bounded retry,
//! strict extraction, and terminal call evidence.

use std::time::Duration;

use bytes::Bytes;
use reqwest::header::{CONTENT_TYPE, HeaderValue};
use serde_json::json;

use sergent_rs_core::error::{ErrorKind, RunError, sanitize_model_output_diagnostic};
use sergent_rs_core::model::{
    Attempt, CallUsage, ModelError, ModelIdentity, ModelRequest, ModelResponse, ParsedJsonObject,
};

use crate::adapter::{BuildInput, credential_headers};
use crate::credentials::{Discovered, Endpoints, Environment, PreAttemptFailure, discover};
use crate::evidence::{Clock, ClockMark, failure_attempt, success_attempt};
use crate::http::{HttpClient, HttpPostRequest};
use crate::selection::{Provider, Selection, resolve};

use super::wire_request::{
    CompletedWireRequest, FailedWireRequest, NonNaturalWireRequest, ResponseObservation,
    WireOutcome, send_and_classify,
};

// The retry budget is fixed at two outer attempts and never
// caller-configurable.
pub(crate) const MAX_ATTEMPTS: usize = 2;

/// The private provider invocation engine, statically dispatched over its HTTP
/// crossing.
pub(super) struct ProviderClient<H> {
    http: H,
    env: Environment,
    endpoints: Endpoints,
    clock: Clock,
}

/// One fully prepared generation call. The resolved selection remains intact
/// beside the single request borrowed by every generation attempt.
struct PreparedInvocation {
    selection: Selection,
    generation_request: HttpPostRequest,
}

/// The two truthful Ollama preflight failure states: either response metadata
/// completed or the transport returned without one.
enum PreflightFailure {
    Response(PreAttemptFailure),
    ResponseLess(PreAttemptFailure),
}

/// The closed result after a natural provider envelope has already produced
/// one successful attempt.
enum NaturalEnvelopeClosure {
    /// Strict extraction produced the one object returned beside call evidence.
    Parsed {
        response: ModelResponse,
        object: ParsedJsonObject,
    },
    /// Strict extraction rejected the semantic text while retaining its exact
    /// raw output, completed usage, identity, and successful attempt.
    ExtractionFailed(ModelError),
}

impl<H> ProviderClient<H> {
    /// Bind one HTTP crossing, environment source, endpoint set, and clock to
    /// the private invocation engine.
    pub(super) fn new(http: H, env: Environment, endpoints: Endpoints, clock: Clock) -> Self {
        Self {
            http,
            env,
            endpoints,
            clock,
        }
    }
}

impl<H: HttpClient> ProviderClient<H> {
    /// Own one complete provider call from selection through strict extraction,
    /// preserving bounded retry and all reached evidence.
    pub(super) async fn invoke(
        &self,
        request: &ModelRequest,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        // Whole-call timing opens before selection and closes only after every
        // reached provider-boundary step.
        let invoke_started = self.clock.start();
        let selection = resolve(request.model_name())?;
        let prepared = self.prepare(request, selection, &invoke_started).await?;
        self.run_generation_attempts(&invoke_started, &prepared)
            .await
    }

    /// Discover external configuration, run any required preflight, and build
    /// the one native generation request used by the entire attempt loop.
    async fn prepare(
        &self,
        request: &ModelRequest,
        selection: Selection,
        invoke_started: &ClockMark,
    ) -> Result<PreparedInvocation, ModelError> {
        let discovered = discover(selection.provider, &self.env, &self.endpoints)
            .map_err(|failure| pre_attempt_error(&selection.identity, failure, None))?;
        let timeout = Duration::from_secs(u64::from(request.model_settings().timeout_secs.get()));
        self.preflight(&selection, &discovered, timeout, invoke_started)
            .await?;
        let generation_request =
            build_generation_request(request, &selection, &discovered, timeout);
        Ok(PreparedInvocation {
            selection,
            generation_request,
        })
    }

    /// Run the fixed attempt budget, record each completed wire classification
    /// in order, and close on the first terminal outcome.
    async fn run_generation_attempts(
        &self,
        invoke_started: &ClockMark,
        prepared: &PreparedInvocation,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        let mut attempts = Vec::new();
        for index in 0..MAX_ATTEMPTS {
            let attempt_started = self.clock.start();
            let outcome = send_and_classify(
                &self.http,
                prepared.selection.provider,
                &prepared.generation_request,
            )
            .await;
            let span = self.clock.close(&attempt_started);
            match outcome {
                WireOutcome::Completed(completed) => {
                    attempts.push(success_attempt(span));
                    return match self.close_natural_envelope(
                        invoke_started,
                        &prepared.selection.identity,
                        attempts,
                        completed,
                    ) {
                        NaturalEnvelopeClosure::Parsed { response, object } => {
                            Ok((response, object))
                        }
                        NaturalEnvelopeClosure::ExtractionFailed(error) => Err(error),
                    };
                }
                WireOutcome::NonNatural(non_natural) => {
                    let description = non_natural_message(&non_natural.reason);
                    attempts.push(failure_attempt(
                        span,
                        RunError::of(ErrorKind::InvalidResponse, description.clone()),
                        false,
                    ));
                    return Err(self.close_non_natural(
                        invoke_started,
                        &prepared.selection.identity,
                        attempts,
                        non_natural,
                        description,
                    ));
                }
                WireOutcome::Failure(failure) => {
                    attempts.push(failure_attempt(
                        span,
                        RunError::of(failure.kind, failure.message.clone()),
                        failure.retryable,
                    ));
                    if failure.retryable && index + 1 < MAX_ATTEMPTS {
                        continue;
                    }
                    return Err(close_generation_failure(
                        &prepared.selection.identity,
                        attempts,
                        failure,
                        self.clock.elapsed_ms(invoke_started),
                    ));
                }
            }
        }
        unreachable!("the retry loop returns on every completed or exhausted attempt")
    }

    /// Run the Ollama existence probe exactly once when selected, before any
    /// generation request is built or attempted.
    async fn preflight(
        &self,
        selection: &Selection,
        discovered: &Discovered,
        timeout: Duration,
        invoke_started: &ClockMark,
    ) -> Result<(), ModelError> {
        if selection.provider != Provider::Ollama {
            return Ok(());
        }
        self.ollama_preflight(discovered, &selection.model, timeout)
            .await
            .map_err(|failure| {
                self.close_preflight_failure(invoke_started, &selection.identity, failure)
            })
    }

    /// Probe the selected Ollama daemon for the exact model before generation.
    async fn ollama_preflight(
        &self,
        discovered: &Discovered,
        model: &str,
        timeout: Duration,
    ) -> Result<(), PreflightFailure> {
        let url = discovered.endpoint.with_segments(["api", "show"]);
        let body = Bytes::from(
            serde_json::to_vec(&json!({ "model": model }))
                .expect("a native preflight JSON value is serializable"),
        );
        let mut headers = credential_headers(discovered.auth.as_ref());
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let request = HttpPostRequest {
            url,
            headers,
            body,
            timeout,
        };
        match self.http.post(&request).await {
            Ok(response) => classify_preflight_status(response.status, model)
                .map_err(PreflightFailure::Response),
            Err(_) => Err(PreflightFailure::ResponseLess(PreAttemptFailure::new(
                ErrorKind::ProviderUnavailable,
                "the ollama daemon is unreachable",
            ))),
        }
    }

    /// Close one failed Ollama preflight according to whether response
    /// metadata completed, without inventing a generation attempt.
    fn close_preflight_failure(
        &self,
        invoke_started: &ClockMark,
        identity: &ModelIdentity,
        failure: PreflightFailure,
    ) -> ModelError {
        match failure {
            PreflightFailure::Response(failure) => pre_attempt_error(
                identity,
                failure,
                Some(CallUsage {
                    latency_ms: self.clock.elapsed_ms(invoke_started),
                    tokens: None,
                    request_id: None,
                }),
            ),
            PreflightFailure::ResponseLess(failure) => pre_attempt_error(identity, failure, None),
        }
    }

    /// Close a natural provider envelope after one strict semantic-object
    /// parse, preserving the successful attempt on extraction failure.
    fn close_natural_envelope(
        &self,
        invoke_started: &ClockMark,
        identity: &ModelIdentity,
        attempts: Vec<Attempt>,
        completed: CompletedWireRequest,
    ) -> NaturalEnvelopeClosure {
        let parsed = extract_json_object(&completed.text);
        let usage = CallUsage {
            latency_ms: self.clock.elapsed_ms(invoke_started),
            tokens: completed.tokens,
            request_id: completed.request_id,
        };
        match parsed {
            Some(object) => NaturalEnvelopeClosure::Parsed {
                response: ModelResponse {
                    identity: identity.clone(),
                    raw_output: completed.text,
                    attempts,
                    usage,
                },
                object,
            },
            None => NaturalEnvelopeClosure::ExtractionFailed(ModelError {
                kind: ErrorKind::InvalidResponse.as_str().to_owned(),
                retryable: false,
                message: "provider output was not one JSON object".to_owned(),
                raw_output: Some(completed.text),
                identity: Some(identity.clone()),
                attempts,
                usage: Some(usage),
            }),
        }
    }

    /// Close a recognized non-natural completion with exact model text kept
    /// separate from bounded native-reason prose.
    fn close_non_natural(
        &self,
        invoke_started: &ClockMark,
        identity: &ModelIdentity,
        attempts: Vec<Attempt>,
        non_natural: NonNaturalWireRequest,
        message: String,
    ) -> ModelError {
        ModelError {
            kind: ErrorKind::InvalidResponse.as_str().to_owned(),
            retryable: false,
            message,
            raw_output: non_natural.raw_output,
            identity: Some(identity.clone()),
            attempts,
            usage: Some(CallUsage {
                latency_ms: self.clock.elapsed_ms(invoke_started),
                tokens: non_natural.tokens,
                request_id: non_natural.request_id,
            }),
        }
    }
}

/// Build one native generation request after discovery and preflight. The
/// returned value is borrowed unchanged by every retry.
fn build_generation_request(
    request: &ModelRequest,
    selection: &Selection,
    discovered: &Discovered,
    timeout: Duration,
) -> HttpPostRequest {
    let prepared = selection.provider.build(&BuildInput {
        model: &selection.model,
        messages: request.messages(),
        settings: request.model_settings(),
        schema: request.proposal_schema().as_ref(),
        endpoint: &discovered.endpoint,
        auth: discovered.auth.as_ref(),
    });
    let body = Bytes::from(
        serde_json::to_vec(&prepared.body).expect("a prepared native JSON value is serializable"),
    );
    let mut headers = prepared.headers;
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    HttpPostRequest {
        url: prepared.url,
        headers,
        body,
        timeout,
    }
}

/// Classify one completed Ollama preflight response without creating a
/// generation attempt.
fn classify_preflight_status(status: u16, model: &str) -> Result<(), PreAttemptFailure> {
    if (200..300).contains(&status) {
        Ok(())
    } else if status == 404 {
        Err(PreAttemptFailure::new(
            ErrorKind::ModelNotFound,
            format!("model {model:?} is not present on the ollama daemon"),
        ))
    } else if (300..400).contains(&status) {
        Err(PreAttemptFailure::new(
            ErrorKind::ProviderError,
            format!("ollama preflight returned HTTP {status}"),
        ))
    } else {
        Err(PreAttemptFailure::new(
            ErrorKind::ProviderUnavailable,
            format!("ollama preflight returned HTTP {status}"),
        ))
    }
}

/// Convert one classified discovery or preflight failure into a `ModelError`
/// carrying identity and no generation attempts or raw output. Usage exists
/// only when a completed preflight response reached the invocation owner.
fn pre_attempt_error(
    identity: &ModelIdentity,
    failure: PreAttemptFailure,
    usage: Option<CallUsage>,
) -> ModelError {
    ModelError {
        kind: failure.kind.as_str().to_owned(),
        retryable: false,
        message: failure.message,
        raw_output: None,
        identity: Some(identity.clone()),
        attempts: Vec::new(),
        usage,
    }
}

/// Close an exhausted or non-retryable generation failure with its ordered
/// attempts and only the response facts its terminal try reached.
fn close_generation_failure(
    identity: &ModelIdentity,
    attempts: Vec<Attempt>,
    failure: FailedWireRequest,
    latency_ms: u64,
) -> ModelError {
    let (usage, raw_output) = match failure.observation {
        ResponseObservation::ResponseLess => (None, None),
        ResponseObservation::Response {
            request_id,
            raw_output,
        } => (
            Some(CallUsage {
                latency_ms,
                tokens: None,
                request_id,
            }),
            raw_output,
        ),
    };
    ModelError {
        kind: failure.kind.as_str().to_owned(),
        retryable: failure.retryable,
        message: failure.message,
        raw_output,
        identity: Some(identity.clone()),
        attempts,
        usage,
    }
}

/// Project provider-native reason prose into one bounded human message. Exact
/// model text remains in the separate raw response channel.
fn non_natural_message(reason: &str) -> String {
    format!(
        "provider returned a non-natural completion: {}",
        sanitize_model_output_diagnostic(reason)
    )
}

/// One strict parse of `raw.trim()` to exactly one JSON object.
/// Framing, prose, trailing content, arrays, scalars, and malformed JSON fail.
pub(crate) fn extract_json_object(raw: &str) -> Option<ParsedJsonObject> {
    serde_json::from_str(raw.trim()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_one_bare_object() {
        assert_eq!(
            extract_json_object(" \n{\"answer\":\"hi\"}\t "),
            Some(serde_json::from_value(json!({ "answer": "hi" })).unwrap())
        );
    }

    #[test]
    fn rejects_framing_non_objects_prose_malformed_and_trailing_content() {
        for raw in [
            "```json\n{\"answer\":\"hi\"}\n```",
            "```\n{\"answer\":\"hi\"}\n```",
            "[1,2,3]",
            "\"a string\"",
            "7",
            "true",
            "null",
            "before {\"answer\":\"hi\"}",
            "{\"answer\":\"hi\"",
            "{\"answer\":\"hi\"} after",
            "{\"answer\":\"hi\"}{\"answer\":\"bye\"}",
        ] {
            assert_eq!(extract_json_object(raw), None, "accepted {raw:?}");
        }
    }
}
