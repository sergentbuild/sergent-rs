//! Send and classify exactly one generation wire request. Retry, preflight,
//! strict semantic parsing, and final call evidence remain invocation-owned.

use sergent_rs_core::error::ErrorKind;
use sergent_rs_core::model::TokenCounts;

use crate::adapter::EnvelopeOutcome;
use crate::http::{BodyCollectionFailure, HttpClient, HttpFailure, HttpPostRequest, HttpResponse};
use crate::selection::Provider;

/// The maximum complete generation envelope admitted from an external
/// provider before native classification.
pub(crate) const MAX_GENERATION_BODY_BYTES: usize = 1024 * 1024;

/// The classified outcome of one completed generation wire request.
pub(super) enum WireOutcome {
    Completed(CompletedWireRequest),
    NonNatural(NonNaturalWireRequest),
    Failure(FailedWireRequest),
}

/// Natural completion facts produced by one generation wire response.
pub(super) struct CompletedWireRequest {
    pub(super) text: String,
    pub(super) tokens: Option<TokenCounts>,
    pub(super) request_id: Option<String>,
}

/// Recognized non-natural completion facts produced by one wire response.
pub(super) struct NonNaturalWireRequest {
    pub(super) reason: String,
    pub(super) raw_output: Option<String>,
    pub(super) tokens: Option<TokenCounts>,
    pub(super) request_id: Option<String>,
}

/// One transport, HTTP-status, or invalid-envelope classification.
pub(super) struct FailedWireRequest {
    pub(super) kind: ErrorKind,
    pub(super) retryable: bool,
    pub(super) message: String,
    pub(super) observation: ResponseObservation,
}

/// Whether a failed generation try reached provider response metadata and the
/// exact admitted text available from that response.
pub(super) enum ResponseObservation {
    ResponseLess,
    Response {
        request_id: Option<String>,
        raw_output: Option<String>,
    },
}

/// Send exactly one generation request and classify its completed outcome.
/// Dropping this future produces no outcome for the invocation engine to
/// record.
pub(super) async fn send_and_classify<H: HttpClient>(
    http: &H,
    provider: Provider,
    request: &HttpPostRequest,
) -> WireOutcome {
    match http.post(request).await {
        Ok(response) => classify_response(provider, response).await,
        Err(failure) => map_http_failure(failure, ResponseObservation::ResponseLess),
    }
}

/// Classify response metadata before deciding whether its body is relevant.
async fn classify_response(provider: Provider, response: HttpResponse) -> WireOutcome {
    let request_id = provider.request_id(&response.headers);
    if !(200..300).contains(&response.status) {
        let (kind, retryable) = provider.map_generation_status(response.status);
        return WireOutcome::Failure(FailedWireRequest {
            kind,
            retryable,
            message: format!("provider returned HTTP {}", response.status),
            observation: ResponseObservation::Response {
                request_id,
                raw_output: None,
            },
        });
    }
    classify_success_envelope(provider, response, request_id).await
}

/// Classify one 2xx body through its provider-native minimal envelope and
/// capture request-ID headers only where the provider contract permits.
async fn classify_success_envelope(
    provider: Provider,
    response: HttpResponse,
    request_id: Option<String>,
) -> WireOutcome {
    if !uses_supported_charset(&response.headers) {
        return WireOutcome::Failure(FailedWireRequest {
            kind: ErrorKind::InvalidResponse,
            retryable: false,
            message: "provider response declared an unsupported charset".to_owned(),
            observation: ResponseObservation::Response {
                request_id,
                raw_output: None,
            },
        });
    }
    let body = match response.collect_body(MAX_GENERATION_BODY_BYTES).await {
        Ok(body) => body,
        Err(failure) => return map_body_failure(failure, request_id),
    };
    match provider.extract(&body) {
        Some(EnvelopeOutcome::Completed { text, tokens }) => {
            WireOutcome::Completed(CompletedWireRequest {
                text,
                tokens,
                request_id,
            })
        }
        Some(EnvelopeOutcome::NonNatural {
            reason,
            text,
            tokens,
        }) => WireOutcome::NonNatural(NonNaturalWireRequest {
            reason,
            raw_output: text,
            tokens,
            request_id,
        }),
        None => WireOutcome::Failure(FailedWireRequest {
            kind: ErrorKind::InvalidResponse,
            retryable: false,
            message: "provider returned an invalid success envelope".to_owned(),
            observation: ResponseObservation::Response {
                request_id,
                raw_output: String::from_utf8(body).ok(),
            },
        }),
    }
}

