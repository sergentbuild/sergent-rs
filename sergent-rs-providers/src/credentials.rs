//! Credential and endpoint discovery for each adapter. A closed read source
//! selects process, immutable configured, or deterministic test values. A
//! missing required key and an Ollama base URL ending in `/api` both fail here
//! with no network access.
//! @sergent/docs/trust-boundaries.md boundary 5

use reqwest::Url;
use reqwest::header::{AUTHORIZATION, HeaderName, HeaderValue};
use sergent_rs_core::error::ErrorKind;

use crate::selection::Provider;

/// The closed deny-list of every provider credential and endpoint environment
/// variable. Hermetic test suites strip exactly these.
pub const CREDENTIAL_ENV_VARS: &[&str] = &[
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "OLLAMA_API_KEY",
    "SERGENT_OLLAMA_BASE_URL",
];

const DEFAULT_OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434";

/// An immutable snapshot of recognized provider credentials and endpoints.
/// Values are never exposed through formatting. Applications may combine a
/// process snapshot with lower-precedence values from their own configuration.
/// @sergent/docs/trust-boundaries.md boundary 5
#[derive(Clone, Default)]
pub struct ProviderConfig {
    values: std::collections::BTreeMap<&'static str, String>,
}

impl ProviderConfig {
    /// Retains nonempty values for recognized provider variables. Unknown
    /// names are application data and do not enter provider configuration.
    pub fn from_values<I, K, V>(values: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: Into<String>,
    {
        let mut config = Self::default();
        for (key, value) in values {
            let Some(variable) = recognized_variable(key.as_ref()) else {
                continue;
            };
            let value = value.into();
            if !value.is_empty() {
                config.values.insert(variable, value);
            }
        }
        config
    }

    /// Captures recognized values currently present in the process
    /// environment without mutating process-wide state.
    pub fn from_process() -> Self {
        Self::from_values(
            CREDENTIAL_ENV_VARS.iter().filter_map(|variable| {
                std::env::var(variable).ok().map(|value| (*variable, value))
            }),
        )
    }

    /// Fills missing values from a lower-precedence immutable configuration.
    /// Existing values always win.
    pub fn with_fallbacks(mut self, fallbacks: Self) -> Self {
        for (variable, value) in fallbacks.values {
            self.values.entry(variable).or_insert(value);
        }
        self
    }
}

/// Maps one external key spelling to its recognized static identity.
fn recognized_variable(key: &str) -> Option<&'static str> {
    CREDENTIAL_ENV_VARS
        .iter()
        .copied()
        .find(|variable| *variable == key)
}

/// A read-only provider-value source. Production reads the process or one
/// immutable configuration; tests inject a fixed map.
pub(crate) trait EnvSource: Send + Sync {
    /// Reads one raw optional value; empty-value normalization belongs to
    /// credential discovery rather than the source.
    fn get(&self, key: &str) -> Option<String>;
}

/// Closed value dispatch: production reads the process or one immutable
/// configuration; tests inject one fixed map without a dynamic extension seam.
pub(crate) enum Environment {
    Process,
    Configured(ProviderConfig),
    #[cfg(test)]
    Map(MapEnv),
}

impl EnvSource for Environment {
    /// Dispatches each invocation-time read to the process or injected map
    /// without adding normalization or caching.
    fn get(&self, key: &str) -> Option<String> {
        match self {
            Self::Process => std::env::var(key).ok(),
            Self::Configured(config) => config.get(key),
            #[cfg(test)]
            Self::Map(env) => env.get(key),
        }
    }
}

impl EnvSource for ProviderConfig {
    /// Reads one cloned value from the immutable recognized-value snapshot.
    fn get(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }
}

/// The host roots for the three cloud adapters. Production defaults are the
/// documented API hosts; the Ollama base is discovered from the environment.
#[derive(Clone, Debug)]
pub(crate) struct Endpoints {
    pub openai: Endpoint,
    pub anthropic: Endpoint,
    pub gemini: Endpoint,
}

impl Endpoints {
    /// Supplies the documented cloud API hosts used when no test endpoints are
    /// injected; Ollama remains environment-owned.
    pub(crate) fn production() -> Self {
        Self {
            openai: Endpoint::fixed("https://api.openai.com"),
            anthropic: Endpoint::fixed("https://api.anthropic.com"),
            gemini: Endpoint::fixed("https://generativelanguage.googleapis.com"),
        }
    }

