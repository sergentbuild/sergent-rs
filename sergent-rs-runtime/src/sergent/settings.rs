//! The per-run model selection and phase settings supplied by the caller.
//! @sergent-rs-runtime/docs/KNOWLEDGE.md
//!
//! `RunSettings` has no complete default: it binds the exact opaque model name
//! the caller chose, and each reached model phase reads only its own settings.
//! Recipe objects store neither model selection nor request budgets.

use sergent_rs_core::model::ModelSettings;

/// The exact caller-provided model name and settings for reached model phases.
/// @sergent/docs/execution-model.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSettings {
    model_name: String,
    intent: ModelSettings,
    plan: ModelSettings,
}

impl RunSettings {
    /// Bind the exact caller-provided model name to default phase settings.
    pub fn new(model_name: impl Into<String>) -> Self {
        Self {
            model_name: model_name.into(),
            intent: ModelSettings::default(),
            plan: ModelSettings::default(),
        }
    }

    /// Replace the settings used if the run reaches a model-backed Intent call.
    pub fn with_intent(mut self, settings: ModelSettings) -> Self {
        self.intent = settings;
        self
    }

    /// Replace the settings used if the run reaches a Plan call.
    pub fn with_plan(mut self, settings: ModelSettings) -> Self {
        self.plan = settings;
        self
    }

    /// Supplies the opaque caller-provided model name unchanged.
    pub(super) fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Supplies per-run settings when the model-backed Intent phase is reached.
    pub(super) fn intent(&self) -> ModelSettings {
        self.intent
    }

    /// Supplies per-run settings when the Plan proposal phase is reached.
    pub(super) fn plan(&self) -> ModelSettings {
        self.plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_settings_preserve_opaque_model_name_and_default_phase_settings() {
        let settings = RunSettings::new(" Provider/Mixed Model ");

        assert_eq!(settings.model_name(), " Provider/Mixed Model ");
        assert_eq!(settings.intent(), ModelSettings::default());
        assert_eq!(settings.plan(), ModelSettings::default());
    }
}
