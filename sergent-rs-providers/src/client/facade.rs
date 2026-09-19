//! Production and scripted facades for the private provider invocation engine.

use sergent_rs_core::model::{
    ModelClient, ModelError, ModelRequest, ModelResponse, ParsedJsonObject,
};

use crate::credentials::{Endpoints, Environment, ProviderConfig};
use crate::evidence::Clock;
use crate::http::ReqwestHttpClient;
#[cfg(test)]
use crate::http::ScriptedHttpClient;

use super::invocation::ProviderClient;

/// The concrete reqwest-backed model transport: one non-generic entry point
/// implementing core `ModelClient` through the private provider engine.
/// @sergent/docs/trust-boundaries.md boundary 5
pub struct LlmClient {
    client: ProviderClient<ReqwestHttpClient>,
}

impl LlmClient {
    /// Construct the production transport reading the process environment.
    pub fn new() -> Self {
        Self {
            client: ProviderClient::new(
                ReqwestHttpClient::production(),
                Environment::Process,
                Endpoints::production(),
                Clock::production(),
            ),
        }
    }

    /// Construct the production transport from one immutable provider
    /// configuration without reading process state during invocation.
    pub fn configured(config: ProviderConfig) -> Self {
        Self {
            client: ProviderClient::new(
                ReqwestHttpClient::production(),
                Environment::Configured(config),
                Endpoints::production(),
                Clock::production(),
            ),
        }
    }
}

impl Default for LlmClient {
    /// Builds the same process-configured provider client as [`Self::new`].
    fn default() -> Self {
        Self::new()
    }
}

impl ModelClient for LlmClient {
    /// Runs the complete provider boundary, including selection, discovery,
    /// native envelope admission, bounded retry, strict parsing, and evidence.
    async fn invoke(
        &self,
        request: &ModelRequest,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        self.client.invoke(request).await
    }
}

/// The crate-private provider test client. It reaches the exact same invocation
/// engine through an in-memory scripted HTTP specialization.
#[cfg(test)]
pub(crate) struct ScriptedLlmClient {
    client: ProviderClient<ScriptedHttpClient>,
}

#[cfg(test)]
impl ScriptedLlmClient {
    /// Injects in-memory HTTP, configuration, endpoints, and timing while
    /// retaining the production provider invocation pipeline.
    pub(crate) fn new(
        http: ScriptedHttpClient,
        env: crate::credentials::MapEnv,
        endpoints: Endpoints,
        clock: Clock,
    ) -> Self {
        Self {
            client: ProviderClient::new(http, Environment::Map(env), endpoints, clock),
        }
    }
}

#[cfg(test)]
impl ModelClient for ScriptedLlmClient {
    /// Exercises the real provider boundary against the injected deterministic
    /// HTTP crossing and evidence clock.
    async fn invoke(
        &self,
        request: &ModelRequest,
    ) -> Result<(ModelResponse, ParsedJsonObject), ModelError> {
        self.client.invoke(request).await
    }
}
