use serde::Serialize;
use sergent_rs_core::ids::{RunId, SceneId};
use sergent_rs_core::scene::SceneIdentity;
use sergent_rs_core::vocab::{ProgressStatus, Stage};

/// The exact portable five-field progress projection. Scene identity remains
/// absent, with revision zero, until the run binds its observed Scene.
/// @sergent/docs/observability.md
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProgressSnapshot {
    /// The stable run identity.
    pub run_id: RunId,
    /// The observed Scene identity, absent before Scene binding.
    pub scene_id: Option<SceneId>,
    /// The current stage.
    pub stage: Stage,
    /// The sanitized status.
    pub status: ProgressStatus,
    /// The observed revision, or zero before Scene binding.
    pub revision: u64,
}

impl ProgressSnapshot {
    /// The initial queued snapshot before the pipeline observes a Scene.
    pub fn initial(run_id: RunId) -> Self {
        Self {
            run_id,
            scene_id: None,
            stage: Stage::Queued,
            status: ProgressStatus::Queued,
            revision: 0,
        }
    }

    /// Record the observed Scene identity on the snapshot.
    pub(crate) fn observe(&mut self, identity: &SceneIdentity) {
        self.scene_id = Some(identity.scene_id.clone());
        self.revision = identity.revision;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serialization_is_exact_before_and_after_scene_binding() {
        let run_id = RunId::parse("run_00000000000000000000000000000001").unwrap();
        let mut snapshot = ProgressSnapshot::initial(run_id);

        assert_eq!(
            serde_json::to_value(&snapshot).unwrap(),
            json!({
                "run_id": "run_00000000000000000000000000000001",
                "scene_id": null,
                "stage": "queued",
                "status": "queued",
                "revision": 0,
            })
        );

        snapshot.observe(&SceneIdentity {
            scene_id: SceneId::parse("doc_00000000000000000000000000000002").unwrap(),
            revision: 7,
        });
        snapshot.stage = Stage::Started;
        snapshot.status = ProgressStatus::Running;

        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            json!({
                "run_id": "run_00000000000000000000000000000001",
                "scene_id": "doc_00000000000000000000000000000002",
                "stage": "started",
                "status": "running",
                "revision": 7,
            })
        );
    }
}
