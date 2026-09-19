//! Owner tests for finite-aware Serde capture and exact JSON preservation.

use std::cell::Cell;
use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::json;

use super::RunRecordEventValue;
use super::finite::to_json;

#[derive(Serialize)]
/// One nested f32 producer used to prove recursive rejection.
struct NestedF32 {
    values: Vec<f32>,
}

#[derive(Serialize)]
/// One nested f64 producer used to prove recursive rejection.
struct NestedF64 {
    branch: Vec<Vec<f64>>,
}

#[test]
fn top_level_and_nested_non_finite_f32_and_f64_are_rejected() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(RunRecordEventValue::from_serializable(&value).is_err());
        assert!(
            RunRecordEventValue::from_serializable(&NestedF32 {
                values: vec![1.0, value],
            })
            .is_err()
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(RunRecordEventValue::from_serializable(&value).is_err());
        assert!(
            RunRecordEventValue::from_serializable(&NestedF64 {
                branch: vec![vec![1.0], vec![value]],
            })
            .is_err()
        );
    }
}

#[derive(Serialize)]
/// Representative externally tagged enum forms supported by serde_json.
enum Choice {
    Unit,
    New(u16),
    Tuple(i8, bool),
    Struct { label: String },
}

#[derive(Serialize)]
/// Finite values spanning the supported JSON-compatible Serde data model.
struct FiniteFixture {
    signed: i128,
    unsigned: u128,
    float32: f32,
    float64: f64,
    character: char,
    optional: Option<u8>,
    sequence: Vec<i16>,
    map: BTreeMap<i32, String>,
    choices: Vec<Choice>,
}

#[test]
fn finite_capture_matches_serde_json_for_every_representative_shape() {
    let fixture = FiniteFixture {
        signed: -9,
        unsigned: 12,
        float32: 1.25,
        float64: -4.5,
        character: 'x',
        optional: Some(3),
        sequence: vec![-2, 5],
        map: BTreeMap::from([(-7, "west".to_owned()), (8, "east".to_owned())]),
        choices: vec![
            Choice::Unit,
            Choice::New(2),
            Choice::Tuple(-1, true),
            Choice::Struct {
                label: "exact".to_owned(),
            },
        ],
    };
    assert_eq!(
        to_json(&fixture).unwrap(),
        serde_json::to_value(&fixture).unwrap()
    );
    assert_eq!(
        to_json(&fixture).unwrap()["choices"],
        json!(["Unit", { "New": 2 }, { "Tuple": [-1, true] }, { "Struct": { "label": "exact" } }])
    );
}

/// Serializer that would expose an unsafe second visit by yielding NaN.
struct OneShot<'a> {
    calls: &'a Cell<u8>,
}

impl Serialize for OneShot<'_> {
    /// Yield one finite value, then a non-finite value on any repeated visit.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let call = self.calls.get();
        self.calls.set(call + 1);
        serializer.serialize_f64(if call == 0 { 1.0 } else { f64::NAN })
    }
}

#[test]
fn application_serializer_is_visited_exactly_once_before_json_projection() {
    let calls = Cell::new(0);
    assert_eq!(
        to_json(&OneShot { calls: &calls }).unwrap(),
        serde_json::json!(1.0)
    );
    assert_eq!(calls.get(), 1);
}
