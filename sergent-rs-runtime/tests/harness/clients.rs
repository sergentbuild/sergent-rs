//! The `ModelClient` fakes: `CannedClient` returns canned JSON by schema name,
//! `ParkedClient` gates on a notify for deterministic await interruption, and
//! `ErrorClient` always fails. `fake_response` builds their response evidence.

use std::sync::{Arc, Mutex};

use serde_json::Value;

use sergent_rs_core::error::{ErrorKind, RunError};
use sergent_rs_core::model::{
    Attempt, CallUsage, ModelClient, ModelError, ModelIdentity, ModelRequest, ModelResponse,
    ParsedJsonObject, TokenCounts,
};

pub fn fake_response(raw: String) -> ModelResponse {
    use sergent_rs_core::timing::{TimeSpan, Timestamp};
    ModelResponse {
        identity: ModelIdentity {
            provider: "fake".to_owned(),
            model: "m1".to_owned(),
            sdk_package: None,
            sdk_version: None,
        },
        raw_output: raw,
        attempts: vec![Attempt::success(TimeSpan::closed(
            Timestamp::from_unix_micros(0),
            Timestamp::from_unix_micros(1_000),
            1,
        ))],
        usage: CallUsage {
            latency_ms: 1,
            tokens: Some(TokenCounts {
                input: Some(3),
                output: Some(2),
            }),
            request_id: Some("req_1".to_owned()),
        },
    }
}

/// A client that returns canned parsed JSON, choosing by schema name so one
/// client serves both the Intent and Plan calls in a run. Every request is
/// recorded before selection.
pub struct CannedClient {
    pub intent_json: ParsedJsonObject,
    pub plan_json: ParsedJsonObject,
    pub requests: Arc<Mutex<Vec<ModelRequest>>>,
}

impl CannedClient {
    pub fn new(intent_json: ParsedJsonObject, plan_json: ParsedJsonObject) -> Self {
        Self {
            intent_json,
            plan_json,
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn parsed_for(&self, request: &ModelRequest) -> ParsedJsonObject {
        if request.proposal_schema().name() == "PlanProposal" {
            self.plan_json.clone()
        } else {
            self.intent_json.clone()
        }
    }
}

impl ModelClient for CannedClient {
    async fn invoke(
        &self,
        request: &ModelRequest,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        self.requests.lock().unwrap().push(request.clone());
        let parsed = self.parsed_for(request);
        Ok((
            fake_response(Value::Object(parsed.clone()).to_string()),
            parsed,
        ))
    }
}

/// A client whose invoke parks on a notify gate until the test releases it. Used
/// to hold a run at a provider await for deterministic cancellation and progress
/// observation, with no sleeps.
pub struct ParkedClient {
    pub gate: Arc<tokio::sync::Notify>,
    pub entered: Arc<tokio::sync::Notify>,
    pub inner: CannedClient,
}

impl ParkedClient {
    pub fn new(intent_json: ParsedJsonObject, plan_json: ParsedJsonObject) -> Self {
        Self {
            gate: Arc::new(tokio::sync::Notify::new()),
            entered: Arc::new(tokio::sync::Notify::new()),
            inner: CannedClient::new(intent_json, plan_json),
        }
    }

    pub fn gate(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.gate)
    }

    pub fn entered(&self) -> Arc<tokio::sync::Notify> {
        Arc::clone(&self.entered)
    }
}

impl ModelClient for ParkedClient {
    async fn invoke(
        &self,
        request: &ModelRequest,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        self.inner.requests.lock().unwrap().push(request.clone());
        self.entered.notify_one();
        self.gate.notified().await;
        let parsed = self.inner.parsed_for(request);
        Ok((
            fake_response(Value::Object(parsed.clone()).to_string()),
            parsed,
        ))
    }
}

/// A client that always fails with either pre-call evidence or one completed
/// provider envelope.
pub struct ErrorClient {
    kind: ErrorKind,
    raw_output: Option<String>,
    identity: Option<ModelIdentity>,
    attempts: Vec<Attempt>,
    usage: Option<CallUsage>,
}

impl ErrorClient {
    pub fn pre_call(kind: ErrorKind) -> Self {
        Self {
            kind,
            raw_output: None,
            identity: None,
            attempts: Vec::new(),
            usage: None,
        }
    }

    pub fn completed_envelope(
        kind: ErrorKind,
        raw_output: impl Into<String>,
        identity: Option<ModelIdentity>,
        usage: Option<CallUsage>,
    ) -> Self {
        use sergent_rs_core::timing::{TimeSpan, Timestamp};

        let mut attempt_error = RunError::of(kind, "provider attempt failed");
        attempt_error.metadata = serde_json::json!({ "retryable": false })
            .as_object()
            .expect("object")
            .clone();

        Self {
            kind,
            raw_output: Some(raw_output.into()),
            identity,
            attempts: vec![Attempt::failure(
                TimeSpan::closed(
                    Timestamp::from_unix_micros(10_000),
                    Timestamp::from_unix_micros(12_000),
                    2,
                ),
                false,
                attempt_error,
            )],
            usage,
        }
    }
}

impl ModelClient for ErrorClient {
    async fn invoke(
        &self,
        _request: &ModelRequest,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        Err(ModelError {
            kind: self.kind.as_str().to_owned(),
            retryable: false,
            message: "provider failed".to_owned(),
            raw_output: self.raw_output.clone(),
            identity: self.identity.clone(),
            attempts: self.attempts.clone(),
            usage: self.usage.clone(),
        })
    }
}
