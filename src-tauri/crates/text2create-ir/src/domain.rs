//! Compiled-in, versioned domain codecs.
//!
//! A domain declares the kinds it defines and, for each entity kind, a typed
//! payload. The core treats payloads as opaque until the codec has decoded,
//! validated, and re-encoded them (see [`normalize_typed`]).

use crate::ids::{RecordRef, RecordType};
use crate::scalar::{check_domain_id, check_kind_name};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;

/// Identifier of the built-in domain.
pub const CORE_DOMAIN: &str = "core";

/// A payload was rejected by its domain codec.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    /// The codec does not define this entity kind.
    #[error("unknown entity kind `{0}`")]
    UnknownKind(String),
    /// The payload does not match the typed payload or fails its validation.
    #[error("invalid payload: {0}")]
    Invalid(String),
}

/// A codec could not be registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("domain `{0}` is already registered")]
    Duplicate(&'static str),
    #[error("domain id `{0}` is invalid or reserved")]
    InvalidId(&'static str),
    #[error("domain `{0}` has version 0; versions start at 1")]
    InvalidVersion(&'static str),
    #[error("domain `{domain}` declares an invalid kind name `{name}`")]
    InvalidKindName {
        domain: &'static str,
        name: &'static str,
    },
}

/// A link kind and the record types allowed at each end.
#[derive(Debug, Clone, Copy)]
pub struct LinkRule {
    /// Kind name without the domain prefix.
    pub name: &'static str,
    pub from: &'static [RecordType],
    pub to: &'static [RecordType],
}

/// The result of processing a payload: its canonical form and the record IDs
/// it contains. The core checks that every reported ID exists.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedPayload {
    pub value: Value,
    pub references: Vec<RecordRef>,
}

/// A domain codec, compiled into the application and registered at startup.
///
/// Kind names are given without the domain prefix, for example `component`
/// for `electronics.component`.
pub trait DomainCodec: Send + Sync {
    /// Domain identifier, for example `electronics`.
    fn id(&self) -> &'static str;
    /// Schema version of the domain's payloads. Starts at 1.
    fn version(&self) -> u32;
    fn entity_kinds(&self) -> &'static [&'static str] {
        &[]
    }
    fn artifact_kinds(&self) -> &'static [&'static str] {
        &[]
    }
    fn action_kinds(&self) -> &'static [&'static str] {
        &[]
    }
    fn check_kinds(&self) -> &'static [&'static str] {
        &[]
    }
    fn link_rules(&self) -> &'static [LinkRule] {
        &[]
    }
    /// Decodes the payload of an entity of kind `kind` into its typed payload,
    /// validates it, re-encodes it, and reports the record IDs it contains.
    /// Implementations should use [`normalize_typed`].
    fn normalize_entity(&self, kind: &str, data: &Value) -> Result<NormalizedPayload, DomainError> {
        let _ = data;
        Err(DomainError::UnknownKind(kind.to_owned()))
    }
}

/// A typed entity payload. Payload types should reject unknown fields.
pub trait EntityPayload: Serialize + DeserializeOwned {
    /// Checks rules that the type alone cannot express.
    fn validate(&self) -> Result<(), DomainError> {
        Ok(())
    }
    /// The record IDs contained in the payload.
    fn references(&self) -> Vec<RecordRef> {
        Vec::new()
    }
}

/// Decodes `data` into `T`, validates it, and re-encodes it.
pub fn normalize_typed<T: EntityPayload>(data: &Value) -> Result<NormalizedPayload, DomainError> {
    let typed = T::deserialize(data).map_err(|e| DomainError::Invalid(e.to_string()))?;
    typed.validate()?;
    let value = serde_json::to_value(&typed).map_err(|e| DomainError::Invalid(e.to_string()))?;
    Ok(NormalizedPayload {
        references: typed.references(),
        value,
    })
}

/// The built-in `core` domain. Its version is the format version, so it is
/// never listed in `domains`.
struct CoreDomain;

