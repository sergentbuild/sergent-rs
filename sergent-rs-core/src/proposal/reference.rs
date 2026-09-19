//! JSON Pointer token encoding for canonical local definition references.

const LOCAL_DEFINITION_PREFIX: &str = "#/$defs/";

/// Compose one local definition reference from a raw `$defs` key.
pub(crate) fn definition_reference(name: &str) -> String {
    format!("{LOCAL_DEFINITION_PREFIX}{}", encode_pointer_token(name))
}

/// Resolve one local definition reference to its raw `$defs` key.
pub(crate) fn referenced_definition(reference: &str) -> Option<String> {
    let token = reference.strip_prefix(LOCAL_DEFINITION_PREFIX)?;
    decode_pointer_token(token)
}

/// Append one raw JSON Pointer token to an existing pointer.
pub(crate) fn append_pointer_token(pointer: &str, token: &str) -> String {
    format!("{pointer}/{}", encode_pointer_token(token))
}

/// Encode one raw JSON Pointer token according to RFC 6901.
fn encode_pointer_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Decode one JSON Pointer token, rejecting malformed or multiple tokens.
fn decode_pointer_token(token: &str) -> Option<String> {
    let mut decoded = String::with_capacity(token.len());
    let mut characters = token.chars();
    while let Some(character) = characters.next() {
        match character {
            '/' => return None,
            '~' => match characters.next() {
                Some('0') => decoded.push('~'),
                Some('1') => decoded.push('/'),
                _ => return None,
            },
            other => decoded.push(other),
        }
    }
    Some(decoded)
}

#[cfg(test)]
mod tests {
    use super::{definition_reference, referenced_definition};

    #[test]
    fn local_definition_reference_round_trips_pointer_tokens() {
        let reference = definition_reference("Root/Child~Leaf");
        assert_eq!(reference, "#/$defs/Root~1Child~0Leaf");
        assert_eq!(
            referenced_definition(&reference).as_deref(),
            Some("Root/Child~Leaf")
        );
    }

    #[test]
    fn malformed_pointer_tokens_do_not_resolve() {
        assert!(referenced_definition("#/$defs/A/B").is_none());
        assert!(referenced_definition("#/$defs/A~2B").is_none());
        assert!(referenced_definition("https://example.test/schema").is_none());
    }
}
