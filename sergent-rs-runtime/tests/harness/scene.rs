//! The document-domain fakes: the `Doc` scene, its `Spot` target, the `DocIntent`
//! and its `IntentKind`, and the `DocMind` MindBuf. These are the passive types
//! the rest of the harness builds behavior on.

use serde::Serialize;
use serde_json::{Map, Value, json};

use sergent_rs_core::ids::{SceneId, TargetId};
use sergent_rs_core::intent::{Intent, IntentFlow};
use sergent_rs_core::mindbuf::MindBuf;
use sergent_rs_core::target::Target;

// --- scene, target, intent ---

#[derive(Clone)]
pub struct Doc {
    pub scene_id: SceneId,
    pub revision: u64,
    pub text: String,
    pub has_spot: bool,
}

pub fn doc(text: &str, revision: u64) -> Doc {
    Doc {
        scene_id: SceneId::parse("doc_00000000000000000000000000000000").unwrap(),
        revision,
        text: text.to_owned(),
        has_spot: true,
    }
}

#[derive(Serialize)]
pub struct Spot {
    pub(super) id: TargetId,
}

impl Target for Spot {
    fn target_id(&self) -> &TargetId {
        &self.id
    }
}

pub fn spot_id() -> TargetId {
    TargetId::parse("spot_00000000000000000000000000000000").unwrap()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum IntentKind {
    #[default]
    Continue,
    Stop,
}

#[derive(Clone, Serialize)]
pub struct DocIntent {
    pub kind: IntentKind,
}

impl Intent for DocIntent {
    fn flow(&self) -> IntentFlow {
        match self.kind {
            IntentKind::Continue => IntentFlow::Continue,
            IntentKind::Stop => IntentFlow::Stop,
        }
    }

    fn terminal_message(&self) -> Option<String> {
        match self.kind {
            IntentKind::Stop => Some("nothing to do".to_owned()),
            _ => None,
        }
    }

    fn terminal_metadata(&self) -> Map<String, Value> {
        let mut metadata = Map::new();
        if self.kind == IntentKind::Stop {
            metadata.insert("reason".to_owned(), json!("already_satisfied"));
        }
        metadata
    }
}

// --- mindbuf ---

pub struct DocMind;

impl MindBuf for DocMind {
    fn export(&self) -> String {
        "recent human activity".to_owned()
    }
}
