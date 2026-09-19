//! Exact Run Record, outcome, terminal, transition, and result contracts.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use sergent_rs_core::error::{ErrorKind, RunError};
use sergent_rs_core::ids::{RunId, SceneId};
use sergent_rs_core::model::{
    CallUsage, Message, ModelIdentity, ModelRequestInput, ModelResponse, ModelSettings, TokenCounts,
};
use sergent_rs_core::proposal::derive_proposal_schema;
use sergent_rs_core::run_record::{
    Cancellation, CancellationCheckpoint, OpenModelCall, OutputTokenTotal, RunOutcome, RunRecord,
    RunRecordCompletion, RunRecordHeader, RunStepEvidence, RunStepRecord, RunTerminal,
    SceneTransition, SergentResult,
};
use sergent_rs_core::timing::{TimeSpan, Timestamp};
use sergent_rs_core::vocab::{RunStepName, RunStepStatus, Stage, TerminalStatus};

#[derive(Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct CallProposal {
    answer: String,
}

fn scene_id() -> SceneId {
    SceneId::parse("doc_11111111111111111111111111111111").unwrap()
}

fn span() -> TimeSpan {
    TimeSpan::closed(
        Timestamp::from_unix_micros(0),
        Timestamp::from_unix_micros(1_000),
        1,
    )
}

fn step(
    name: RunStepName,
    status: RunStepStatus,
    input: Option<sergent_rs_core::run_record::CapturedValue>,
    output: Option<sergent_rs_core::run_record::CapturedValue>,
) -> RunStepRecord {
    RunStepRecord::new(
        name,
        status,
        span(),
        RunStepEvidence::new(input, output, None),
    )
}

fn success(name: RunStepName) -> RunStepRecord {
    step(name, RunStepStatus::Success, None, None)
}

fn record(
    outcome: RunOutcome,
    transition: SceneTransition,
    steps: Vec<RunStepRecord>,
    cancellation: Option<Cancellation>,
) -> RunRecord {
    RunRecord::new(
        RunRecordHeader::new(
            RunId::parse("run_00000000000000000000000000000000").unwrap(),
            "openai/gpt".to_owned(),
        ),
        span(),
        transition,
        steps,
        RunRecordCompletion::new(outcome, cancellation),
    )
}

fn call_with_output(output: Option<u64>) -> sergent_rs_core::run_record::ModelCallRecord {
    let request = ModelRequestInput::new(
        "openai/gpt".to_owned(),
        ModelSettings::default(),
        Arc::new(derive_proposal_schema::<CallProposal>().unwrap()),
    )
    .into_request(vec![Message::user("hello")]);
    let parsed = Map::from_iter([("answer".to_owned(), json!("ok"))]);
    let response = ModelResponse {
        identity: ModelIdentity {
            provider: "openai".to_owned(),
            model: "gpt".to_owned(),
            sdk_package: None,
            sdk_version: None,
        },
        raw_output: r#"{"answer":"ok"}"#.to_owned(),
        attempts: Vec::new(),
        usage: CallUsage {
            latency_ms: 12,
            tokens: Some(TokenCounts {
                input: Some(3),
                output,
            }),
            request_id: None,
        },
    };
    OpenModelCall::new(&request)
        .completed(&response, &parsed)
        .accepted(&CallProposal {
            answer: "ok".to_owned(),
        })
}

#[test]
fn scene_transition_constructors_serialize_exact_outcome_semantics() {
    assert_eq!(
        serde_json::to_value(SceneTransition::uncommitted(scene_id(), 3)).unwrap(),
        json!({
            "scene_id": scene_id(),
            "revision_before": 3,
            "revision_after": null
        })
    );
    assert_eq!(
        serde_json::to_value(SceneTransition::unchanged(scene_id(), 3)).unwrap(),
        json!({
            "scene_id": scene_id(),
            "revision_before": 3,
            "revision_after": 3
        })
    );
    assert_eq!(
        SceneTransition::committed(scene_id(), 3, 4).revision_after(),
        Some(4)
    );
}

