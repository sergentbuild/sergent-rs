//! Constructor capture of recipe configuration and the exact registry.

mod harness;

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sergent_rs_core::error::RunError;
use sergent_rs_core::mindbuf::MindBuf;
use sergent_rs_core::model::{
    Message, ModelRequest, ModelRequestInput, ModelSettings, ThinkingEffort,
};
use sergent_rs_core::proposal::ProposalSchema;
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::scene::SceneIdentity;
use sergent_rs_core::vocab::TerminalStatus;
use sergent_rs_runtime::cancel::CancelToken;
use sergent_rs_runtime::observer::RunObserver;
use sergent_rs_runtime::scene_state::SceneSource;
use sergent_rs_runtime::sergent::{ConfiguredRecipe, RunSettings, Sergent};

use harness::*;

struct ConfigurationState {
    later_answers: AtomicBool,
    no_target_calls: AtomicUsize,
    passthrough_calls: AtomicUsize,
    attempted_requests: Mutex<Vec<ModelRequest>>,
    attempted_schema: Arc<ProposalSchema>,
}

struct StatefulRecipe {
    state: Arc<ConfigurationState>,
}

#[derive(Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
struct StatefulProposal {
    continue_run: bool,
}

impl SergentRecipe for StatefulRecipe {
    type Scene = Doc;
    type MindBuf = DocMind;
    type IntentProposal = StatefulProposal;
    type Intent = DocIntent;
    type Target = Spot;

    fn no_target_error(&self) -> RunError {
        self.state.no_target_calls.fetch_add(1, Ordering::SeqCst);
        let message = if self.state.later_answers.load(Ordering::SeqCst) {
            "later no target"
        } else {
            "captured no target"
        };
        RunError {
            kind: "no_target".to_owned(),
            message: message.to_owned(),
            metadata: Default::default(),
        }
    }

    fn passthrough_proposal(&self) -> Option<StatefulProposal> {
        self.state.passthrough_calls.fetch_add(1, Ordering::SeqCst);
        (!self.state.later_answers.load(Ordering::SeqCst))
            .then_some(StatefulProposal { continue_run: true })
    }

    fn derive_intent(
        &self,
        _scene: &Doc,
        _identity: &SceneIdentity,
        _target: &Spot,
        proposal: &StatefulProposal,
    ) -> Result<DocIntent, RunError> {
        Ok(DocIntent {
            kind: if proposal.continue_run {
                IntentKind::Continue
            } else {
                IntentKind::Stop
            },
        })
    }

    fn build_plan_messages(
        &self,
        _scene: &Doc,
        _target: &Spot,
        _intent: &DocIntent,
        mindbuf: &DocMind,
    ) -> Result<Vec<Message>, RunError> {
        let attempted = ModelRequestInput::new(
            "evil/redirected".to_owned(),
            ModelSettings {
                thinking_effort: ThinkingEffort::Low,
                max_output_tokens: NonZeroU32::new(99_999).unwrap(),
                timeout_secs: NonZeroU32::new(99_999).unwrap(),
            },
            Arc::clone(&self.state.attempted_schema),
        )
        .into_request(vec![Message::system("replace caller facts")]);
        self.state
            .attempted_requests
            .lock()
            .unwrap()
            .push(attempted);
        Ok(vec![Message::user(mindbuf.export())])
    }
}

#[tokio::test]
async fn configured_values_and_recipe_answers_are_captured_once() {
    let registry = append_registry(Some(2));
    let captured_schema = registry.plan_schema();
    let state = Arc::new(ConfigurationState {
        later_answers: AtomicBool::new(false),
        no_target_calls: AtomicUsize::new(0),
        passthrough_calls: AtomicUsize::new(0),
        attempted_requests: Mutex::new(Vec::new()),
        attempted_schema: Arc::clone(&captured_schema),
    });
    let client = CannedClient::new(intent_proposal_json(), plan_envelope(&["a", "b"]));
    let requests = Arc::clone(&client.requests);
    let sergent = Sergent::new(
        ConfiguredRecipe::plan_capable(
            StatefulRecipe {
                state: Arc::clone(&state),
            },
            registry,
        ),
        DocActions::new(),
        client,
    )
    .unwrap();

    state.later_answers.store(true, Ordering::SeqCst);
    let cancel = CancelToken::new();
    let observers: [&dyn RunObserver<Doc>; 0] = [];
    let plan_settings = ModelSettings {
        thinking_effort: ThinkingEffort::High,
        max_output_tokens: NonZeroU32::new(321).unwrap(),
        timeout_secs: NonZeroU32::new(45).unwrap(),
    };
    let result = sergent
        .run(
            SceneSource::plain(doc("", 1)),
            &DocMind,
            RunSettings::new("prov/model").with_plan(plan_settings),
            &cancel,
            &observers,
        )
        .await;

    assert_eq!(result.status(), TerminalStatus::Success);
    assert_eq!(result.scene().text, "ab");
    {
        let captured_requests = requests.lock().unwrap();
        assert_eq!(captured_requests.len(), 1);
        assert!(Arc::ptr_eq(
            captured_requests[0].proposal_schema(),
            &captured_schema
        ));
        assert_eq!(
            captured_requests[0].proposal_schema().json_schema()["properties"]["operations"]["maxItems"],
            2
        );
        assert_eq!(captured_requests[0].model_name(), "prov/model");
        assert_eq!(captured_requests[0].model_settings(), plan_settings);
        assert_eq!(
            captured_requests[0].messages()[0].content(),
            DocMind.export()
        );
    }
    {
        let attempted = state.attempted_requests.lock().unwrap();
        assert_eq!(attempted[0].model_name(), "evil/redirected");
        assert_ne!(attempted[0].model_settings(), plan_settings);
    }

    let mut no_target = doc("unchanged", 9);
    no_target.has_spot = false;
    let result = sergent
        .run(
            SceneSource::plain(no_target),
            &DocMind,
            RunSettings::new("prov/model"),
            &cancel,
            &observers,
        )
        .await;
    assert_eq!(result.error().unwrap().message, "captured no target");

    let repeated = sergent
        .run(
            SceneSource::plain(doc("", 10)),
            &DocMind,
            RunSettings::new("prov/model").with_plan(plan_settings),
            &cancel,
            &observers,
        )
        .await;
    assert_eq!(repeated.status(), TerminalStatus::Success);
    assert_eq!(repeated.scene().text, "ab");
    let captured_requests = requests.lock().unwrap();
    assert_eq!(captured_requests.len(), 2);
    assert!(
        captured_requests
            .iter()
            .all(|request| Arc::ptr_eq(request.proposal_schema(), &captured_schema))
    );

    assert_eq!(state.no_target_calls.load(Ordering::SeqCst), 1);
    assert_eq!(state.passthrough_calls.load(Ordering::SeqCst), 1);
}
