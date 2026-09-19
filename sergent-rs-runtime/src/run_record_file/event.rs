//! Direct-event validation and the runtime-owned JSON conversion graph.

use std::collections::{BTreeMap, HashSet};
use std::fmt::{Debug, Display};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use serde_json::{Map, Number, Value, json};
use sergent_rs_core::ids::{RunId, SceneId};

use crate::progress::ProgressSnapshot;

use super::RunRecordFileError;

const MAX_FALLBACK_CHARS: usize = 2_048;

/// Independently optional run, Scene, and revision envelope correlation.
/// @sergent/docs/run-record-file-format.md
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunRecordCorrelation {
    pub(crate) run_id: Option<String>,
    pub(crate) scene_id: Option<String>,
    pub(crate) revision: Option<u64>,
}

impl RunRecordCorrelation {
    /// Admit optional correlation values without checking them against a Scene.
    pub fn new(
        run_id: Option<String>,
        scene_id: Option<String>,
        revision: Option<u64>,
    ) -> Result<Self, RunRecordFileError> {
        validate_optional_id("run id", run_id.as_deref())?;
        validate_optional_id("scene id", scene_id.as_deref())?;
        Ok(Self {
            run_id,
            scene_id,
            revision,
        })
    }

    /// Construct correlation for one admitted Scene identity.
    pub fn scene(scene_id: &SceneId, revision: u64) -> Self {
        Self {
            run_id: None,
            scene_id: Some(scene_id.as_str().to_owned()),
            revision: Some(revision),
        }
    }

    /// Construct complete correlation for one run over one admitted Scene.
    pub fn run_scene(run_id: &RunId, scene_id: &SceneId, revision: u64) -> Self {
        Self {
            run_id: Some(run_id.as_str().to_owned()),
            scene_id: Some(scene_id.as_str().to_owned()),
            revision: Some(revision),
        }
    }

    /// Project one trusted started snapshot into its exact envelope correlation.
    pub(crate) fn from_progress(progress: &ProgressSnapshot) -> Self {
        Self {
            run_id: Some(progress.run_id.as_str().to_owned()),
            scene_id: progress
                .scene_id
                .as_ref()
                .map(|scene_id| scene_id.as_str().to_owned()),
            revision: progress.scene_id.as_ref().map(|_| progress.revision),
        }
    }
}

/// One validated direct event and its separately owned correlation.
/// @sergent/docs/run-record-file-format.md
pub struct RunRecordEvent {
    pub(crate) name: String,
    pub(crate) payload: Option<RunRecordEventValue>,
    pub(crate) correlation: RunRecordCorrelation,
}

impl RunRecordEvent {
    /// Admit a non-blank verbatim name and an optional direct-event value.
    pub fn new(
        name: impl Into<String>,
        payload: Option<RunRecordEventValue>,
        correlation: RunRecordCorrelation,
    ) -> Result<Self, RunRecordFileError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(RunRecordFileError::validation(
                "Run Record event name must contain a non-whitespace character",
            ));
        }
        Ok(Self {
            name,
            payload,
            correlation,
        })
    }

    /// Convert the payload completely before yielding facts to writer authority.
    pub(crate) fn into_json(
        self,
    ) -> Result<(String, Value, RunRecordCorrelation), RunRecordFileError> {
        let payload = match self.payload {
            Some(payload) => payload.project()?,
            None => json!({}),
        };
        Ok((self.name, payload, self.correlation))
    }
}

/// A shared identity-bearing array node for direct-event value graphs.
/// @sergent/docs/run-record-file-format.md
#[derive(Clone, Default)]
pub struct RunRecordEventArray {
    values: Arc<Mutex<Vec<RunRecordEventValue>>>,
}

impl RunRecordEventArray {
    /// Construct one array from its current ordered children.
    pub fn new(values: impl IntoIterator<Item = RunRecordEventValue>) -> Self {
        Self {
            values: Arc::new(Mutex::new(values.into_iter().collect())),
        }
    }

    /// Append a child, including a shared child or a deliberate cycle.
    pub fn push(&self, value: RunRecordEventValue) {
        lock(&self.values).push(value);
    }
}

/// A shared identity-bearing object node for direct-event value graphs.
/// @sergent/docs/run-record-file-format.md
#[derive(Clone, Default)]
pub struct RunRecordEventObject {
    values: Arc<Mutex<BTreeMap<String, RunRecordEventValue>>>,
}

impl RunRecordEventObject {
    /// Construct an empty object node.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace one exact JSON object key.
    pub fn insert(&self, key: impl Into<String>, value: RunRecordEventValue) {
        lock(&self.values).insert(key.into(), value);
    }
}