impl DomainCodec for CoreDomain {
    fn id(&self) -> &'static str {
        CORE_DOMAIN
    }

    fn version(&self) -> u32 {
        crate::model::FORMAT_VERSION
    }

    fn artifact_kinds(&self) -> &'static [&'static str] {
        &["document"]
    }

    fn link_rules(&self) -> &'static [LinkRule] {
        use RecordType::{Artifact, Entity, Requirement};
        &[
            LinkRule {
                name: "satisfies",
                from: &[Entity, Artifact],
                to: &[Requirement],
            },
            LinkRule {
                name: "derived_from",
                from: &[Requirement, Entity, Artifact],
                to: &[Requirement, Entity, Artifact],
            },
            LinkRule {
                name: "describes",
                from: &[Artifact],
                to: &[Entity],
            },
            LinkRule {
                name: "refines",
                from: &[Requirement],
                to: &[Requirement],
            },
        ]
    }
}

/// The domains available in this build. Always contains `core`.
pub struct DomainRegistry {
    domains: BTreeMap<&'static str, Box<dyn DomainCodec>>,
}

impl Default for DomainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DomainRegistry {
    /// A registry with only the built-in `core` domain.
    pub fn new() -> Self {
        let mut domains: BTreeMap<&'static str, Box<dyn DomainCodec>> = BTreeMap::new();
        domains.insert(CORE_DOMAIN, Box::new(CoreDomain));
        Self { domains }
    }

    /// Registers a domain codec after checking its identifier, version, and
    /// kind names.
    pub fn register(mut self, codec: impl DomainCodec + 'static) -> Result<Self, RegistryError> {
        let id = codec.id();
        if id == CORE_DOMAIN || check_domain_id(id).is_err() {
            return Err(RegistryError::InvalidId(id));
        }
        if self.domains.contains_key(id) {
            return Err(RegistryError::Duplicate(id));
        }
        if codec.version() == 0 {
            return Err(RegistryError::InvalidVersion(id));
        }
        let names = [
            codec.entity_kinds(),
            codec.artifact_kinds(),
            codec.action_kinds(),
            codec.check_kinds(),
        ]
        .into_iter()
        .flatten()
        .copied()
        .chain(codec.link_rules().iter().map(|rule| rule.name));
        for name in names {
            if check_kind_name(name).is_err() {
                return Err(RegistryError::InvalidKindName { domain: id, name });
            }
        }
        self.domains.insert(id, Box::new(codec));
        Ok(self)
    }

    pub fn get(&self, id: &str) -> Option<&dyn DomainCodec> {
        self.domains.get(id).map(|codec| codec.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Named(&'static str, u32, &'static [&'static str]);

    impl DomainCodec for Named {
        fn id(&self) -> &'static str {
            self.0
        }
        fn version(&self) -> u32 {
            self.1
        }
        fn entity_kinds(&self) -> &'static [&'static str] {
            self.2
        }
    }

    #[test]
    fn registry_contains_core_and_rejects_bad_codecs() {
        assert!(DomainRegistry::new().get(CORE_DOMAIN).is_some());
        let error = |codec| DomainRegistry::new().register(codec).err();
        assert_eq!(
            error(Named("core", 1, &[])),
            Some(RegistryError::InvalidId("core"))
        );
        assert_eq!(
            error(Named("Bad", 1, &[])),
            Some(RegistryError::InvalidId("Bad"))
        );
        assert_eq!(
            error(Named("a", 0, &[])),
            Some(RegistryError::InvalidVersion("a"))
        );
        assert_eq!(
            error(Named("a", 1, &["Thing"])),
            Some(RegistryError::InvalidKindName {
                domain: "a",
                name: "Thing"
            })
        );
        let twice = DomainRegistry::new()
            .register(Named("a", 1, &[]))
            .unwrap()
            .register(Named("a", 1, &[]));
        assert_eq!(twice.err(), Some(RegistryError::Duplicate("a")));
    }

    #[test]
    fn default_normalize_rejects_every_kind() {
        let codec = Named("a", 1, &["thing"]);
        assert_eq!(
            codec.normalize_entity("thing", &Value::Null),
            Err(DomainError::UnknownKind("thing".to_owned()))
        );
    }
}
