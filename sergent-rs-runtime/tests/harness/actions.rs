//! The `DocActions` SceneActions fake: target selection, scene cloning, applying
//! a plan, and a configurable verification report. `scene_identity` derives a
//! scene's identity through these actions.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sergent_rs_core::operation::{OperationFault, PlanStep};
use sergent_rs_core::scene::{SceneActions, SceneIdentity, VerificationReport};

use sergent_rs_runtime::cancel::CancelToken;

use super::scene::{Doc, DocIntent, Spot, spot_id};

pub fn scene_identity(scene: &Doc) -> SceneIdentity {
    DocActions::new().identity(scene)
}

pub struct DocActions {
    pub apply_fault: Option<OperationFault>,
    pub verify_issues: Option<(String, Vec<String>)>,
    pub trip_in_verify: Option<CancelToken>,
    pub apply_calls: Arc<AtomicUsize>,
}

impl DocActions {
    pub fn new() -> Self {
        Self {
            apply_fault: None,
            verify_issues: None,
            trip_in_verify: None,
            apply_calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl Default for DocActions {
    fn default() -> Self {
        Self::new()
    }
}

impl SceneActions for DocActions {
    type Scene = Doc;
    type Intent = DocIntent;
    type Target = Spot;

    fn identity(&self, scene: &Doc) -> SceneIdentity {
        SceneIdentity {
            scene_id: scene.scene_id.clone(),
            revision: scene.revision,
        }
    }

    sergent_rs_core::clone_scene_via_clone!();

    fn select_target(&self, scene: &Doc) -> Option<Spot> {
        if scene.has_spot {
            Some(Spot { id: spot_id() })
        } else {
            None
        }
    }

    fn has_target(&self, scene: &Doc, _target: &Spot) -> bool {
        scene.has_spot
    }

    fn apply(
        &self,
        scene: &mut Doc,
        intent: &DocIntent,
        target: &Spot,
        operations: &[PlanStep<Doc, DocIntent, Spot>],
    ) -> Result<(), OperationFault> {
        self.apply_calls.fetch_add(1, Ordering::SeqCst);
        if let Some(fault) = &self.apply_fault {
            return Err(fault.clone());
        }
        for step in operations {
            step.operation().apply(scene, intent, target)?;
        }
        Ok(())
    }

    fn verify(
        &self,
        _before: &Doc,
        _after: &Doc,
        _target: &Spot,
        _operations: &[PlanStep<Doc, DocIntent, Spot>],
    ) -> VerificationReport {
        if let Some(token) = &self.trip_in_verify {
            token.cancel();
        }
        match &self.verify_issues {
            Some((first, additional)) => {
                VerificationReport::rejected(first.clone(), additional.iter().cloned())
            }
            None => VerificationReport::accepted(),
        }
    }
}