/// Map one typed HTTP failure without echoing transport details, URLs, or
/// headers into evidence.
fn map_http_failure(failure: HttpFailure, observation: ResponseObservation) -> WireOutcome {
    let (kind, retryable, message) = match failure {
        HttpFailure::Timeout => (ErrorKind::Timeout, true, "provider request timed out"),
        HttpFailure::Connection => (
            ErrorKind::ProviderUnavailable,
            true,
            "could not connect to the provider",
        ),
        HttpFailure::Send => (
            ErrorKind::ProviderUnavailable,
            true,
            "provider transport failure",
        ),
        HttpFailure::BodyRead => (
            ErrorKind::ProviderUnavailable,
            true,
            "provider response body could not be read",
        ),
        HttpFailure::BodyTooLarge => (
            ErrorKind::InvalidResponse,
            false,
            "provider response body exceeded the byte limit",
        ),
    };
    WireOutcome::Failure(FailedWireRequest {
        kind,
        retryable,
        message: message.to_owned(),
        observation,
    })
}

/// Preserve bounded admitted UTF-8 bytes observed before response-body I/O
/// failed, then retain the already reached response identity.
fn map_body_failure(failure: BodyCollectionFailure, request_id: Option<String>) -> WireOutcome {
    let raw_output = if failure.partial.is_empty() {
        None
    } else {
        String::from_utf8(failure.partial).ok()
    };
    map_http_failure(
        failure.failure,
        ResponseObservation::Response {
            request_id,
            raw_output,
        },
    )
}

/// Admits absent charset metadata or an exact case-insensitive UTF-8
/// declaration. Unsupported and malformed charset declarations fail closed.
fn uses_supported_charset(headers: &reqwest::header::HeaderMap) -> bool {
    let mut values = headers.get_all(reqwest::header::CONTENT_TYPE).iter();
    let Some(value) = values.next() else {
        return true;
    };
    if values.next().is_some() {
        return false;
    }
    let Ok(value) = value.to_str() else {
        return false;
    };
    let Ok(mut parameters) = ParameterScanner::new(value) else {
        return false;
    };
    let mut saw_charset = false;
    loop {
        let parameter = match parameters.next_parameter() {
            Ok(Some(parameter)) => parameter,
            Ok(None) => return true,
            Err(()) => return false,
        };
        if !parameter.name.eq_ignore_ascii_case("charset") {
            continue;
        }
        if saw_charset {
            return false;
        }
        saw_charset = true;
        if !logical_value_eq(parameter.value, parameter.quoted, b"utf-8") {
            return false;
        }
    }
}

/// One syntactically admitted Content-Type parameter.
struct ContentTypeParameter<'a> {
    name: &'a str,
    value: &'a str,
    quoted: bool,
}

/// A quote-aware scanner over one validated Content-Type parameter sequence.
struct ParameterScanner<'a> {
    remaining: Option<&'a str>,
}

impl<'a> ParameterScanner<'a> {
    /// Validates the media type and positions the scanner after its first
    /// structural semicolon, when present.
    fn new(content_type: &'a str) -> Result<Self, ()> {
        let (media_type, remaining) = match content_type.split_once(';') {
            Some((media_type, remaining)) => (media_type, Some(remaining)),
            None => (content_type, None),
        };
        if !is_media_type(trim_ows(media_type)) {
            return Err(());
        }
        Ok(Self { remaining })
    }

