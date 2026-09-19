//! One-pass Serde capture that rejects non-finite floats before exact JSON projection.

use std::fmt::{self, Display};

use serde::Serialize;
use serde::ser::{self, SerializeMap};
use serde_json::Value;

use super::RunRecordFileError;
use super::finite_compound::{Map, Sequence, Struct, StructVariant, TupleVariant};

/// Serialize once into a float-preserving tree, then project that tree to JSON.
pub(crate) fn to_json<T: Serialize + ?Sized>(value: &T) -> Result<Value, RunRecordFileError> {
    let captured = value.serialize(FiniteSerializer).map_err(|error| {
        RunRecordFileError::conversion(format!("event value conversion failed: {error}"))
    })?;
    serde_json::to_value(captured).map_err(|error| {
        RunRecordFileError::conversion(format!("event value conversion failed: {error}"))
    })
}

/// Serde's complete data model retained without converting floats to JSON yet.
pub(super) enum FiniteValue {
    Bool(bool),
    I64(i64),
    I128(i128),
    U64(u64),
    U128(u128),
    F32(f32),
    F64(f64),
    Char(char),
    String(String),
    Bytes(Vec<u8>),
    Unit,
    Option(Option<Box<Self>>),
    Newtype(Box<Self>),
    Seq(Vec<Self>),
    Map(Vec<(Self, Self)>),
}

impl Serialize for FiniteValue {
    /// Replay the captured data model without revisiting the caller's serializer.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::I64(value) => serializer.serialize_i64(*value),
            Self::I128(value) => serializer.serialize_i128(*value),
            Self::U64(value) => serializer.serialize_u64(*value),
            Self::U128(value) => serializer.serialize_u128(*value),
            Self::F32(value) => serializer.serialize_f32(*value),
            Self::F64(value) => serializer.serialize_f64(*value),
            Self::Char(value) => serializer.serialize_char(*value),
            Self::String(value) => serializer.serialize_str(value),
            Self::Bytes(value) => serializer.serialize_bytes(value),
            Self::Unit => serializer.serialize_unit(),
            Self::Option(None) => serializer.serialize_none(),
            Self::Option(Some(value)) => serializer.serialize_some(value),
            Self::Newtype(value) => serializer.serialize_newtype_struct("", value),
            Self::Seq(values) => values.serialize(serializer),
            Self::Map(entries) => serialize_map(entries, serializer),
        }
    }
}

/// Replay captured entries in their original serialization order.
fn serialize_map<S: serde::Serializer>(
    entries: &[(FiniteValue, FiniteValue)],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(entries.len()))?;
    for (key, value) in entries {
        map.serialize_entry(key, value)?;
    }
    map.end()
}

/// A direct-event conversion rejection produced during Serde capture.
#[derive(Debug)]
pub(super) struct FiniteError(pub(super) String);

impl Display for FiniteError {
    /// Render the exact capture diagnostic.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FiniteError {}

impl ser::Error for FiniteError {
    /// Retain custom errors returned by application serializers.
    fn custom<T: Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

/// Serializer that captures every supported Serde value and rejects bad floats.
pub(super) struct FiniteSerializer;

impl serde::Serializer for FiniteSerializer {
    type Ok = FiniteValue;
    type Error = FiniteError;
    type SerializeSeq = Sequence;
    type SerializeTuple = Sequence;
    type SerializeTupleStruct = Sequence;
    type SerializeTupleVariant = TupleVariant;
    type SerializeMap = Map;
    type SerializeStruct = Struct;
    type SerializeStructVariant = StructVariant;

    /// Capture one boolean.
    fn serialize_bool(self, value: bool) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::Bool(value))
    }

