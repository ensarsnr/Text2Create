//! Typed record identifiers.
//!
//! An identifier is serialized as a type prefix, an underscore, and a UUID in
//! lowercase hyphenated form, for example `req_01a21c90-0000-7000-8000-000000000011`.
//! Each record type has its own Rust type, so identifiers cannot be mixed up.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// An identifier string is malformed or has the wrong prefix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid identifier `{0}`")]
pub struct IdError(pub String);

fn parse_prefixed(prefix: &str, s: &str) -> Result<Uuid, IdError> {
    let invalid = || IdError(s.to_owned());
    let rest = s
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('_'))
        .ok_or_else(invalid)?;
    let uuid = Uuid::parse_str(rest).map_err(|_| invalid())?;
    // Only the canonical text form is accepted: lowercase and hyphenated.
    if uuid.hyphenated().to_string() != rest {
        return Err(invalid());
    }
    Ok(uuid)
}

macro_rules! ids {
    ($($(#[$doc:meta])* $name:ident = $prefix:literal;)*) => {$(
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Uuid);

        impl $name {
            /// The serialized type prefix.
            pub const PREFIX: &'static str = $prefix;

            /// Wraps a UUID. New identifiers are allocated by the application core.
            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            /// The UUID without the type prefix.
            pub fn uuid(&self) -> Uuid {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, self.0.hyphenated())
            }
        }

        impl FromStr for $name {
            type Err = IdError;
            fn from_str(s: &str) -> Result<Self, IdError> {
                parse_prefixed($prefix, s).map(Self)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer)?
                    .parse()
                    .map_err(serde::de::Error::custom)
            }
        }
    )*};
}

ids! {
    /// Project identifier (`prj`).
    ProjectId = "prj";
    /// Provenance record identifier (`prv`).
    ProvenanceId = "prv";
    /// Requirement identifier (`req`).
    RequirementId = "req";
    /// Entity identifier (`ent`).
    EntityId = "ent";
    /// Artifact identifier (`art`).
    ArtifactId = "art";
    /// Link identifier (`lnk`).
    LinkId = "lnk";
    /// Plan identifier (`pln`).
    PlanId = "pln";
    /// Task identifier (`tsk`).
    TaskId = "tsk";
    /// Validation result identifier (`val`).
    ValidationId = "val";
    /// Task run identifier (`run`). Task runs are stored outside the IR (#27),
    /// so these references are not resolved in v1.
    TaskRunId = "run";
}

/// The type of a record that can be referenced by a [`RecordRef`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordType {
    Requirement,
    Entity,
    Artifact,
    Plan,
    Task,
    Validation,
}

/// Reference to a record. Serialized as the prefixed identifier; the prefix
/// selects the record type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordRef {
    Requirement(RequirementId),
    Entity(EntityId),
    Artifact(ArtifactId),
    Plan(PlanId),
    Task(TaskId),
    Validation(ValidationId),
}

impl RecordRef {
    /// The type of the referenced record.
    pub fn record_type(&self) -> RecordType {
        match self {
            Self::Requirement(_) => RecordType::Requirement,
            Self::Entity(_) => RecordType::Entity,
            Self::Artifact(_) => RecordType::Artifact,
            Self::Plan(_) => RecordType::Plan,
            Self::Task(_) => RecordType::Task,
            Self::Validation(_) => RecordType::Validation,
        }
    }
}

impl fmt::Display for RecordRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Requirement(id) => id.fmt(f),
            Self::Entity(id) => id.fmt(f),
            Self::Artifact(id) => id.fmt(f),
            Self::Plan(id) => id.fmt(f),
            Self::Task(id) => id.fmt(f),
            Self::Validation(id) => id.fmt(f),
        }
    }
}

impl FromStr for RecordRef {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Self, IdError> {
        match s.split_once('_').map(|(prefix, _)| prefix) {
            Some(RequirementId::PREFIX) => s.parse().map(Self::Requirement),
            Some(EntityId::PREFIX) => s.parse().map(Self::Entity),
            Some(ArtifactId::PREFIX) => s.parse().map(Self::Artifact),
            Some(PlanId::PREFIX) => s.parse().map(Self::Plan),
            Some(TaskId::PREFIX) => s.parse().map(Self::Task),
            Some(ValidationId::PREFIX) => s.parse().map(Self::Validation),
            _ => Err(IdError(s.to_owned())),
        }
    }
}

impl Serialize for RecordRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for RecordRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "01a21c90-0000-7000-8000-000000000011";

    #[test]
    fn round_trip_and_display() {
        let id: RequirementId = format!("req_{UUID}").parse().unwrap();
        assert_eq!(id.to_string(), format!("req_{UUID}"));
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"req_{UUID}\""));
        assert_eq!(serde_json::from_str::<RequirementId>(&json).unwrap(), id);
    }

    #[test]
    fn accepts_any_uuid_version_in_canonical_form() {
        assert!(
            "ent_550e8400-e29b-41d4-a716-446655440000"
                .parse::<EntityId>()
                .is_ok()
        );
    }

    #[test]
    fn rejects_non_canonical_or_mistyped_identifiers() {
        for bad in [
            format!("ent_{UUID}"),                    // wrong prefix
            format!("req{UUID}"),                     // missing underscore
            format!("req_{}", UUID.to_uppercase()),   // uppercase
            format!("req_{}", UUID.replace('-', "")), // simple form
            format!("req_{{{UUID}}}"),                // braced form
            format!("req_urn:uuid:{UUID}"),           // URN form
            "req_".to_owned(),
            format!(" req_{UUID}"),
        ] {
            assert!(bad.parse::<RequirementId>().is_err(), "{bad}");
        }
    }

    #[test]
    fn record_ref_selects_type_by_prefix() {
        let r: RecordRef = format!("art_{UUID}").parse().unwrap();
        assert_eq!(r.record_type(), RecordType::Artifact);
        assert_eq!(r.to_string(), format!("art_{UUID}"));
        // Provenance records, links, projects, and task runs are not linkable records.
        for prefix in ["prv", "lnk", "prj", "run", "xyz"] {
            assert!(format!("{prefix}_{UUID}").parse::<RecordRef>().is_err());
        }
    }
}