#[test]
fn step_record_always_serializes_all_seven_fields() {
    let input =
        sergent_rs_core::run_record::CapturedValue::capture(&json!({ "observation": "hot" }));
    let output = sergent_rs_core::run_record::CapturedValue::capture(&json!({
        "selected_target": null
    }));
    let step = step(
        RunStepName::ProcessInput,
        RunStepStatus::Failure {
            error: RunError::new("no_target", "none"),
        },
        Some(input.clone()),
        Some(output.clone()),
    );

    assert_eq!(
        serde_json::to_value(step).unwrap(),
        json!({
            "name": "process_input",
            "status": "failure",
            "timing": serde_json::to_value(span()).unwrap(),
            "input": input,
            "output": output,
            "error": { "kind": "no_target", "message": "none", "metadata": {} },
            "model_call": null
        })
    );
}

#[test]
fn outcome_and_terminal_serialize_exact_null_bearing_records() {
    let terminal = RunTerminal::capture(
        Some("done".to_owned()),
        Map::from_iter([("decision".to_owned(), json!("accepted"))]),
    );
    assert_eq!(
        serde_json::to_value(RunOutcome::Success { terminal }).unwrap(),
        json!({
            "status": "success",
            "error": null,
            "terminal": {
                "message": {
                    "value": "done",
                    "value_type": std::any::type_name::<Option<String>>(),
                    "error": null,
                    "status": "captured"
                },
                "metadata": {
                    "value": { "decision": "accepted" },
                    "value_type": std::any::type_name::<Map<String, Value>>(),
                    "error": null,
                    "status": "captured"
                }
            }
        })
    );
    assert_eq!(
        serde_json::to_value(RunOutcome::Failure {
            error: RunError::of(ErrorKind::ValidationError, "bad"),
        })
        .unwrap(),
        json!({
            "status": "failure",
            "error": { "kind": "validation_error", "message": "bad", "metadata": {} },
            "terminal": null
        })
    );
}

#[test]
fn partial_terminal_facts_keep_both_capture_envelopes() {
    for (message, metadata) in [
        (Some("done".to_owned()), Map::new()),
        (
            None,
            Map::from_iter([("decision".to_owned(), json!("accepted"))]),
        ),
    ] {
        let terminal = RunTerminal::capture(message.clone(), metadata.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&terminal).unwrap(),
            json!({
                "message": {
                    "value": message,
                    "value_type": std::any::type_name::<Option<String>>(),
                    "error": null,
                    "status": "captured"
                },
                "metadata": {
                    "value": metadata,
                    "value_type": std::any::type_name::<Map<String, Value>>(),
                    "error": null,
                    "status": "captured"
                }
            })
        );
        let result = SergentResult::new(
            Stage::Intent,
            "scene",
            record(
                RunOutcome::Success {
                    terminal: Some(terminal),
                },
                SceneTransition::unchanged(scene_id(), 3),
                vec![
                    success(RunStepName::ProcessInput),
                    success(RunStepName::Intent),
                ],
                None,
            ),
            Vec::new(),
        );
        assert_eq!(result.terminal_message(), message.as_deref());
        assert_eq!(result.terminal_metadata(), &metadata);
    }
    assert!(RunTerminal::capture(None, Map::new()).is_none());
}

#[test]
fn run_record_has_only_the_exact_seven_top_level_fields() {
    let record = record(
        RunOutcome::Success { terminal: None },
        SceneTransition::unchanged(scene_id(), 3),
        vec![
            success(RunStepName::ProcessInput),
            success(RunStepName::Intent),
        ],
        None,
    );
    let value = serde_json::to_value(record).unwrap();
    let keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();

    assert_eq!(
        keys,
        [
            "cancellation",
            "model_name",
            "outcome",
            "run_id",
            "scene",
            "steps",
            "timing"
        ]
    );
    assert_eq!(value["cancellation"], Value::Null);
    assert!(value.get("command").is_none());
    assert!(value.get("source_kind").is_none());
    assert!(value["steps"][0].get("operation_trace").is_none());
}

