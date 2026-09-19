//! Hermetic tests for provider selection and credential discovery. No network,
//! no real credentials.

use sergent_rs_core::error::ErrorKind;

use crate::credentials::{CREDENTIAL_ENV_VARS, Endpoints, MapEnv, ProviderConfig, discover};
use crate::selection::{Provider, resolve};

/// Uses the documented cloud hosts while credential tests remain free of I/O.
fn endpoints() -> Endpoints {
    Endpoints::production()
}

#[test]
fn resolves_provider_and_model() {
    let selection = resolve("openai/gpt-5.5").expect("resolves");
    assert_eq!(selection.provider, Provider::OpenAi);
    assert_eq!(selection.model, "gpt-5.5");
    assert_eq!(selection.identity.provider, "openai");
    assert_eq!(selection.identity.model, "gpt-5.5");
    assert!(selection.identity.sdk_package.is_none());
}

#[test]
fn splits_only_at_the_first_slash_and_keeps_colons() {
    assert_eq!(resolve("ollama/llama3:8b").unwrap().model, "llama3:8b");
    assert_eq!(resolve("openai/a/b").unwrap().model, "a/b");
}

#[test]
fn preserves_the_complete_nonempty_model_remainder() {
    let selection = resolve("openai/ gpt-5.5/a  ").unwrap();
    assert_eq!(selection.model, " gpt-5.5/a  ");
    assert_eq!(selection.identity.model, " gpt-5.5/a  ");
}

#[test]
fn malformed_names_are_invalid_model_name_with_no_evidence() {
    for name in ["no-slash", "/model", "openai/"] {
        let error = resolve(name).unwrap_err();
        assert_eq!(
            error.kind,
            ErrorKind::InvalidModelName.as_str(),
            "for {name:?}"
        );
        assert!(error.identity.is_none());
        assert!(error.attempts.is_empty());
        assert!(error.usage.is_none());
    }
}

#[test]
fn a_whitespace_only_remainder_is_still_opaque_model_data() {
    let selection = resolve("openai/   ").unwrap();
    assert_eq!(selection.model, "   ");
    assert_eq!(selection.identity.model, "   ");
}

#[test]
fn unknown_provider_is_a_local_failure_with_no_evidence() {
    let error = resolve("bogus/model").unwrap_err();
    assert_eq!(error.kind, ErrorKind::UnknownProvider.as_str());
    assert!(error.identity.is_none());
    assert!(error.attempts.is_empty());
}

#[test]
fn openai_requires_its_key() {
    let error = discover(Provider::OpenAi, &MapEnv::new(), &endpoints()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::MissingCredentials);
}

#[test]
fn openai_key_becomes_a_bearer_header() {
    let env = MapEnv::new().with("OPENAI_API_KEY", "sk-abc");
    let discovered = discover(Provider::OpenAi, &env, &endpoints()).expect("ok");
    assert_credential(discovered, "authorization", "Bearer sk-abc");
}

#[test]
fn present_but_invalid_credential_is_missing_credentials() {
    let env = MapEnv::new().with("OPENAI_API_KEY", "sk-valid-prefix\ninjected");
    let error = discover(Provider::OpenAi, &env, &endpoints()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::MissingCredentials);
    assert!(!error.message.contains("sk-valid-prefix"));
}

#[test]
fn anthropic_key_becomes_an_api_key_header() {
    let env = MapEnv::new().with("ANTHROPIC_API_KEY", "an-key");
    let discovered = discover(Provider::Anthropic, &env, &endpoints()).expect("ok");
    assert_credential(discovered, "x-api-key", "an-key");
}

#[test]
fn gemini_accepts_the_google_fallback() {
    let env = MapEnv::new().with("GOOGLE_API_KEY", "g-key");
    let discovered = discover(Provider::Gemini, &env, &endpoints()).expect("ok");
    assert_credential(discovered, "x-goog-api-key", "g-key");
}

#[test]
fn gemini_prefers_its_own_key_over_the_fallback() {
    let env = MapEnv::new()
        .with("GEMINI_API_KEY", "primary")
        .with("GOOGLE_API_KEY", "fallback");
    let discovered = discover(Provider::Gemini, &env, &endpoints()).expect("ok");
    assert_credential(discovered, "x-goog-api-key", "primary");
}

#[test]
fn gemini_without_any_key_is_missing_credentials() {
    let error = discover(Provider::Gemini, &MapEnv::new(), &endpoints()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::MissingCredentials);
}

#[test]
fn immutable_configuration_retains_only_recognized_nonempty_values() {
    let config = ProviderConfig::from_values([
        ("OPENAI_API_KEY", "fixed-key"),
        ("ANTHROPIC_API_KEY", ""),
        ("UNRELATED_SETTING", "application-value"),
    ]);

    let discovered = discover(Provider::OpenAi, &config, &endpoints()).expect("ok");
    assert_credential(discovered, "authorization", "Bearer fixed-key");
    let error = discover(Provider::Anthropic, &config, &endpoints()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::MissingCredentials);
}

#[test]
fn immutable_configuration_merges_fallbacks_without_overwriting() {
    let primary = ProviderConfig::from_values([("GEMINI_API_KEY", "primary")]);
    let fallback = ProviderConfig::from_values([
        ("GEMINI_API_KEY", "fallback"),
        ("ANTHROPIC_API_KEY", "other"),
    ]);

    let config = primary.with_fallbacks(fallback);
    let discovered = discover(Provider::Gemini, &config, &endpoints()).expect("ok");
    assert_credential(discovered, "x-goog-api-key", "primary");
    let discovered = discover(Provider::Anthropic, &config, &endpoints()).expect("ok");
    assert_credential(discovered, "x-api-key", "other");
}

#[test]
fn ollama_defaults_the_base_and_needs_no_key() {
    let discovered = discover(Provider::Ollama, &MapEnv::new(), &endpoints()).expect("ok");
    assert_eq!(discovered.endpoint.as_str(), "http://127.0.0.1:11434/");
    assert!(discovered.auth.is_none());
}

#[test]
fn ollama_optional_bearer_is_sent_when_set() {
    let env = MapEnv::new().with("OLLAMA_API_KEY", "o-key");
    let discovered = discover(Provider::Ollama, &env, &endpoints()).expect("ok");
    assert_credential(discovered, "authorization", "Bearer o-key");
}

#[test]
fn ollama_rejects_a_base_url_ending_in_api() {
    let env = MapEnv::new().with("SERGENT_OLLAMA_BASE_URL", "http://127.0.0.1:11434/api");
    let error = discover(Provider::Ollama, &env, &endpoints()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::ProviderUnavailable);
}

#[test]
fn credential_deny_list_is_closed_and_complete() {
    for var in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
        "OLLAMA_API_KEY",
        "SERGENT_OLLAMA_BASE_URL",
    ] {
        assert!(
            CREDENTIAL_ENV_VARS.contains(&var),
            "{var} missing from the deny-list"
        );
    }
    assert_eq!(CREDENTIAL_ENV_VARS.len(), 6);
}

/// Verifies discovered authorization is exact and its secret-bearing value is marked sensitive.
fn assert_credential(
    discovered: crate::credentials::Discovered,
    expected_name: &str,
    expected_value: &str,
) {
    let credential = discovered.auth.expect("credential header");
    assert_eq!(credential.name.as_str(), expected_name);
    assert_eq!(credential.value.to_str().unwrap(), expected_value);
    assert!(credential.value.is_sensitive());
}
