//! Canonical registry references with escaped JSON Pointer definition names.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sergent_rs_core::ids::TargetId;
use sergent_rs_core::operation::{Operation, OperationFault};
use sergent_rs_core::registry::OperationRegistry;
use sergent_rs_core::target::Target;

struct Scene;
struct Intent;

#[derive(Serialize)]
struct SceneTarget(TargetId);

impl Target for SceneTarget {
    fn target_id(&self) -> &TargetId {
        &self.0
    }
}

#[allow(dead_code)]
#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Nested/Type~Leaf")]
struct EscapedNested {
    value: bool,
}

#[allow(dead_code)]
#[derive(Clone, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Root/Operation~Leaf")]
struct EscapedOperation {
    nested: EscapedNested,
}

impl Operation for EscapedOperation {
    type Scene = Scene;
    type Intent = Intent;
    type Target = SceneTarget;

    fn apply(
        &self,
        _scene: &mut Scene,
        _intent: &Intent,
        _target: &SceneTarget,
    ) -> Result<(), OperationFault> {
        Ok(())
    }
}

fn reference_at<'a>(value: &'a Value, pointer: &str) -> &'a str {
    value.pointer(pointer).unwrap().as_str().unwrap()
}

fn assert_reference_resolves(document: &Value, reference: &str) {
    let pointer = reference.strip_prefix('#').unwrap();
    assert!(
        document.pointer(pointer).is_some(),
        "unresolved {reference}"
    );
}

#[test]
fn root_and_nested_definition_references_escape_and_resolve() {
    let registry = OperationRegistry::<Scene, Intent, SceneTarget>::builder()
        .register::<EscapedOperation>("escaped")
        .unwrap()
        .build(None)
        .unwrap();
    let schema = registry.plan_schema();
    let document = schema.json_schema();

    let root_reference = reference_at(document, "/properties/operations/items/$ref");
    assert_eq!(root_reference, "#/$defs/Root~1Operation~0Leaf");
    assert_reference_resolves(document, root_reference);

    let nested_reference = reference_at(
        document,
        "/$defs/Root~1Operation~0Leaf/properties/nested/$ref",
    );
    assert_eq!(nested_reference, "#/$defs/Nested~1Type~0Leaf");
    assert_reference_resolves(document, nested_reference);
}
