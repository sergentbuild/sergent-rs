//! Model-call error projection at the model-output crossing.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md

use sergent_rs_core::error::RunError;
use sergent_rs_core::model::ModelError;

/// Preserve provider kind and prose while projecting retryability into the
/// enclosing transport error's exact one-key metadata.
pub(crate) fn error_to_run_error(error: &ModelError) -> RunError {
    let mut run_error = RunError::new(error.kind.clone(), error.message.clone());
    run_error.metadata = serde_json::Map::from_iter([(
        "retryable".to_owned(),
        serde_json::Value::Bool(error.retryable),
    )]);
    run_error
}

#[cfg(test)]
mod tests {
    use sergent_rs_core::model::ModelError;

    use super::error_to_run_error;

    #[test]
    fn model_error_preserves_open_kind_and_exact_retryability_metadata() {
        let error = ModelError {
            kind: "provider_specific_overload".to_owned(),
            retryable: true,
            message: "capacity unavailable".to_owned(),
            raw_output: None,
            identity: None,
            attempts: Vec::new(),
            usage: None,
        };

        let run_error = error_to_run_error(&error);

        assert_eq!(run_error.kind, "provider_specific_overload");
        assert_eq!(run_error.message, "capacity unavailable");
        assert_eq!(
            run_error.metadata,
            serde_json::json!({ "retryable": true })
                .as_object()
                .unwrap()
                .clone()
        );
    }
}
