use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn from_u128(value: u128) -> Self {
                Self(Uuid::from_u128(value))
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }
    };
}

uuid_id!(EntityId);
uuid_id!(ClientId);
uuid_id!(RequestId);
uuid_id!(OperationId);
uuid_id!(RuntimeEpochId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("{kind} revision overflow at {value}")]
pub struct RevisionOverflow {
    pub kind: &'static str,
    pub value: u64,
}

macro_rules! revision {
    ($name:ident, $kind:literal) => {
        #[derive(
            Debug,
            Default,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub const ZERO: Self = Self(0);

            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }

            pub fn checked_next(self) -> Result<Self, RevisionOverflow> {
                self.0.checked_add(1).map(Self).ok_or(RevisionOverflow {
                    kind: $kind,
                    value: self.0,
                })
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

revision!(DocumentRevision, "document");
revision!(TargetGraphRevision, "target graph");
revision!(ActiveGraphRevision, "active graph");
revision!(ProjectionRevision, "projection");
revision!(TranscriptRevision, "transcript");

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NameError {
    #[error("{kind} must not be blank")]
    Blank { kind: &'static str },
}

macro_rules! named_id {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, NameError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(NameError::Blank { kind: $kind });
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            #[must_use]
            pub fn is_blank(&self) -> bool {
                self.0.trim().is_empty()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl FromStr for $name {
            type Err = NameError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

named_id!(ModuleTypeId, "module type ID");
named_id!(PortId, "port ID");
named_id!(StreamTypeId, "stream type ID");
named_id!(ControlId, "control ID");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_roles_are_serialized_but_not_interchangeable() {
        let entity = EntityId::from_u128(1);
        assert_eq!(entity.to_string(), "00000000-0000-0000-0000-000000000001");
        assert_eq!(
            serde_json::to_string(&entity).unwrap(),
            format!("\"{entity}\"")
        );
    }

    #[test]
    fn revisions_fail_instead_of_wrapping() {
        let error = DocumentRevision::new(u64::MAX).checked_next().unwrap_err();
        assert_eq!(error.kind, "document");
        assert_eq!(error.value, u64::MAX);
    }

    #[test]
    fn named_ids_reject_blank_values() {
        assert!(matches!(
            PortId::new("  "),
            Err(NameError::Blank { kind: "port ID" })
        ));
    }
}