#[test]
fn result_projects_success_or_failure_terminal_facts_and_identity() {
    let success = SergentResult::new(
        Stage::Intent,
        "scene",
        record(
            RunOutcome::Success {
                terminal: RunTerminal::capture(
                    Some("done".to_owned()),
                    Map::from_iter([("answer".to_owned(), json!(42))]),
                ),
            },
            SceneTransition::unchanged(scene_id(), 3),
            vec![
                success(RunStepName::ProcessInput),
                success(RunStepName::Intent),
            ],
            None,
        ),
        Vec::new(),
    );
    assert_eq!(success.terminal_message(), Some("done"));
    assert_eq!(
        success.terminal_metadata(),
        &Map::from_iter([("answer".to_owned(), json!(42))])
    );
    assert_eq!(success.identity().revision, 3);

    let error = RunError::new("application", "failed").with("fact", true);
    let failure = SergentResult::new(
        Stage::Started,
        "scene",
        record(
            RunOutcome::Failure {
                error: error.clone(),
            },
            SceneTransition::uncommitted(scene_id(), 3),
            vec![step(
                RunStepName::ProcessInput,
                RunStepStatus::Failure {
                    error: error.clone(),
                },
                None,
                None,
            )],
            None,
        ),
        Vec::new(),
    );
    assert_eq!(failure.status(), TerminalStatus::Failure);
    assert_eq!(failure.terminal_message(), Some("failed"));
    assert_eq!(failure.terminal_metadata(), &error.metadata);
    assert_eq!(failure.identity().revision, 3);
}

#[test]
fn total_output_tokens_distinguishes_complete_incomplete_and_overflow() {
    let total = |intent, plan| {
        let call_step = |name, output| {
            RunStepRecord::new(
                name,
                RunStepStatus::Success,
                span(),
                RunStepEvidence::new(None, None, Some(call_with_output(output))),
            )
        };
        record(
            RunOutcome::Success { terminal: None },
            SceneTransition::committed(scene_id(), 3, 4),
            vec![
                success(RunStepName::ProcessInput),
                call_step(RunStepName::Intent, intent),
                call_step(RunStepName::ExecutionPlan, plan),
                success(RunStepName::Patch),
                success(RunStepName::Commit),
            ],
            None,
        )
        .total_output_tokens()
    };

    assert_eq!(total(Some(10), Some(5)), OutputTokenTotal::Complete(15));
    assert_eq!(total(None, Some(5)), OutputTokenTotal::Incomplete);
    assert_eq!(total(Some(u64::MAX), Some(1)), OutputTokenTotal::Overflow);
}

#[test]
fn cancellation_before_intent_keeps_process_input_successful() {
    let cancellation = Cancellation::new(
        Timestamp::from_unix_micros(1_000),
        Some(CancellationCheckpoint::BeforeIntent),
    );
    let record = record(
        RunOutcome::Cancelled {
            error: RunError::of(ErrorKind::Cancelled, "cancelled"),
        },
        SceneTransition::uncommitted(scene_id(), 3),
        vec![success(RunStepName::ProcessInput)],
        Some(cancellation),
    );

    assert_eq!(record.steps()[0].status(), &RunStepStatus::Success);
    assert_eq!(record.scene().revision_after(), None);
}

#[test]
#[should_panic(expected = "Scene transition must match the terminal run path")]
fn failure_cannot_claim_a_committed_revision() {
    let error = RunError::of(ErrorKind::ValidationError, "bad");
    let _ = record(
        RunOutcome::Failure {
            error: error.clone(),
        },
        SceneTransition::committed(scene_id(), 3, 4),
        vec![step(
            RunStepName::ProcessInput,
            RunStepStatus::Failure { error },
            None,
            None,
        )],
        None,
    );
}
