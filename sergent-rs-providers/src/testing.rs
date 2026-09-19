//! The sanctioned deterministic test client. It implements the core
//! `ModelClient` with canned JSON outputs, records every request before
//! selection, reuses the real name resolution and identity assembly, and never
//! touches the network, environment, preflight, or clock. An exhausted output
//! queue is a harness error and panics.
//! @sergent-rs-providers/docs/KNOWLEDGE.md

use std::collections::VecDeque;
use std::sync::Mutex;

use sergent_rs_core::error::{ErrorKind, RunError};
use sergent_rs_core::model::{
    CallUsage, ModelClient, ModelError, ModelRequest, ModelResponse, ParsedJsonObject, TokenCounts,
};
use sergent_rs_core::timing::{TimeSpan, Timestamp};

use crate::client::{MAX_ATTEMPTS, extract_json_object};
use crate::evidence::{failure_attempt, success_attempt};
use crate::selection::resolve;

const STATIC_ATTEMPT_DURATION_MS: u64 = 1;
const MICROSECONDS_PER_MILLISECOND: u64 = 1_000;

/// One closed deterministic model outcome. Only variants with living battery
/// or application containment consumers belong in this script.
pub enum StaticLlmOutcome {
    /// Exact semantic text passed through the production strict object parser.
    Output(String),
    /// A response-less timeout after the production attempt budget is spent.
    Timeout,
    /// Response-backed HTTP rate limiting after the attempt budget is spent.
    RateLimited,
}

/// A provider-owned deterministic `ModelClient` for hermetic tests. Battery and
/// example tests consume it through test-only wiring; it never reaches a
/// network, environment, or preflight. Invoking it more times than it has
/// scripted outcomes panics.
pub struct StaticLlmClient {
    outcomes: Mutex<VecDeque<StaticLlmOutcome>>,
    requests: Mutex<Vec<ModelRequest>>,
}

impl StaticLlmClient {
    /// Build a client that serves the given raw outputs, in order, one per call.
    /// Serving more calls than outputs is a harness error and panics.
    pub fn new(outputs: impl IntoIterator<Item = String>) -> Self {
        Self::scripted(outputs.into_iter().map(StaticLlmOutcome::Output))
    }

    /// Build a client from the closed provider-owned outcome script. Serving
    /// more calls than outcomes is a harness error and panics.
    pub fn scripted(outcomes: impl IntoIterator<Item = StaticLlmOutcome>) -> Self {
        Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// Every request received so far, in call order.
    pub fn recorded_requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().expect("static client mutex").clone()
    }
}

/// Supplies stable, non-sensitive evidence for one deterministic canned call.
fn fixed_usage() -> CallUsage {
    CallUsage {
        latency_ms: STATIC_ATTEMPT_DURATION_MS,
        tokens: Some(TokenCounts {
            input: Some(0),
            output: Some(0),
        }),
        request_id: Some("static".to_owned()),
    }
}

/// Build one ordered attempt span from the deterministic timing sequence used
/// by both output and structured-failure evidence.
fn fixed_span(index: usize) -> TimeSpan {
    let started_at = u64::try_from(index)
        .expect("static attempt index fits u64")
        .checked_mul(MICROSECONDS_PER_MILLISECOND)
        .expect("static attempt timestamp fits u64");
    let finished_at = started_at
        .checked_add(MICROSECONDS_PER_MILLISECOND)
        .expect("static attempt timestamp fits u64");
    TimeSpan::closed(
        Timestamp::from_unix_micros(started_at),
        Timestamp::from_unix_micros(finished_at),
        STATIC_ATTEMPT_DURATION_MS,
    )
}

/// Build the exact failed attempts produced when a retryable static outcome
/// exhausts the production provider budget.
fn retryable_attempts(kind: ErrorKind, message: &str) -> Vec<sergent_rs_core::model::Attempt> {
    (0..MAX_ATTEMPTS)
        .map(|index| failure_attempt(fixed_span(index), RunError::of(kind, message), true))
        .collect()
}

/// Close one structured static transport failure with production-shaped
/// identity, attempts, response usage, and null raw response.
fn static_failure(
    identity: sergent_rs_core::model::ModelIdentity,
    kind: ErrorKind,
    message: &str,
    usage: Option<CallUsage>,
) -> ModelError {
    ModelError {
        kind: kind.as_str().to_owned(),
        retryable: true,
        message: message.to_owned(),
        raw_output: None,
        identity: Some(identity),
        attempts: retryable_attempts(kind, message),
        usage,
    }
}

impl ModelClient for StaticLlmClient {
    /// Records the request before real name resolution, consumes one canned
    /// output, and applies the production strict object parser without I/O. An
    /// exhausted queue panics with the request count instead of fabricating a
    /// provider failure for a call that generated nothing.
    async fn invoke(
        &self,
        request: &ModelRequest,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        // Record every request before selection.
        let request_count = {
            let mut requests = self.requests.lock().expect("static client mutex");
            requests.push(request.clone());
            requests.len()
        };

        // Reuse the real name resolution and identity assembly.
        let identity = resolve(request.model_name())?.identity;
        let outcome = self
            .outcomes
            .lock()
            .expect("static client mutex")
            .pop_front();
        let Some(outcome) = outcome else {
            panic!("static client outputs exhausted after request {request_count}")
        };

        match outcome {
            StaticLlmOutcome::Output(raw_output) => close_output(identity, raw_output),
            StaticLlmOutcome::Timeout => Err(static_failure(
                identity,
                ErrorKind::Timeout,
                "provider request timed out",
                None,
            )),
            StaticLlmOutcome::RateLimited => Err(static_failure(
                identity,
                ErrorKind::RateLimited,
                "provider returned HTTP 429",
                Some(CallUsage {
                    latency_ms: u64::try_from(MAX_ATTEMPTS)
                        .expect("static attempt budget fits u64")
                        .checked_mul(STATIC_ATTEMPT_DURATION_MS)
                        .expect("static call latency fits u64"),
                    tokens: None,
                    request_id: None,
                }),
            )),
        }
    }
}

/// Apply the existing strict object path and fixed successful evidence to one
/// exact scripted output.
// The unboxed ModelError is the ModelClient contract this helper closes.
#[allow(clippy::result_large_err)]
fn close_output(
    identity: sergent_rs_core::model::ModelIdentity,
    raw_output: String,
) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
    let usage = fixed_usage();
    let attempt = success_attempt(fixed_span(0));
    match extract_json_object(&raw_output) {
        Some(parsed) => Ok((
            ModelResponse {
                identity,
                raw_output,
                attempts: vec![attempt],
                usage,
            },
            parsed,
        )),
        None => Err(ModelError {
            kind: ErrorKind::InvalidResponse.as_str().to_owned(),
            retryable: false,
            message: "static output was not one JSON object".to_owned(),
            raw_output: Some(raw_output),
            identity: Some(identity),
            attempts: vec![attempt],
            usage: Some(usage),
        }),
    }
}
