//! Message, credential, status, and token policy every adapter shares.
//! Provider-native reasons, endpoints, and native bodies stay in each adapter;
//! only rules the provider contract states once for all four live here.
//! @sergent-rs-providers/docs/providers.md

use reqwest::header::HeaderMap;

use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::{ImagePart, Message, MessageRole, TokenCounts};

use crate::credentials::CredentialHeader;

/// The shared non-success HTTP status mapping: 429 rate limited and 5xx
/// unavailable are retryable, every other status ends the invoke.
pub(super) fn generic_status(status: u16) -> (ErrorKind, bool) {
    match status {
        429 => (ErrorKind::RateLimited, true),
        500..=599 => (ErrorKind::ProviderUnavailable, true),
        _ => (ErrorKind::ProviderError, false),
    }
}

/// Clone one prevalidated credential into a typed request header collection.
pub(crate) fn credential_headers(auth: Option<&CredentialHeader>) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if let Some(auth) = auth {
        headers.insert(auth.name.clone(), auth.value.clone());
    }
    headers
}

/// The concatenated system-message text, when any system message is present.
pub(super) fn system_text(messages: &[Message]) -> Option<String> {
    let joined = messages
        .iter()
        .filter(|message| message.role() == MessageRole::System)
        .map(Message::content)
        .collect::<Vec<_>>()
        .join("\n");
    (!joined.is_empty()).then_some(joined)
}

/// The user messages, in order.
pub(super) fn user_messages(messages: &[Message]) -> impl Iterator<Item = &Message> {
    messages
        .iter()
        .filter(|message| message.role() == MessageRole::User)
}

/// A `data:` URI for one bounded PNG image part.
pub(super) fn data_uri(image: &ImagePart) -> String {
    format!("data:{};base64,{}", image.media_type(), image.data_base64())
}

/// Normalized token counts, or `None` when the provider reported neither side.
pub(super) fn token_counts(input: Option<u64>, output: Option<u64>) -> Option<TokenCounts> {
    (input.is_some() || output.is_some()).then_some(TokenCounts { input, output })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_status_matrix() {
        assert_eq!(generic_status(429), (ErrorKind::RateLimited, true));
        assert_eq!(generic_status(503), (ErrorKind::ProviderUnavailable, true));
        assert_eq!(generic_status(400), (ErrorKind::ProviderError, false));
        assert_eq!(generic_status(401), (ErrorKind::ProviderError, false));
        assert_eq!(generic_status(404), (ErrorKind::ProviderError, false));
    }

    #[test]
    fn system_text_joins_only_system_messages() {
        let messages = vec![
            Message::system("one"),
            Message::user("ignored"),
            Message::system("two"),
        ];
        assert_eq!(system_text(&messages), Some("one\ntwo".to_owned()));
        assert_eq!(system_text(&[Message::user("only user")]), None);
    }
}
