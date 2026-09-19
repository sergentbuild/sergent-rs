//! The closed run vocabulary must serialize to its exact strings. The serde
//! derives own those strings, so every variant is pinned against serde output.

use sergent_rs_core::error::{ErrorKind, RunError};
use sergent_rs_core::vocab::{ProgressStatus, RunStepName, RunStepStatus, Stage, TerminalStatus};

fn serialized<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn stage_strings_are_exact() {
    let cases = [
        (Stage::Queued, "queued"),
        (Stage::Started, "started"),
        (Stage::IntentCall, "intent_call"),
        (Stage::Intent, "intent"),
        (Stage::PlanCall, "plan_call"),
        (Stage::ExecutionPlan, "execution_plan"),
        (Stage::Patch, "patch"),
        (Stage::DryRun, "dry_run"),
        (Stage::Commit, "commit"),
    ];
    for (stage, expected) in cases {
        assert_eq!(serialized(&stage), expected);
    }
}

#[test]
fn progress_status_strings_are_exact() {
    let cases = [
        (ProgressStatus::Queued, "queued"),
        (ProgressStatus::Running, "running"),
        (ProgressStatus::Success, "success"),
        (ProgressStatus::Failure, "failure"),
        (ProgressStatus::Cancelled, "cancelled"),
    ];
    for (status, expected) in cases {
        assert_eq!(serialized(&status), expected);
    }
}

#[test]
fn terminal_status_strings_are_exact() {
    for (status, expected) in [
        (TerminalStatus::Success, "success"),
        (TerminalStatus::Failure, "failure"),
        (TerminalStatus::Cancelled, "cancelled"),
    ] {
        assert_eq!(serialized(&status), expected);
    }
}

#[test]
fn run_step_status_strings_are_exact() {
    for (status, expected) in [
        (RunStepStatus::Success, "success"),
        (
            RunStepStatus::Failure {
                error: RunError::of(ErrorKind::ValidationError, "failed"),
            },
            "failure",
        ),
        (
            RunStepStatus::Cancelled {
                error: RunError::of(ErrorKind::Cancelled, "cancelled"),
            },
            "cancelled",
        ),
    ] {
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["status"], expected);
    }
}

#[test]
fn run_step_names_are_exact() {
    let cases = [
        (RunStepName::ProcessInput, "process_input"),
        (RunStepName::Intent, "intent"),
        (RunStepName::ExecutionPlan, "execution_plan"),
        (RunStepName::Patch, "patch"),
        (RunStepName::Commit, "commit"),
    ];
    for (name, expected) in cases {
        assert_eq!(serialized(&name), expected);
    }
}
