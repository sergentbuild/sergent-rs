//! The vocabulary one adapter speaks: the inputs it receives, the native
//! request it produces, and the outcome of reading one provider envelope. These
//! values cross to the invocation engine and carry nothing proposal- or
//! Operation-shaped. @sergent-rs-providers/docs/providers.md

use reqwest::header::HeaderMap;
use serde_json::Value;

use sergent_rs_core::model::{Message, ModelSettings, TokenCounts};
use sergent_rs_core::proposal::ProposalSchema;

use crate::credentials::{CredentialHeader, Endpoint};

/// The inputs an adapter needs to build one provider request. It carries the
/// canonical schema and the discovered endpoint and auth, nothing proposal- or
/// Operation-shaped.
pub(crate) struct BuildInput<'a> {
    pub model: &'a str,
    pub messages: &'a [Message],
    pub settings: ModelSettings,
    pub schema: &'a ProposalSchema,
    pub endpoint: &'a Endpoint,
    pub auth: Option<&'a CredentialHeader>,
}

/// One built HTTP request: the target URL, extra headers (the JSON body sets
/// the content type), and the JSON body.
pub(crate) struct PreparedRequest {
    pub url: reqwest::Url,
    pub headers: HeaderMap,
    pub body: Value,
}

/// The outcome of reading one typed provider envelope. A recognized
/// non-natural completion keeps native reason prose separate from exact
/// observed model text and never enters proposal parsing.
pub(crate) enum EnvelopeOutcome {
    Completed {
        text: String,
        tokens: Option<TokenCounts>,
    },
    NonNatural {
        reason: String,
        text: Option<String>,
        tokens: Option<TokenCounts>,
    },
}
