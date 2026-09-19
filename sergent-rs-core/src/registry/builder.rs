//! Registry construction and canonical Plan schema assembly.

use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::operation::Operation;
use crate::proposal::{
    ProposalSchema, SchemaError, definition_reference, normalize_node, raw_schema,
    strengthen_required, validate_dialect,
};
use crate::target::Target;

use super::branch::{Branch, DecodeFn, decode_operation};
use super::decode::OperationRegistry;
use super::errors::RegistryError;

/// A construction-time builder for an `OperationRegistry`.
/// @sergent/docs/framework.md
pub struct OperationRegistryBuilder<S, I, T: Target> {
    pub(super) branches: Vec<Branch<S, I, T>>,
    pub(super) by_call: BTreeMap<String, usize>,
}

impl<S, I, T: Target> OperationRegistryBuilder<S, I, T> {
    /// Register one Operation branch declaration.
    ///
    /// `call` is the branch's explicit non-empty model-visible discriminator.
    /// The registry retains it for schema composition, decode, and Plan steps.
    ///
    /// `Op` must follow the trusted derive-only binding profile documented in
    /// the crate knowledge base. Source-only Serde/Schemars behavior cannot be
    /// proved from these generic bounds or the emitted schema.
    pub fn register<Op>(mut self, call: &'static str) -> Result<Self, RegistryError>
    where
        Op: Operation<Scene = S, Intent = I, Target = T>
            + Clone
            + DeserializeOwned
            + Serialize
            + JsonSchema
            + 'static,
    {
        let type_name = std::any::type_name::<Op>();
        if call.is_empty() {
            return Err(RegistryError::EmptyDiscriminator { type_name });
        }
        if let Some(&existing) = self.by_call.get(call) {
            return Err(RegistryError::DuplicateDiscriminator {
                call: call.to_owned(),
                first: self.branches[existing].type_name,
                second: type_name,
            });
        }

        let def_name = Op::schema_name().into_owned();
        let mut schema = raw_schema::<Op>();
        normalize_node(&mut schema);
        let nested_defs = match schema.as_object_mut().and_then(|map| map.remove("$defs")) {
            Some(Value::Object(defs)) => defs,
            Some(_) => {
                return Err(RegistryError::Schema(SchemaError::Dialect {
                    proposal: def_name,
                    pointer: "/$defs".to_owned(),
                    message: "$defs must be an object".to_owned(),
                }));
            }
            None => Map::new(),
        };
        inject_call(&mut schema, call, type_name)?;

        let decode: DecodeFn<S, I, T> = decode_operation::<Op, S, I, T>;

        self.by_call.insert(call.to_owned(), self.branches.len());
        self.branches.push(Branch {
            type_name,
            call,
            def_name,
            def_body: schema,
            nested_defs,
            decode,
        });
        Ok(self)
    }

    /// Compose the Plan envelope and finish the registry. A configured maximum
    /// is only expressible with a registry, so a maximum without a registry is
    /// unrepresentable.
    pub fn build(
        self,
        max_operations: Option<usize>,
    ) -> Result<OperationRegistry<S, I, T>, RegistryError> {
        if self.branches.is_empty() {
            return Err(RegistryError::EmptyRegistry);
        }
        if let Some(max) = max_operations
            && max < 1
        {
            return Err(RegistryError::InvalidMaximum { max });
        }

        let mut defs: Map<String, Value> = Map::new();
        let mut refs: Vec<Value> = Vec::with_capacity(self.branches.len());
        for branch in &self.branches {
            merge_def(
                &mut defs,
                &branch.def_name,
                &branch.def_body,
                branch.type_name,
            )?;
            for (name, schema) in &branch.nested_defs {
                merge_def(&mut defs, name, schema, branch.type_name)?;
            }
            refs.push(json!({ "$ref": definition_reference(&branch.def_name) }));
        }

        let items = if refs.len() == 1 {
            refs.into_iter().next().expect("one ref")
        } else {
            json!({ "anyOf": refs })
        };
        let mut operations = Map::new();
        operations.insert("type".to_owned(), json!("array"));
        operations.insert("items".to_owned(), items);
        operations.insert("minItems".to_owned(), json!(1));
        if let Some(max) = max_operations {
            operations.insert("maxItems".to_owned(), json!(max));
        }

        let envelope = json!({
            "type": "object",
            "properties": { "operations": Value::Object(operations) },
            "additionalProperties": false,
            "required": ["operations"],
            "$defs": Value::Object(defs),
        });
        validate_dialect(&envelope, "PlanProposal")?;
        let plan_schema = Arc::new(ProposalSchema::from_proved_parts(
            "PlanProposal".to_owned(),
            envelope,
        ));

        Ok(OperationRegistry {
            branches: self.branches,
            by_call: self.by_call,
            max_operations,
            plan_schema,
        })
    }
}

/// Inject a fixed required `call` property into an object branch, rejecting a
/// non-object schema or application-owned `call` field.
fn inject_call(
    schema: &mut Value,
    call: &str,
    type_name: &'static str,
) -> Result<(), RegistryError> {
    let object = schema
        .as_object_mut()
        .ok_or(RegistryError::BranchNotObject { type_name })?;
    if object.get("type").and_then(Value::as_str) != Some("object") {
        return Err(RegistryError::BranchNotObject { type_name });
    }
    {
        let properties = object
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or(RegistryError::BranchNotObject { type_name })?;
        if properties.contains_key("call") {
            return Err(RegistryError::ReservedCallField { type_name });
        }
        properties.insert(
            "call".to_owned(),
            json!({ "type": "string", "enum": [call] }),
        );
    }
    strengthen_required(object);
    Ok(())
}

/// Share an equal definition by name, insert a new definition, or reject an
/// unequal name collision.
fn merge_def(
    defs: &mut Map<String, Value>,
    name: &str,
    schema: &Value,
    owner: &'static str,
) -> Result<(), RegistryError> {
    match defs.get(name) {
        Some(existing) if existing == schema => Ok(()),
        Some(_) => Err(RegistryError::DefinitionCollision {
            name: name.to_owned(),
            owner,
        }),
        None => {
            defs.insert(name.to_owned(), schema.clone());
            Ok(())
        }
    }
}