    /// Capture one signed eight-bit integer.
    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(i64::from(value))
    }

    /// Capture one signed sixteen-bit integer.
    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(i64::from(value))
    }

    /// Capture one signed thirty-two-bit integer.
    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        self.serialize_i64(i64::from(value))
    }

    /// Capture one signed sixty-four-bit integer.
    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::I64(value))
    }

    /// Capture one signed wide integer for later JSON range admission.
    fn serialize_i128(self, value: i128) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::I128(value))
    }

    /// Capture one unsigned eight-bit integer.
    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(u64::from(value))
    }

    /// Capture one unsigned sixteen-bit integer.
    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(u64::from(value))
    }

    /// Capture one unsigned thirty-two-bit integer.
    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        self.serialize_u64(u64::from(value))
    }

    /// Capture one unsigned sixty-four-bit integer.
    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::U64(value))
    }

    /// Capture one unsigned wide integer for later JSON range admission.
    fn serialize_u128(self, value: u128) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::U128(value))
    }

    /// Reject non-finite f32 before retaining a finite value.
    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        finite(value).map(FiniteValue::F32)
    }

    /// Reject non-finite f64 before retaining a finite value.
    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        finite(value).map(FiniteValue::F64)
    }

    /// Capture one character.
    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::Char(value))
    }

    /// Capture one exact string.
    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::String(value.to_owned()))
    }

    /// Capture one exact byte sequence.
    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::Bytes(value.to_vec()))
    }

    /// Capture an absent optional value.
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::Option(None))
    }

    /// Capture one present optional value recursively.
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        value
            .serialize(self)
            .map(|value| FiniteValue::Option(Some(Box::new(value))))
    }

    /// Capture unit as the JSON null class.
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(FiniteValue::Unit)
    }

    /// Capture a unit struct as unit.
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.serialize_unit()
    }

    /// Capture a unit variant as its exact name.
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.serialize_str(variant)
    }

    /// Capture a newtype struct without changing its JSON shape.
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value
            .serialize(self)
            .map(|value| FiniteValue::Newtype(Box::new(value)))
    }

    /// Capture an externally tagged newtype variant.
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        let value = value.serialize(self)?;
        Ok(single_entry(variant, value))
    }

    /// Open one ordered sequence capture.
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(Sequence(Vec::with_capacity(len.unwrap_or(0))))
    }

    /// Open one tuple as an ordered sequence.
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        self.serialize_seq(Some(len))
    }

    /// Open one tuple struct as an ordered sequence.
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        self.serialize_seq(Some(len))
    }

    /// Open one externally tagged tuple variant.
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Ok(TupleVariant {
            variant,
            values: Vec::with_capacity(len),
        })
    }

    /// Open one map while retaining entry order.
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(Map {
            entries: Vec::with_capacity(len.unwrap_or(0)),
            key: None,
        })
    }

    /// Open one named-field struct capture.
    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(Struct(Vec::with_capacity(len)))
    }

    /// Open one externally tagged named-field variant.
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Ok(StructVariant {
            variant,
            entries: Vec::with_capacity(len),
        })
    }

    /// Capture Display-based Serde values as exact strings.
    fn collect_str<T: Display + ?Sized>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        self.serialize_str(&value.to_string())
    }
}

/// Reject any non-finite native float without converting it to null.
fn finite<T: Float>(value: T) -> Result<T, FiniteError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(FiniteError(
            "event value contains NaN or infinity".to_owned(),
        ))
    }
}

/// The one float capability shared by f32 and f64 capture.
trait Float: Copy {
    /// Report native finiteness without conversion.
    fn is_finite(self) -> bool;
}

impl Float for f32 {
    /// Delegate to the native f32 classification.
    fn is_finite(self) -> bool {
        f32::is_finite(self)
    }
}

impl Float for f64 {
    /// Delegate to the native f64 classification.
    fn is_finite(self) -> bool {
        f64::is_finite(self)
    }
}

/// Build the externally tagged representation of one enum variant.
pub(super) fn single_entry(variant: &'static str, value: FiniteValue) -> FiniteValue {
    FiniteValue::Map(vec![(FiniteValue::String(variant.to_owned()), value)])
}