    /// Validates injected test hosts through the same endpoint construction
    /// used for process configuration.
    #[cfg(test)]
    pub(crate) fn scripted(openai: &str, anthropic: &str, gemini: &str) -> Self {
        Self {
            openai: Endpoint::parse(openai).expect("valid scripted OpenAI endpoint"),
            anthropic: Endpoint::parse(anthropic).expect("valid scripted Anthropic endpoint"),
            gemini: Endpoint::parse(gemini).expect("valid scripted Gemini endpoint"),
        }
    }
}

/// One validated hierarchical HTTP endpoint. Query, fragment, and embedded
/// credentials are rejected once at discovery so later paths compose safely.
#[derive(Clone, Debug)]
pub(crate) struct Endpoint(Url);

impl Endpoint {
    /// Validates one external endpoint before it becomes trusted construction
    /// data.
    pub(crate) fn parse(raw: &str) -> Result<Self, PreAttemptFailure> {
        let url = Url::parse(raw).map_err(|_| invalid_endpoint())?;
        let valid_scheme = matches!(url.scheme(), "http" | "https");
        if !valid_scheme
            || url.cannot_be_a_base()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid_endpoint());
        }
        Ok(Self(url))
    }

    /// Builds one hard-coded endpoint whose literal is a crate
    /// construction invariant.
    fn fixed(raw: &str) -> Self {
        Self::parse(raw).expect("fixed provider endpoint is valid")
    }

    /// Appends trusted structural path segments without string concatenation.
    pub(crate) fn with_segments<const N: usize>(&self, segments: [&str; N]) -> Url {
        let mut url = self.0.clone();
        while url.path().len() > 1 && url.path().ends_with('/') {
            url.path_segments_mut()
                .expect("validated endpoint is hierarchical")
                .pop_if_empty();
        }
        let mut path = url
            .path_segments_mut()
            .expect("validated endpoint is hierarchical");
        path.pop_if_empty();
        for segment in segments {
            path.push(segment);
        }
        drop(path);
        url
    }

    /// Reports whether the normalized base path ends in the forbidden Ollama
    /// API segment.
    fn ends_in_api(&self) -> bool {
        self.0.path().trim_end_matches('/').ends_with("/api")
    }

    /// Exposes the normalized endpoint spelling to owner tests.
    #[cfg(test)]
    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// The resolved endpoint and optional authorization header for one call.
#[derive(Debug)]
pub(crate) struct Discovered {
    pub endpoint: Endpoint,
    pub auth: Option<CredentialHeader>,
}

/// One credential converted to HTTP types and marked sensitive before any
/// attempt can begin.
#[derive(Clone, Debug)]
pub(crate) struct CredentialHeader {
    pub name: HeaderName,
    pub value: HeaderValue,
}

/// One failure classified before any generation attempt can exist. Discovery
/// and the Ollama preflight both fail closed this way, and the invocation owner
/// converts the value into a `ModelError` carrying zero attempts.
#[derive(Debug)]
pub(crate) struct PreAttemptFailure {
    pub kind: ErrorKind,
    pub message: String,
}

