//! The common model-call operation shared by the two proposal phases.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

use sergent_rs_core::error::RunError;
use sergent_rs_core::model::{ModelClient, ModelRequest, ParsedJsonObject};
use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::run_record::{CompletedModelCall, ModelCallRecord, OpenModelCall};
use sergent_rs_core::scene::SceneActions;
use sergent_rs_core::timing::Timestamp;

use crate::cancel::CancelToken;
use crate::model_call::error_to_run_error;

use super::Sergent;

/// The narrow result of one raced provider invocation.
pub(super) enum ModelInvocation {
    Cancelled {
        requested_at: Timestamp,
        call: Box<ModelCallRecord>,
    },
    ProviderFailure {
        call: Box<ModelCallRecord>,
        error: RunError,
    },
    Completed {
        call: Box<CompletedModelCall>,
        parsed_json: ParsedJsonObject,
    },
}

impl<R, A, M> Sergent<R, A, M>
where
    R: SergentRecipe,
    A: SceneActions<Scene = R::Scene, Intent = R::Intent, Target = R::Target>,
    M: ModelClient,
{
    /// Race one complete runtime-constructed provider request against cancellation.
    pub(super) async fn invoke_model(
        &self,
        request: &ModelRequest,
        cancel: &CancelToken,
    ) -> ModelInvocation {
        let call = OpenModelCall::new(request);
        let invoke = self.model_client.invoke(request);
        tokio::pin!(invoke);
        tokio::select! {
            biased;
            requested_at = cancel.cancelled() => ModelInvocation::Cancelled {
                requested_at,
                call: Box::new(call.interrupted()),
            },
            result = &mut invoke => match result {
                Ok((response, parsed_json)) => ModelInvocation::Completed {
                    call: Box::new(call.completed(&response, &parsed_json)),
                    parsed_json,
                },
                Err(model_error) => ModelInvocation::ProviderFailure {
                    call: Box::new(call.failed(&model_error)),
                    error: error_to_run_error(&model_error),
                },
            },
        }
    }
}