/// One direct-event value node, deliberately separate from Run Record capture.
/// @sergent/docs/run-record-file-format.md
#[derive(Clone)]
pub struct RunRecordEventValue(Arc<ValueNode>);

/// The closed native classes admitted by direct-event conversion.
enum ValueNode {
    Json(Value),
    Number(f64),
    Temporal(String),
    Binary(String),
    Path(PathBuf),
    Exception { type_name: String, message: String },
    Degraded { type_name: String, repr: String },
    Array(RunRecordEventArray),
    Object(RunRecordEventObject),
}

impl RunRecordEventValue {
    /// Carry an already formed JSON value without using Run Record capture policy.
    pub fn json(value: Value) -> Self {
        Self(Arc::new(ValueNode::Json(value)))
    }

    /// Carry a floating-point value whose finiteness is checked at conversion.
    pub fn number(value: f64) -> Self {
        Self(Arc::new(ValueNode::Number(value)))
    }

    /// Carry the native string projection of one temporal value.
    pub fn temporal(value: impl Display) -> Self {
        Self(Arc::new(ValueNode::Temporal(value.to_string())))
    }

    /// Carry the native string projection of one binary value.
    pub fn binary(value: impl Into<String>) -> Self {
        Self(Arc::new(ValueNode::Binary(value.into())))
    }

    /// Carry a path for lossless Unicode or native lossy string projection.
    pub fn path(value: impl Into<PathBuf>) -> Self {
        Self(Arc::new(ValueNode::Path(value.into())))
    }

    /// Carry one exception-like value as qualified type and bounded message.
    pub fn exception<E: std::error::Error + ?Sized + 'static>(error: &E) -> Self {
        Self(Arc::new(ValueNode::Exception {
            type_name: std::any::type_name::<E>().to_owned(),
            message: bounded(error.to_string()),
        }))
    }

    /// Carry one otherwise unconvertible value as type and bounded rendering.
    pub fn degraded<T: Debug + ?Sized + 'static>(value: &T) -> Self {
        Self(Arc::new(ValueNode::Degraded {
            type_name: std::any::type_name::<T>().to_owned(),
            repr: bounded(format!("{value:?}")),
        }))
    }

    /// Convert any serializable caller value before the record operation begins.
    pub fn from_serializable<T: Serialize + ?Sized>(value: &T) -> Result<Self, RunRecordFileError> {
        super::finite::to_json(value).map(Self::json)
    }

    /// Project one complete graph using a fresh active recursion stack.
    fn project(&self) -> Result<Value, RunRecordFileError> {
        self.project_with(&mut HashSet::new())
    }

    /// Project one node while rejecting only container identity on the active stack.
    fn project_with(&self, active: &mut HashSet<usize>) -> Result<Value, RunRecordFileError> {
        match self.0.as_ref() {
            ValueNode::Json(value) => Ok(value.clone()),
            ValueNode::Number(value) => {
                Number::from_f64(*value).map(Value::Number).ok_or_else(|| {
                    RunRecordFileError::conversion("event value contains NaN or infinity")
                })
            }
            ValueNode::Temporal(value) | ValueNode::Binary(value) => {
                Ok(Value::String(value.clone()))
            }
            ValueNode::Path(value) => Ok(Value::String(value.to_string_lossy().into_owned())),
            ValueNode::Exception { type_name, message } => {
                Ok(json!({ "message": message, "type": type_name }))
            }
            ValueNode::Degraded { type_name, repr } => {
                Ok(json!({ "repr": repr, "type": type_name }))
            }
            ValueNode::Array(array) => project_array(array, active),
            ValueNode::Object(object) => project_object(object, active),
        }
    }
}

impl From<RunRecordEventArray> for RunRecordEventValue {
    /// Retain the array's shared identity for DAG and cycle detection.
    fn from(value: RunRecordEventArray) -> Self {
        Self(Arc::new(ValueNode::Array(value)))
    }
}

impl From<RunRecordEventObject> for RunRecordEventValue {
    /// Retain the object's shared identity for DAG and cycle detection.
    fn from(value: RunRecordEventObject) -> Self {
        Self(Arc::new(ValueNode::Object(value)))
    }
}

/// Snapshot and project one array after its identity enters the active stack.
fn project_array(
    array: &RunRecordEventArray,
    active: &mut HashSet<usize>,
) -> Result<Value, RunRecordFileError> {
    let identity = Arc::as_ptr(&array.values) as usize;
    enter(identity, active)?;
    let children = lock(&array.values).clone();
    let result = children
        .iter()
        .map(|child| child.project_with(active))
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array);
    active.remove(&identity);
    result
}

