//! Construction invariants for closed authoritative run records.

use crate::vocab::{RunStepName, RunStepStatus, Stage};

use super::SceneTransition;
use super::{Cancellation, CancellationCheckpoint, RunOutcome, RunRecord, RunStepRecord};

/// Assert that step order, terminal status, cancellation, and Scene transition
/// form one truthful closed record.
pub(super) fn assert_run_record(
    steps: &[RunStepRecord],
    outcome: &RunOutcome,
    scene: &SceneTransition,
    cancellation: Option<&Cancellation>,
) {
    assert!(!steps.is_empty(), "a closed RunRecord has a reached step");
    assert_eq!(
        steps[0].name(),
        RunStepName::ProcessInput,
        "a closed RunRecord starts with ProcessInput"
    );
    assert!(
        steps
            .windows(2)
            .all(|pair| step_order(pair[1].name()) == step_order(pair[0].name()) + 1),
        "closed RunRecord steps must be ordered, unique, and contiguous"
    );
    assert!(
        steps[..steps.len() - 1]
            .iter()
            .all(|step| matches!(step.status(), RunStepStatus::Success)),
        "only the final reached step may be unsuccessful"
    );

    let final_step = steps.last().expect("non-empty checked above");
    let cancelled_before_intent = matches!(
        (
            outcome,
            final_step.name(),
            final_step.status(),
            cancellation
        ),
        (
            RunOutcome::Cancelled { .. },
            RunStepName::ProcessInput,
            RunStepStatus::Success,
            Some(Cancellation {
                checkpoint: Some(CancellationCheckpoint::BeforeIntent),
                ..
            })
        )
    );
    let terminal_status_matches = match (outcome, final_step.status()) {
        (RunOutcome::Success { .. }, RunStepStatus::Success)
        | (RunOutcome::Failure { .. }, RunStepStatus::Failure { .. })
        | (RunOutcome::Cancelled { .. }, RunStepStatus::Cancelled { .. }) => true,
        _ => cancelled_before_intent,
    };
    assert!(
        terminal_status_matches,
        "final step status must match outcome"
    );

    match (outcome, final_step.name(), scene.revision_after) {
        (RunOutcome::Failure { .. } | RunOutcome::Cancelled { .. }, _, None) => {}
        (RunOutcome::Success { .. }, RunStepName::Intent, Some(after))
            if after == scene.revision_before => {}
        (RunOutcome::Success { .. }, RunStepName::Commit, Some(after))
            if after >= scene.revision_before => {}
        _ => panic!("Scene transition must match the terminal run path"),
    }
}

/// Assert that reported terminal stage agrees with outcome and final step.
pub(super) fn assert_result_stage(record: &RunRecord, stage: Stage) {
    let final_name = record
        .steps
        .last()
        .expect("RunRecord construction requires a reached step")
        .name();
    let coherent = match (&record.outcome, final_name) {
        (RunOutcome::Success { .. }, RunStepName::Intent) => stage == Stage::Intent,
        (RunOutcome::Success { .. }, RunStepName::Commit) => stage == Stage::Commit,
        (RunOutcome::Failure { .. }, RunStepName::ProcessInput) => stage == Stage::Started,
        (RunOutcome::Failure { .. }, RunStepName::Intent) => {
            matches!(stage, Stage::IntentCall | Stage::Intent)
        }
        (RunOutcome::Failure { .. }, RunStepName::ExecutionPlan) => {
            matches!(stage, Stage::PlanCall | Stage::ExecutionPlan)
        }
        (RunOutcome::Failure { .. }, RunStepName::Patch) => {
            matches!(stage, Stage::Patch | Stage::DryRun)
        }
        (RunOutcome::Failure { .. }, RunStepName::Commit) => stage == Stage::Commit,
        (RunOutcome::Cancelled { .. }, RunStepName::ProcessInput) => stage == Stage::Started,
        (RunOutcome::Cancelled { .. }, RunStepName::Intent) => {
            matches!(stage, Stage::IntentCall | Stage::Intent)
        }
        (RunOutcome::Cancelled { .. }, RunStepName::ExecutionPlan) => stage == Stage::PlanCall,
        (RunOutcome::Cancelled { .. }, RunStepName::Patch) => stage == Stage::DryRun,
        (RunOutcome::Cancelled { .. }, RunStepName::Commit) => stage == Stage::Commit,
        _ => false,
    };
    assert!(coherent, "terminal stage must match the final reached step");
}

/// Map a Step Record name to its ordinal in the closed pipeline sequence.
fn step_order(name: RunStepName) -> u8 {
    match name {
        RunStepName::ProcessInput => 0,
        RunStepName::Intent => 1,
        RunStepName::ExecutionPlan => 2,
        RunStepName::Patch => 3,
        RunStepName::Commit => 4,
    }
}
