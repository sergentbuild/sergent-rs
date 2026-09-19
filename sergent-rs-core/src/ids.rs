//! Framework-owned bookkeeping identifiers. @sergent/docs/execution-model.md
//!
//! Every framework identifier is a lowercase prefix plus 32 lowercase hex
//! characters, for example `op_1f0c...`. The grammar is load
//! bearing across implementations because persisted application artifacts carry
//! checked identifiers that the other implementation must load.

use thiserror::Error;

/// Rejection of a value that is not a framework identifier.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdError {
    /// The value does not match `<prefix>_<32 lowercase hex>`.
    #[error("id must match '<prefix>_<32 lowercase hex>' but got {value:?}")]
    Malformed {
        /// The offending value.
        value: String,
    },
    /// The identifier prefix is not the one required for this identifier kind.
    #[error("id prefix must be {expected:?} but got {actual:?}")]
    WrongPrefix {
        /// The required prefix.
        expected: &'static str,
        /// The prefix that was found.
        actual: String,
    },
    /// A mint prefix that does not match `[a-z][a-z0-9]*`.
    #[error("id prefix {prefix:?} must match '[a-z][a-z0-9]*'")]
    BadPrefix {
        /// The rejected prefix.
        prefix: String,
    },
}

/// Return whether a prefix starts with a lowercase ASCII letter and continues
/// with lowercase ASCII letters or digits.
fn prefix_is_valid(prefix: &str) -> bool {
    let mut bytes = prefix.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_lowercase() => {}
        _ => return false,
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// Return whether an identifier body is exactly 32 lowercase hexadecimal
/// ASCII bytes.
fn body_is_hex32(body: &str) -> bool {
    body.len() == 32 && body.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Validate the grammar, and optionally require one prefix.
fn check_grammar(value: &str, required_prefix: Option<&'static str>) -> Result<(), IdError> {
    let (prefix, body) = value.rsplit_once('_').ok_or_else(|| IdError::Malformed {
        value: value.to_owned(),
    })?;
    if !prefix_is_valid(prefix) || !body_is_hex32(body) {
        return Err(IdError::Malformed {
            value: value.to_owned(),
        });
    }
    if let Some(expected) = required_prefix
        && prefix != expected
    {
        return Err(IdError::WrongPrefix {
            expected,
            actual: prefix.to_owned(),
        });
    }
    Ok(())
}

/// Join a supplied prefix to a fresh UUID v4 hexadecimal body.
fn mint(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

macro_rules! fixed_prefix_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Mint a fresh identifier with this kind's fixed prefix.
            pub fn mint() -> Self {
                Self(mint($prefix))
            }

            /// Admit a string that already carries a valid identifier of this kind.
            pub fn parse(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                check_grammar(&value, Some($prefix))?;
                Ok(Self(value))
            }

            /// Borrow the identifier text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;
            fn try_from(value: String) -> Result<Self, IdError> {
                Self::parse(value)
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }
    };
}

macro_rules! open_prefix_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Mint a fresh identifier with an application-chosen prefix.
            pub fn mint(prefix: &str) -> Result<Self, IdError> {
                if !prefix_is_valid(prefix) {
                    return Err(IdError::BadPrefix { prefix: prefix.to_owned() });
                }
                Ok(Self(mint(prefix)))
            }

            /// Admit a string that already carries a valid framework identifier.
            pub fn parse(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                check_grammar(&value, None)?;
                Ok(Self(value))
            }

            /// Borrow the identifier text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;
            fn try_from(value: String) -> Result<Self, IdError> {
                Self::parse(value)
            }
        }

        impl From<$name> for String {
            fn from(id: $name) -> String {
                id.0
            }
        }
    };
}

/// The framework-minted identity of one Operation, used for deterministic
/// Patch and trace matching. @sergent/docs/framework.md
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct OperationId(String);

impl OperationId {
    /// Mint a fresh Operation identity inside the framework decode owner.
    pub(crate) fn mint() -> Self {
        Self(mint("op"))
    }

    /// Borrow the identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for OperationId {
    /// Write the opaque Operation identifier without transformation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fixed_prefix_id!(
    /// The stable identity of one run, linking progress, Run Record, and committed
    /// changes. @sergent/docs/execution-model.md
    RunId,
    "run"
);

open_prefix_id!(
    /// The stable identity of one Scene. @sergent/docs/framework.md
    SceneId
);

open_prefix_id!(
    /// The stable identity carried by a selected Target. @sergent/docs/framework.md
    TargetId
);

impl From<SceneId> for TargetId {
    /// Reuse an owned Scene identity as the identity of its whole-Scene Target.
    fn from(scene_id: SceneId) -> Self {
        Self(scene_id.0)
    }
}

impl From<&SceneId> for TargetId {
    /// Clone a Scene identity as the identity of its whole-Scene Target.
    fn from(scene_id: &SceneId) -> Self {
        Self(scene_id.0.clone())
    }
}

impl PartialEq<SceneId> for TargetId {
    /// Compare the exact identifier text across the Target and Scene wrappers.
    fn eq(&self, other: &SceneId) -> bool {
        self.0 == other.0
    }
}

impl PartialEq<TargetId> for SceneId {
    /// Compare the exact identifier text across the Scene and Target wrappers.
    fn eq(&self, other: &TargetId) -> bool {
        self.0 == other.0
    }
}
