//! The fake `Append` operation, its registry builder, and the plan-envelope
//! JSON constructor the model-client fakes return for the Plan call.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use sergent_rs_core::model::ParsedJsonObject;
use sergent_rs_core::operation::{Inadmissible, Operation, OperationFault};
use sergent_rs_core::registry::OperationRegistry;

use super::scene::{Doc, DocIntent, Spot};

#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Append {
    /// The non-empty text to append to the document.
    pub text: String,
}

impl Operation for Append {
    type Scene = Doc;
    type Intent = DocIntent;
    type Target = Spot;

    fn check_admissible(
        &self,
        _scene: &Doc,
        _intent: &DocIntent,
        _target: &Spot,
    ) -> Result<(), Inadmissible> {
        if self.text.is_empty() {
            Err(Inadmissible::new("append text must not be empty"))
        } else {
            Ok(())
        }
    }

    fn apply(
        &self,
        scene: &mut Doc,
        _intent: &DocIntent,
        _target: &Spot,
    ) -> Result<(), OperationFault> {
        scene.text.push_str(&self.text);
        scene.revision += 1;
        Ok(())
    }
}

pub fn append_registry(max: Option<usize>) -> OperationRegistry<Doc, DocIntent, Spot> {
    OperationRegistry::<Doc, DocIntent, Spot>::builder()
        .register::<Append>("append")
        .unwrap()
        .build(max)
        .unwrap()
}

pub fn plan_envelope(appends: &[&str]) -> ParsedJsonObject {
    let operations: Vec<Value> = appends
        .iter()
        .map(|text| json!({ "call": "append", "text": text }))
        .collect();
    super::object(json!({ "operations": operations }))
}