    /// Returns the next complete parameter after validating quoted boundaries
    /// and syntax.
    fn next_parameter(&mut self) -> Result<Option<ContentTypeParameter<'a>>, ()> {
        let Some(remaining) = self.remaining.take() else {
            return Ok(None);
        };
        let boundary = find_parameter_boundary(remaining)?;
        let (raw, next) = match boundary {
            Some(index) => (&remaining[..index], Some(&remaining[index + 1..])),
            None => (remaining, None),
        };
        self.remaining = next;
        parse_parameter(raw).map(Some).ok_or(())
    }
}

/// Finds the next unquoted semicolon while respecting HTTP quoted-pair
/// escapes. An unclosed quoted string fails the entire field.
fn find_parameter_boundary(value: &str) -> Result<Option<usize>, ()> {
    let bytes = value.as_bytes();
    let mut index = 0;
    let mut quoted = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if quoted => {
                index += 1;
                if index == bytes.len() || !is_quoted_pair_byte(bytes[index]) {
                    return Err(());
                }
            }
            b'"' => quoted = !quoted,
            b';' if !quoted => return Ok(Some(index)),
            _ => {}
        }
        index += 1;
    }
    if quoted { Err(()) } else { Ok(None) }
}

/// Validates one parameter and returns its exact token or unescaped quoted
/// value. Quoting remains explicit for logical-octet comparison.
fn parse_parameter(raw: &str) -> Option<ContentTypeParameter<'_>> {
    let parameter = trim_ows(raw);
    let (name, raw_value) = parameter.split_once('=')?;
    if !is_http_token(name) || raw_value.is_empty() {
        return None;
    }
    if raw_value.starts_with('"') {
        let value = parse_quoted_value(raw_value)?;
        Some(ContentTypeParameter {
            name,
            value,
            quoted: true,
        })
    } else if is_http_token(raw_value) {
        Some(ContentTypeParameter {
            name,
            value: raw_value,
            quoted: false,
        })
    } else {
        None
    }
}

/// Validates one complete HTTP quoted string and returns its raw inner bytes.
fn parse_quoted_value(value: &str) -> Option<&str> {
    let bytes = value.as_bytes();
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => return (index + 1 == bytes.len()).then_some(&value[1..index]),
            b'\\' => {
                index += 1;
                if index == bytes.len() || !is_quoted_pair_byte(bytes[index]) {
                    return None;
                }
            }
            byte if !is_quoted_text_byte(byte) => return None,
            _ => {}
        }
        index += 1;
    }
    None
}

/// Compares a token or quoted parameter's logical unescaped bytes to one ASCII
/// value without allocating an intermediate string.
fn logical_value_eq(value: &str, quoted: bool, expected: &[u8]) -> bool {
    let mut source = value.bytes();
    let mut expected = expected.iter().copied();
    loop {
        let actual = match source.next() {
            Some(b'\\') if quoted => source.next(),
            actual => actual,
        };
        match (actual, expected.next()) {
            (Some(actual), Some(expected)) if actual.eq_ignore_ascii_case(&expected) => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

/// Reports whether one trimmed field prefix is a syntactic HTTP media type.
fn is_media_type(value: &str) -> bool {
    value
        .split_once('/')
        .is_some_and(|(top, sub)| is_http_token(top) && is_http_token(sub))
}

/// Reports whether a nonempty string contains only HTTP token bytes.
fn is_http_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

/// Trims only HTTP optional whitespace from one structural boundary.
fn trim_ows(value: &str) -> &str {
    value.trim_matches([' ', '\t'])
}

/// Reports whether one byte is legal after a quoted-pair backslash.
fn is_quoted_pair_byte(byte: u8) -> bool {
    matches!(byte, b'\t' | b' ' | 0x21..=0x7e | 0x80..=0xff)
}

/// Reports whether one byte is legal unescaped quoted-string content.
fn is_quoted_text_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'\t' | b' ' | b'!' | 0x23..=0x5b | 0x5d..=0x7e | 0x80..=0xff
    )
}

#[cfg(test)]
#[path = "wire_request_tests.rs"]
mod tests;
