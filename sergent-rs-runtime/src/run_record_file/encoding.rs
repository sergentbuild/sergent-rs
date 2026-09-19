//! Canonical recursive ASCII JSON encoding for every Run Record file line.

use std::fmt::Write as _;

use serde_json::{Map, Value, json};
use sergent_rs_core::timing::Timestamp;

use super::{RunRecordCorrelation, RunRecordFileError};

pub(crate) const SCHEMA_VERSION: &str = "sergent.run_record.v1";
pub(crate) const SERGENT_ACTIVITY: &str = "sergent_activity";
pub(crate) const APP_ACTIVITY: &str = "app_activity";
pub(crate) const USER_ACTIVITY: &str = "user_activity";

/// Build one complete canonical envelope line before it reaches writer authority.
pub(crate) fn line(
    timestamp: Timestamp,
    activity: &'static str,
    event: &str,
    payload: Value,
    correlation: &RunRecordCorrelation,
) -> Result<Vec<u8>, RunRecordFileError> {
    let timestamp = serde_json::to_value(timestamp).map_err(|error| {
        RunRecordFileError::conversion(format!("Run Record timestamp conversion failed: {error}"))
    })?;
    let mut envelope = Map::new();
    envelope.insert("schema_version".to_owned(), json!(SCHEMA_VERSION));
    envelope.insert("timestamp_utc".to_owned(), timestamp);
    envelope.insert("activity".to_owned(), json!(activity));
    envelope.insert("event".to_owned(), json!(event));
    envelope.insert("payload".to_owned(), payload);
    if let Some(run_id) = correlation.run_id.as_ref() {
        envelope.insert("run_id".to_owned(), json!(run_id));
    }
    if let Some(scene_id) = correlation.scene_id.as_ref() {
        envelope.insert("scene_id".to_owned(), json!(scene_id));
    }
    if let Some(revision) = correlation.revision {
        envelope.insert("revision".to_owned(), json!(revision));
    }

    let mut output = String::new();
    write_value(&Value::Object(envelope), &mut output);
    output.push('\n');
    Ok(output.into_bytes())
}

/// Encode one JSON value recursively under the canonical compact rules.
fn write_value(value: &Value, output: &mut String) {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => output.push_str(&value.to_string()),
        Value::String(value) => write_string(value, output),
        Value::Array(values) => write_array(values, output),
        Value::Object(values) => write_object(values, output),
    }
}

/// Encode a JSON string while escaping every non-ASCII UTF-16 unit.
fn write_string(value: &str, output: &mut String) {
    let escaped = serde_json::to_string(value).expect("serializing a JSON string cannot fail");
    for character in escaped.chars() {
        if character.is_ascii() {
            output.push(character);
        } else {
            let mut units = [0_u16; 2];
            for unit in character.encode_utf16(&mut units) {
                write!(output, "\\u{unit:04x}").expect("writing to a String cannot fail");
            }
        }
    }
}

/// Encode array children in their application-owned order.
fn write_array(values: &[Value], output: &mut String) {
    output.push('[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        write_value(value, output);
    }
    output.push(']');
}

/// Encode object entries by ascending Unicode code point of unescaped key text.
fn write_object(values: &Map<String, Value>, output: &mut String) {
    let mut fields = values.iter().collect::<Vec<_>>();
    fields.sort_unstable_by_key(|(key, _)| *key);
    output.push('{');
    for (index, (key, value)) in fields.into_iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        write_string(key, output);
        output.push(':');
        write_value(value, output);
    }
    output.push('}');
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sergent_rs_core::timing::Timestamp;

    use super::{APP_ACTIVITY, line};
    use crate::run_record_file::RunRecordCorrelation;

    #[test]
    fn line_is_recursive_sorted_compact_ascii_json_with_one_lf() {
        let bytes = line(
            Timestamp::from_unix_micros(1_709_164_801_234_567),
            APP_ACTIVITY,
            "note.\u{4e16}\u{754c}",
            json!({ "z": { "b": "\u{1f600}", "a": 1 }, "a": ["cafe\u{301}"] }),
            &RunRecordCorrelation::new(
                Some("run_x".to_owned()),
                Some("scene_y".to_owned()),
                Some(7),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "{\"activity\":\"app_activity\",\"event\":\"note.\\u4e16\\u754c\",\"payload\":{\"a\":[\"cafe\\u0301\"],\"z\":{\"a\":1,\"b\":\"\\ud83d\\ude00\"}},\"revision\":7,\"run_id\":\"run_x\",\"scene_id\":\"scene_y\",\"schema_version\":\"sergent.run_record.v1\",\"timestamp_utc\":\"2024-02-29T00:00:01.234567Z\"}\n"
        );
    }
}
