//! Derivation of the canonical proposal schema from one exact typed proposal
//! definition, and the schemars generation profile it uses.
//! @sergent/docs/framework.md

use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::Value;

use super::dialect::validate_dialect;
use super::normalize::normalize_node;
use super::schema::{ProposalSchema, SchemaError, name_is_valid};

/// Derive the canonical proposal schema from one exact typed proposal
/// definition, normalizing and proving the dialect at construction.
/// @sergent/docs/framework.md
///
/// `T` must follow the trusted derive-only binding profile documented in the
/// crate knowledge base. Source-only Serde/Schemars behavior cannot be proved
/// from the `JsonSchema` bound or emitted schema.
pub fn derive_proposal_schema<T: JsonSchema>() -> Result<ProposalSchema, SchemaError> {
    let name = T::schema_name().into_owned();
    if !name_is_valid(&name) {
        return Err(SchemaError::InvalidName { name });
    }
    let mut schema = raw_schema::<T>();
    normalize_node(&mut schema);
    validate_dialect(&schema, &name)?;
    Ok(ProposalSchema::from_proved_parts(name, schema))
}

/// Generate the schemars schema for one type under JSON Schema 2020-12 and the
/// deserialize contract, with the meta-schema declaration suppressed.
pub(crate) fn raw_schema<T: JsonSchema>() -> Value {
    let mut settings = SchemaSettings::draft2020_12().for_deserialize();
    settings.meta_schema = None;
    let generator = settings.into_generator();
    let schema = generator.into_root_schema_for::<T>();
    serde_json::to_value(schema).expect("a schemars schema is JSON-compatible")
}