impl PreAttemptFailure {
    /// Classify one pre-attempt failure with its closed kind and a sanitized
    /// message that never carries a credential.
    pub(crate) fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// Discover credentials and endpoint for a provider. Failures carry a closed
/// error kind and never touch the network.
pub(crate) fn discover<E: EnvSource>(
    provider: Provider,
    env: &E,
    endpoints: &Endpoints,
) -> Result<Discovered, PreAttemptFailure> {
    match provider {
        Provider::OpenAi => Ok(Discovered {
            endpoint: endpoints.openai.clone(),
            auth: Some(credential_header(
                AUTHORIZATION,
                format!("Bearer {}", required(env, "OPENAI_API_KEY")?),
                "OPENAI_API_KEY",
            )?),
        }),
        Provider::Anthropic => Ok(Discovered {
            endpoint: endpoints.anthropic.clone(),
            auth: Some(credential_header(
                HeaderName::from_static("x-api-key"),
                required(env, "ANTHROPIC_API_KEY")?,
                "ANTHROPIC_API_KEY",
            )?),
        }),
        Provider::Gemini => {
            let key = present(env, "GEMINI_API_KEY")
                .or_else(|| present(env, "GOOGLE_API_KEY"))
                .ok_or(PreAttemptFailure::new(
                    ErrorKind::MissingCredentials,
                    "GEMINI_API_KEY or GOOGLE_API_KEY is required",
                ))?;
            Ok(Discovered {
                endpoint: endpoints.gemini.clone(),
                auth: Some(credential_header(
                    HeaderName::from_static("x-goog-api-key"),
                    key,
                    "GEMINI_API_KEY or GOOGLE_API_KEY",
                )?),
            })
        }
        Provider::Ollama => {
            let base = present(env, "SERGENT_OLLAMA_BASE_URL")
                .unwrap_or_else(|| DEFAULT_OLLAMA_BASE_URL.to_owned());
            let endpoint = Endpoint::parse(&base)?;
            if endpoint.ends_in_api() {
                return Err(PreAttemptFailure::new(
                    ErrorKind::ProviderUnavailable,
                    "SERGENT_OLLAMA_BASE_URL must not end in /api",
                ));
            }
            let auth = present(env, "OLLAMA_API_KEY")
                .map(|key| {
                    credential_header(AUTHORIZATION, format!("Bearer {key}"), "OLLAMA_API_KEY")
                })
                .transpose()?;
            Ok(Discovered { endpoint, auth })
        }
    }
}

/// Produces one sanitized endpoint failure without echoing external data.
fn invalid_endpoint() -> PreAttemptFailure {
    PreAttemptFailure::new(
        ErrorKind::ProviderUnavailable,
        "provider endpoint is not a valid HTTP base URL",
    )
}

/// Converts a discovered credential into a sensitive typed header before any
/// attempt; malformed values fail closed without exposing the credential.
fn credential_header(
    name: HeaderName,
    value: String,
    variable: &str,
) -> Result<CredentialHeader, PreAttemptFailure> {
    let mut value = HeaderValue::from_str(&value).map_err(|_| {
        PreAttemptFailure::new(
            ErrorKind::MissingCredentials,
            format!("{variable} cannot form a valid HTTP credential"),
        )
    })?;
    value.set_sensitive(true);
    Ok(CredentialHeader { name, value })
}

/// Treats an absent or exactly empty environment value as unset while
/// preserving every byte of a present nonempty value.
fn present<E: EnvSource>(env: &E, var: &str) -> Option<String> {
    env.get(var).filter(|value| !value.is_empty())
}

/// Requires a nonempty discovered value and maps absence to the closed
/// missing-credentials failure before model I/O.
fn required<E: EnvSource>(env: &E, var: &str) -> Result<String, PreAttemptFailure> {
    present(env, var).ok_or(PreAttemptFailure::new(
        ErrorKind::MissingCredentials,
        format!("{var} is required"),
    ))
}

/// Deterministic test environment with exact stored values and an optional
/// clock effect on every read.
#[cfg(test)]
pub(crate) struct MapEnv {
    values: std::collections::HashMap<String, String>,
    clock_advance: Option<(crate::evidence::ManualClock, u64)>,
}

#[cfg(test)]
impl MapEnv {
    /// Creates an empty environment with no timing side effect.
    pub(crate) fn new() -> Self {
        Self {
            values: std::collections::HashMap::new(),
            clock_advance: None,
        }
    }

    /// Inserts or replaces one exact test environment value.
    pub(crate) fn with(mut self, key: &str, value: &str) -> Self {
        self.values.insert(key.to_owned(), value.to_owned());
        self
    }

    /// Configures every subsequent read to advance the shared manual clock by
    /// the given milliseconds.
    pub(crate) fn advancing_clock_on_read(
        mut self,
        clock: crate::evidence::ManualClock,
        milliseconds: u64,
    ) -> Self {
        self.clock_advance = Some((clock, milliseconds));
        self
    }
}

#[cfg(test)]
impl EnvSource for MapEnv {
    /// Applies the configured read-time effect, then returns a cloned exact
    /// value without consulting the process environment.
    fn get(&self, key: &str) -> Option<String> {
        if let Some((clock, milliseconds)) = &self.clock_advance {
            clock.advance(*milliseconds);
        }
        self.values.get(key).cloned()
    }
}
