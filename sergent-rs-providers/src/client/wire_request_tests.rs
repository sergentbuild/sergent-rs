//! Owner tests for exact Content-Type charset admission.

use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

use super::uses_supported_charset;

/// One text or deliberately non-text Content-Type field set.
enum ContentTypeInput {
    Text(&'static [&'static str]),
    NonText(&'static [u8]),
}

/// The complete charset syntax and logical-value matrix.
const CHARSET_CASES: &[(&str, ContentTypeInput, bool)] = &[
    ("no content type", ContentTypeInput::Text(&[]), true),
    (
        "ordinary json",
        ContentTypeInput::Text(&["application/json"]),
        true,
    ),
    (
        "case variants",
        ContentTypeInput::Text(&["Application/JSON; CHARSET=UTF-8"]),
        true,
    ),
    (
        "ows around separators",
        ContentTypeInput::Text(&["application/json ; profile=x ; charset=utf-8 ; version=1"]),
        true,
    ),
    (
        "quoted utf8",
        ContentTypeInput::Text(&["application/json;charset=\"UtF-8\""]),
        true,
    ),
    (
        "escaped charset punctuation",
        ContentTypeInput::Text(&["application/json;charset=\"utf\\-8\""]),
        true,
    ),
    (
        "escaped charset case variant",
        ContentTypeInput::Text(&["application/json;charset=\"u\\TF-8\""]),
        true,
    ),
    (
        "quoted non-charset semicolon",
        ContentTypeInput::Text(&["application/json;profile=\"urn:x;charset=iso-8859-1\""]),
        true,
    ),
    (
        "quoted pair and semicolon",
        ContentTypeInput::Text(&["application/json;profile=\"urn:x\\\";charset=iso-8859-1\""]),
        true,
    ),
    (
        "space before equals",
        ContentTypeInput::Text(&["application/json;charset =utf-8"]),
        false,
    ),
    (
        "space after equals",
        ContentTypeInput::Text(&["application/json;charset= utf-8"]),
        false,
    ),
    (
        "duplicate identical",
        ContentTypeInput::Text(&["application/json;charset=utf-8;charset=\"UTF-8\""]),
        false,
    ),
    (
        "duplicate conflicting",
        ContentTypeInput::Text(&["application/json;charset=utf-8;charset=iso-8859-1"]),
        false,
    ),
    (
        "unbalanced opening quote",
        ContentTypeInput::Text(&["application/json;charset=\"utf-8"]),
        false,
    ),
    (
        "unbalanced closing quote",
        ContentTypeInput::Text(&["application/json;charset=utf-8\""]),
        false,
    ),
    (
        "unbalanced non-charset quote",
        ContentTypeInput::Text(&["application/json;profile=\"unterminated;charset=utf-8"]),
        false,
    ),
    (
        "junk after closing quote",
        ContentTypeInput::Text(&["application/json;profile=\"closed\"junk;charset=utf-8"]),
        false,
    ),
    (
        "unsupported charset",
        ContentTypeInput::Text(&["application/json;charset=iso-8859-1"]),
        false,
    ),
    (
        "escaped different octet",
        ContentTypeInput::Text(&["application/json;charset=\"utf\\_8\""]),
        false,
    ),
    (
        "escaped backslash remains logical",
        ContentTypeInput::Text(&["application/json;charset=\"utf\\\\-8\""]),
        false,
    ),
    (
        "multiple content type fields",
        ContentTypeInput::Text(&["application/json", "application/json;charset=utf-8"]),
        false,
    ),
    (
        "non-text content type",
        ContentTypeInput::NonText(b"application/json;profile=\x80"),
        false,
    ),
];

#[test]
fn charset_admission_is_exact_and_fail_closed() {
    for (name, input, expected) in CHARSET_CASES {
        let headers = content_type_headers(input);
        assert_eq!(uses_supported_charset(&headers), *expected, "{name}");
    }
}

/// Builds exact test headers, including one accepted non-text header value.
fn content_type_headers(input: &ContentTypeInput) -> HeaderMap {
    let mut headers = HeaderMap::new();
    match input {
        ContentTypeInput::Text(values) => {
            for value in *values {
                headers.append(
                    CONTENT_TYPE,
                    HeaderValue::from_str(value).expect("test content type is a header value"),
                );
            }
        }
        ContentTypeInput::NonText(value) => {
            headers.append(
                CONTENT_TYPE,
                HeaderValue::from_bytes(value).expect("test bytes are a header value"),
            );
        }
    }
    headers
}
