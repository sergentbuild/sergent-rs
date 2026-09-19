//! Runtime-owned Recipe configuration selected once before `Sergent`
//! construction. @sergent-rs-runtime/docs/KNOWLEDGE.md

use std::sync::Arc;

use sergent_rs_core::recipe::SergentRecipe;
use sergent_rs_core::registry::OperationRegistry;

/// One Recipe plus its fixed Operation Registry capability.
///
/// Registry presence and the Recipe's captured Intent source together express
/// the Run Kind. @sergent/docs/execution-model.md
pub struct ConfiguredRecipe<R: SergentRecipe> {
    recipe: R,
    #[allow(clippy::type_complexity)]
    registry: Option<Arc<OperationRegistry<R::Scene, R::Intent, R::Target>>>,
}

impl<R: SergentRecipe> ConfiguredRecipe<R> {
    /// Configure registry absence: Stop can succeed, but Continue cannot reach Plan.
    pub fn intent_only(recipe: R) -> Self {
        Self {
            recipe,
            registry: None,
        }
    }

    /// Configure one completed registry for either two-phase or Execution-Plan-Only Runs.
    ///
    /// The registry is wrapped without rebuilding its canonical schema.
    pub fn plan_capable(
        recipe: R,
        registry: OperationRegistry<R::Scene, R::Intent, R::Target>,
    ) -> Self {
        Self {
            recipe,
            registry: Some(Arc::new(registry)),
        }
    }

    /// Transfer the exact configured values into runtime construction.
    #[allow(clippy::type_complexity)]
    pub(super) fn into_parts(
        self,
    ) -> (
        R,
        Option<Arc<OperationRegistry<R::Scene, R::Intent, R::Target>>>,
    ) {
        (self.recipe, self.registry)
    }
}