/// Snapshot and project one object after its identity enters the active stack.
fn project_object(
    object: &RunRecordEventObject,
    active: &mut HashSet<usize>,
) -> Result<Value, RunRecordFileError> {
    let identity = Arc::as_ptr(&object.values) as usize;
    enter(identity, active)?;
    let children = lock(&object.values).clone();
    let result = children
        .iter()
        .map(|(key, child)| Ok((key.clone(), child.project_with(active)?)))
        .collect::<Result<Map<_, _>, RunRecordFileError>>()
        .map(Value::Object);
    active.remove(&identity);
    result
}

/// Reject re-entry while allowing a shared node after its earlier visit returns.
fn enter(identity: usize, active: &mut HashSet<usize>) -> Result<(), RunRecordFileError> {
    if active.insert(identity) {
        Ok(())
    } else {
        Err(RunRecordFileError::conversion(
            "event value contains a cyclic container",
        ))
    }
}

/// Require each independently optional correlation identifier to be non-empty.
fn validate_optional_id(
    label: &'static str,
    value: Option<&str>,
) -> Result<(), RunRecordFileError> {
    if value == Some("") {
        Err(RunRecordFileError::validation(format!(
            "Run Record correlation {label} must be non-empty when present"
        )))
    } else {
        Ok(())
    }
}

/// Bound exception and degraded fallback prose by Unicode scalar count.
fn bounded(value: String) -> String {
    value.chars().take(MAX_FALLBACK_CHARS).collect()
}

/// Recover graph ownership after a producer panic without inventing another value.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use serde_json::json;

    use super::{
        RunRecordCorrelation, RunRecordEvent, RunRecordEventArray, RunRecordEventObject,
        RunRecordEventValue,
    };

    #[test]
    fn direct_value_classes_project_exactly() {
        #[derive(Debug)]
        /// One opaque native value projected through the degraded class.
        struct Opaque;

        let object = RunRecordEventObject::new();
        object.insert("json", RunRecordEventValue::json(json!({ "z": 1 })));
        object.insert("number", RunRecordEventValue::number(2.5));
        object.insert("temporal", RunRecordEventValue::temporal("2026-08-31"));
        object.insert("binary", RunRecordEventValue::binary("AQI="));
        object.insert("path", RunRecordEventValue::path("a/b"));
        let exception = std::io::Error::other("failed");
        object.insert("exception", RunRecordEventValue::exception(&exception));
        object.insert("degraded", RunRecordEventValue::degraded(&Opaque));
        let array = RunRecordEventArray::new([RunRecordEventValue::json(json!(true))]);
        object.insert("array", array.into());
        assert_eq!(
            RunRecordEventValue::from(object).project().unwrap(),
            json!({
                "array": [true],
                "binary": "AQI=",
                "degraded": { "repr": "Opaque", "type": std::any::type_name::<Opaque>() },
                "exception": { "message": "failed", "type": std::any::type_name::<std::io::Error>() },
                "json": { "z": 1 },
                "number": 2.5,
                "path": "a/b",
                "temporal": "2026-08-31"
            })
        );
    }

    #[test]
    fn a_shared_dag_is_valid_but_a_true_cycle_and_non_finite_number_fail() {
        let child = RunRecordEventObject::new();
        child.insert("value", RunRecordEventValue::json(json!(7)));
        let root = RunRecordEventArray::new([
            RunRecordEventValue::from(child.clone()),
            RunRecordEventValue::from(child),
        ]);
        assert_eq!(
            RunRecordEventValue::from(root).project().unwrap(),
            json!([{ "value": 7 }, { "value": 7 }])
        );

        let cyclic = RunRecordEventArray::default();
        cyclic.push(RunRecordEventValue::from(cyclic.clone()));
        assert!(RunRecordEventValue::from(cyclic).project().is_err());
        assert!(RunRecordEventValue::number(f64::NAN).project().is_err());
        assert!(
            RunRecordEventValue::number(f64::INFINITY)
                .project()
                .is_err()
        );
    }

    #[test]
    fn names_correlation_absence_and_conversion_failure_are_explicit() {
        assert!(RunRecordEvent::new("  ", None, RunRecordCorrelation::default()).is_err());
        assert!(RunRecordCorrelation::new(Some(String::new()), None, None).is_err());
        let event =
            RunRecordEvent::new(" exact name ", None, RunRecordCorrelation::default()).unwrap();
        let (name, payload, _) = event.into_json().unwrap();
        assert_eq!(name, " exact name ");
        assert_eq!(payload, json!({}));

        /// A living serialization failure used to prove pre-record conversion.
        struct Refuses;
        impl Serialize for Refuses {
            /// Return the scripted conversion error without yielding JSON.
            fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("refused"))
            }
        }
        assert!(RunRecordEventValue::from_serializable(&Refuses).is_err());
    }
}
