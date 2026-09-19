//! Exact closed evidence for one model call nested inside a run step.
//! @sergent/docs/run-record-spec.md

use serde::Serialize;

use crate::model::{
    Attempt, CallUsage, ModelError, ModelIdentity, ModelRequest, ModelRequestCapture,
    ModelResponse, ParsedJsonObject,
};
use crate::proposal::ProposalSchema;

use super::CapturedValue;

/// The four payload facts that become available as a model call advances.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelCallPayloads {
    request: CapturedValue,
    raw_response: Option<String>,
    parsed_json: Option<ParsedJsonObject>,
    parsed_proposal: Option<CapturedValue>,
}

impl ModelCallPayloads {
    /// Borrow the request captured before provider invocation.
    pub fn request(&self) -> &CapturedValue {
        &self.request
    }

    /// Borrow exact reached response or model text, including admitted partial
    /// UTF-8 text from a response-body failure.
    pub fn raw_response(&self) -> Option<&str> {
        self.raw_response.as_deref()
    }

    /// Borrow the provider-admitted JSON object, when one completed.
    pub fn parsed_json(&self) -> Option<&ParsedJsonObject> {
        self.parsed_json.as_ref()
    }

    /// Borrow concrete typed proposal capture after a successful crossing.
    pub fn parsed_proposal(&self) -> Option<&CapturedValue> {
        self.parsed_proposal.as_ref()
    }
}

/// The exact inert evidence for one model call nested inside a Step Record.
/// @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelCallRecord {
    proposal_schema: ProposalSchema,
    model_name: String,
    identity: Option<ModelIdentity>,
    payloads: ModelCallPayloads,
    usage: Option<CallUsage>,
    attempts: Vec<Attempt>,
}

/// Request-only evidence opened before a provider future can start.
pub struct OpenModelCall {
    record: ModelCallRecord,
}

/// Provider-complete evidence awaiting the typed proposal crossing.
pub struct CompletedModelCall {
    record: ModelCallRecord,
}

impl OpenModelCall {
    /// Open request-only evidence from the immutable runtime request.
    pub fn new(request: &ModelRequest) -> Self {
        let request_projection = serde_json::to_value(ModelRequestCapture::from(request))
            .map_err(|error| error.to_string());
        Self {
            record: ModelCallRecord {
                proposal_schema: (**request.proposal_schema()).clone(),
                model_name: request.model_name().to_owned(),
                identity: None,
                payloads: ModelCallPayloads {
                    request: CapturedValue::from_projection(
                        std::any::type_name::<ModelRequest>(),
                        request_projection,
                    ),
                    raw_response: None,
                    parsed_json: None,
                    parsed_proposal: None,
                },
                usage: None,
                attempts: Vec::new(),
            },
        }
    }

    /// Close an interrupted provider await without inventing response facts.
    pub fn interrupted(self) -> ModelCallRecord {
        self.record
    }

    /// Close a provider failure with exactly its reached identity, response
    /// text, usage, and attempts. A response-less later failure may retain
    /// identity and attempts while response text and usage remain null.
    pub fn failed(mut self, error: &ModelError) -> ModelCallRecord {
        self.record.identity = error.identity.clone();
        self.record.payloads.raw_response = error.raw_output.clone();
        self.record.usage = error.usage.clone();
        self.record.attempts = error.attempts.clone();
        self.record
    }

    /// Extend request-only evidence with one completed provider response.
    pub fn completed(
        mut self,
        response: &ModelResponse,
        parsed_json: &ParsedJsonObject,
    ) -> CompletedModelCall {
        self.record.identity = Some(response.identity.clone());
        self.record.payloads.raw_response = Some(response.raw_output.clone());
        self.record.payloads.parsed_json = Some(parsed_json.clone());
        self.record.usage = Some(response.usage.clone());
        self.record.attempts = response.attempts.clone();
        CompletedModelCall {
            record: self.record,
        }
    }
}

impl CompletedModelCall {
    /// Close a failed typed crossing while retaining completed call evidence.
    pub fn rejected(self) -> ModelCallRecord {
        self.record
    }

    /// Close a successful typed crossing with the concrete proposal capture.
    pub fn accepted<T: Serialize + ?Sized>(mut self, proposal: &T) -> ModelCallRecord {
        self.record.payloads.parsed_proposal = Some(CapturedValue::capture(proposal));
        self.record
    }
}

impl ModelCallRecord {
    /// Borrow the canonical schema used for the call.
    pub fn proposal_schema(&self) -> &ProposalSchema {
        &self.proposal_schema
    }

    /// Borrow the caller's full model selection.
    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Borrow resolved provider identity, when reached.
    pub fn identity(&self) -> Option<&ModelIdentity> {
        self.identity.as_ref()
    }

    /// Borrow the exact payload lifecycle.
    pub fn payloads(&self) -> &ModelCallPayloads {
        &self.payloads
    }

    /// Borrow measured usage, when reached.
    pub fn usage(&self) -> Option<&CallUsage> {
        self.usage.as_ref()
    }

    /// Borrow completed provider attempts in order.
    pub fn attempts(&self) -> &[Attempt] {
        &self.attempts
    }
}
