//! Best-effort outbound capture of application-shaped evidence.
//! @sergent/docs/run-record-spec.md

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};
use serde_json::Value;

use crate::error::{ErrorKind, RunError};

/// One application-shaped value projected into inert JSON evidence. A local
/// serialization failure is retained inside the envelope and never controls
/// the run. @sergent/docs/run-record-spec.md
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedValue {
    value: Option<Value>,
    value_type: &'static str,
    error: Option<RunError>,
}

impl CapturedValue {
    /// Capture one concrete serializable value without exposing a failure to
    /// run control.
    pub fn capture<T: Serialize + ?Sized>(value: &T) -> Self {
        Self::from_projection(
            std::any::type_name::<T>(),
            serde_json::to_value(value).map_err(|error| error.to_string()),
        )
    }

    /// Capture a projection assembled by a typed owner that cannot derive
    /// `Serialize`, such as a heterogeneous Operation script.
    pub(crate) fn from_projection(
        value_type: &'static str,
        projection: Result<Value, String>,
    ) -> Self {
        match projection {
            Ok(value) => Self {
                value: Some(value),
                value_type,
                error: None,
            },
            Err(message) => Self {
                value: None,
                value_type,
                error: Some(
                    RunError::of(
                        ErrorKind::CaptureError,
                        format!("failed to capture {value_type}: {message}"),
                    )
                    .with("value_type", value_type),
                ),
            },
        }
    }

    /// Borrow the captured JSON value, absent when projection failed.
    pub fn value(&self) -> Option<&Value> {
        self.value.as_ref()
    }

    /// The implementation-native concrete type name retained for diagnosis.
    pub fn value_type(&self) -> &str {
        self.value_type
    }

    /// Borrow the contained capture failure, when one occurred.
    pub fn error(&self) -> Option<&RunError> {
        self.error.as_ref()
    }
}

impl Serialize for CapturedValue {
    /// Serialize the exact four-field envelope, deriving status from error.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut capture = serializer.serialize_struct("CapturedValue", 4)?;
        capture.serialize_field("value", &self.value)?;
        capture.serialize_field("value_type", self.value_type)?;
        capture.serialize_field("error", &self.error)?;
        let status = if self.error.is_some() {
            "capture_error"
        } else {
            "captured"
        };
        capture.serialize_field("status", status)?;
        capture.end()
    }
}
