//! Private local fakes for the runtime test suite. The runtime must not depend
//! on the provider crate, not even as a dev-dependency, so these
//! deterministic fakes are owned here and gated on tokio sync primitives, never
//! on sleeps.
//!
//! The fakes are grouped by the interface each implements; every public item is
//! re-exported here so consumers keep using `harness::*` unchanged. A given test
//! binary uses only part of this superset, so `dead_code` covers the unused
//! fakes and `unused_imports` covers the re-exports that binary does not name.
#![allow(dead_code, unused_imports)]

mod actions;
mod clients;
mod observers;
mod operations;
mod rebase;
mod recipes;
mod scene;

pub use actions::*;
pub use clients::*;
pub use observers::*;
pub use operations::*;
pub use rebase::*;
pub use recipes::*;
pub use scene::*;

/// Build one object-typed parsed-JSON fixture.
pub fn object(value: serde_json::Value) -> sergent_rs_core::model::ParsedJsonObject {
    serde_json::from_value(value).expect("test model output must be a JSON object")
}

/// Default per-run fixture for tests that do not exercise model identity or budgets.
pub fn run_settings() -> sergent_rs_runtime::sergent::RunSettings {
    run_settings_for("prov/model")
}

/// Per-run model settings preserving the exact model name used by a scenario.
pub fn run_settings_for(model_name: &str) -> sergent_rs_runtime::sergent::RunSettings {
    sergent_rs_runtime::sergent::RunSettings::new(model_name)
}

/// Prove that a failed or cancelled path did not invent an after revision.
pub fn assert_no_commit_revision(record: &sergent_rs_core::run_record::RunRecord) {
    assert_eq!(
        record.scene().revision_after(),
        None,
        "a no-commit result must leave revision_after null"
    );
}

/// Borrow the exact compiled Patch summary nested in Patch-step output.
pub fn patch_summary(
    step: &sergent_rs_core::run_record::RunStepRecord,
) -> &serde_json::Map<String, serde_json::Value> {
    step.output()
        .and_then(sergent_rs_core::run_record::CapturedValue::value)
        .and_then(serde_json::Value::as_object)
        .and_then(|output| output.get("compiled_patch"))
        .and_then(serde_json::Value::as_object)
        .expect("Patch output must contain a captured compiled_patch summary")
}

/// Read the ordered operation trace ids from a nested Patch summary.
pub fn patch_operation_trace_ids(step: &sergent_rs_core::run_record::RunStepRecord) -> Vec<&str> {
    patch_summary(step)["operation_trace_ids"]
        .as_array()
        .expect("operation_trace_ids must be an array")
        .iter()
        .map(|value| value.as_str().expect("an operation trace id must be text"))
        .collect()
}

/// Borrow the inert request projection retained by one model-call record.
pub fn captured_request(call: &sergent_rs_core::run_record::ModelCallRecord) -> &serde_json::Value {
    call.payloads()
        .request()
        .value()
        .expect("the request projection must capture")
}
