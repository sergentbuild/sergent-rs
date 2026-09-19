//! Compound Serde capture states for finite-aware direct-event conversion.

use serde::Serialize;
use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};

use super::finite::{FiniteError, FiniteSerializer, FiniteValue, single_entry};

/// Ordered children for sequences, tuples, and tuple structs.
pub(super) struct Sequence(pub(super) Vec<FiniteValue>);

impl SerializeSeq for Sequence {
    type Ok = FiniteValue;
    type Error = FiniteError;

    /// Capture one sequence child recursively.
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.0.push(value.serialize(FiniteSerializer)?);
        Ok(())
    }

    /// Close the ordered sequence.
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::Seq(self.0))
    }
}

impl SerializeTuple for Sequence {
    type Ok = FiniteValue;
    type Error = FiniteError;

    /// Capture one tuple child through sequence ownership.
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        SerializeSeq::serialize_element(self, value)
    }

    /// Close the tuple through sequence ownership.
    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}

impl SerializeTupleStruct for Sequence {
    type Ok = FiniteValue;
    type Error = FiniteError;

    /// Capture one tuple-struct field through sequence ownership.
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        SerializeSeq::serialize_element(self, value)
    }

    /// Close the tuple struct through sequence ownership.
    fn end(self) -> Result<Self::Ok, Self::Error> {
        SerializeSeq::end(self)
    }
}

/// One externally tagged tuple variant under construction.
pub(super) struct TupleVariant {
    pub(super) variant: &'static str,
    pub(super) values: Vec<FiniteValue>,
}

impl SerializeTupleVariant for TupleVariant {
    type Ok = FiniteValue;
    type Error = FiniteError;

    /// Capture one tuple-variant field recursively.
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.values.push(value.serialize(FiniteSerializer)?);
        Ok(())
    }

    /// Close the tuple variant under its external tag.
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(single_entry(self.variant, FiniteValue::Seq(self.values)))
    }
}

/// One map retaining source entry order until JSON key admission.
pub(super) struct Map {
    pub(super) entries: Vec<(FiniteValue, FiniteValue)>,
    pub(super) key: Option<FiniteValue>,
}

impl SerializeMap for Map {
    type Ok = FiniteValue;
    type Error = FiniteError;

    /// Capture one map key without projecting it early.
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.key = Some(key.serialize(FiniteSerializer)?);
        Ok(())
    }

    /// Pair one recursively captured value with its pending key.
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| FiniteError("map value serialized before its key".to_owned()))?;
        self.entries.push((key, value.serialize(FiniteSerializer)?));
        Ok(())
    }

    /// Close the map while preserving entry order.
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::Map(self.entries))
    }
}

/// One named-field struct retaining declaration order.
pub(super) struct Struct(pub(super) Vec<(FiniteValue, FiniteValue)>);

impl SerializeStruct for Struct {
    type Ok = FiniteValue;
    type Error = FiniteError;

    /// Capture one named field recursively.
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.0.push((
            FiniteValue::String(key.to_owned()),
            value.serialize(FiniteSerializer)?,
        ));
        Ok(())
    }

    /// Close the named-field struct as a map.
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::Map(self.0))
    }
}

/// One externally tagged named-field variant under construction.
pub(super) struct StructVariant {
    pub(super) variant: &'static str,
    pub(super) entries: Vec<(FiniteValue, FiniteValue)>,
}

impl SerializeStructVariant for StructVariant {
    type Ok = FiniteValue;
    type Error = FiniteError;

    /// Capture one named variant field recursively.
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.entries.push((
            FiniteValue::String(key.to_owned()),
            value.serialize(FiniteSerializer)?,
        ));
        Ok(())
    }

    /// Close the named variant under its external tag.
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(single_entry(self.variant, FiniteValue::Map(self.entries)))
    }
}
