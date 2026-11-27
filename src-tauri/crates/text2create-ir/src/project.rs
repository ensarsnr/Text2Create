//! The loading pipeline and canonical serialization.

use crate::domain::{CORE_DOMAIN, DomainError, DomainRegistry};
use crate::ids::{EntityId, RecordRef};
use crate::invariants::{self, InvariantError, KindUse, Rule};
use crate::model::{FORMAT_SCHEMA, FORMAT_VERSION, ProjectDocument, ValidationResult};
use crate::strict::{self, StrictJsonError};
use serde_json::Value;

/// Why a document could not be loaded. Loading never modifies its input.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// Size limit or strict JSON parsing.
    #[error(transparent)]
    Json(#[from] StrictJsonError),
    #[error("not a Project IR document: {0}")]
    NotProjectIr(&'static str),
    #[error("format version {found} is newer than the supported version {supported}")]
    NewerFormat { found: u64, supported: u32 },
    #[error(
        "format version {found} is older than the supported version {supported} and needs a migration, which is not available yet"
    )]
    OlderFormat { found: u64, supported: u32 },
    #[error("domain `{0}` is not available in this build")]
    UnknownDomain(String),
    #[error("domain `{id}` version {found} is newer than the supported version {supported}")]
    NewerDomain {
        id: String,
        found: u64,
        supported: u32,
    },
    #[error(
        "domain `{id}` version {found} is older than the supported version {supported} and needs a migration, which is not available yet"
    )]
    OlderDomain {
        id: String,
        found: u64,
        supported: u32,
    },
    /// Typed parsing failed. The message includes the line and column.
    #[error("schema violation: {0}")]
    Schema(serde_json::Error),
    /// A domain codec rejected an entity payload.
    #[error("entity `{entity}`: {error}")]
    Payload {
        entity: EntityId,
        error: DomainError,
    },
    #[error(transparent)]
    Invariant(#[from] InvariantError),
}

/// A Project IR document that passed every check of the loading pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    document: ProjectDocument,
}

impl Project {
    /// Loads and validates a document from untrusted bytes.
    ///
    /// Pipeline: size limit and strict JSON parsing, version checks, typed
    /// parsing, payload normalization, invariant checks.
    pub fn load(bytes: &[u8], registry: &DomainRegistry) -> Result<Self, LoadError> {
        let raw = strict::parse(bytes)?;
        check_versions(&raw, registry)?;
        drop(raw);
        // Parse the original bytes again so that errors report a line and column.
        let document: ProjectDocument = serde_json::from_slice(bytes).map_err(LoadError::Schema)?;
        let payload_references = normalize_payloads(&document, registry)?;
        invariants::check(&document, registry, &payload_references)?;
        Ok(Self { document })
    }

    pub fn document(&self) -> &ProjectDocument {
        &self.document
    }

    /// The canonical serialization: UTF-8, two-space indentation, LF line
    /// endings, and one trailing newline. Identical state produces identical bytes.
    pub fn to_canonical_json(&self) -> String {
        let mut json = serde_json::to_string_pretty(&self.document)
            .expect("Project IR types always serialize to JSON");
        json.push('\n');
        json
    }

    /// Whether `validation` is stale: any subject digest differs from the
    /// artifact's current content digest. Staleness is computed, never stored.
    pub fn is_stale(&self, validation: &ValidationResult) -> bool {
        validation.subjects.iter().any(|subject| {
            let current = self
                .document
                .artifacts
                .iter()
                .find(|a| a.id == subject.artifact)
                .and_then(|a| a.content.as_ref())
                .map(|content| &content.digest);
            current != Some(&subject.digest)
        })
    }
}

/// Checks `format` and the domain versions on the raw value, before typed
/// parsing, so that a document from a newer version gets a precise error.
/// Malformed entries are left to typed parsing.
fn check_versions(raw: &Value, registry: &DomainRegistry) -> Result<(), LoadError> {
    let format = raw
        .get("format")
        .ok_or(LoadError::NotProjectIr("missing `format`"))?;
    if format.get("schema").and_then(Value::as_str) != Some(FORMAT_SCHEMA) {
        return Err(LoadError::NotProjectIr("unexpected `format.schema`"));
    }
    let found = format
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(LoadError::NotProjectIr("invalid `format.version`"))?;
    let supported = FORMAT_VERSION;
    if found > u64::from(supported) {
        return Err(LoadError::NewerFormat { found, supported });
    }
    if found < u64::from(supported) {
        return Err(LoadError::OlderFormat { found, supported });
    }

    let entries = raw.get("domains").and_then(Value::as_array);
    for entry in entries.into_iter().flatten() {
        let (Some(id), Some(found)) = (
            entry.get("id").and_then(Value::as_str),
            entry.get("version").and_then(Value::as_u64),
        ) else {
            continue;
        };
        // Listing `core` is an invariant violation, reported after typed parsing.
        if id == CORE_DOMAIN {
            continue;
        }
        let codec = registry
            .get(id)
            .ok_or_else(|| LoadError::UnknownDomain(id.to_owned()))?;
        let supported = codec.version();
        if found > u64::from(supported) {
            return Err(LoadError::NewerDomain {
                id: id.to_owned(),
                found,
                supported,
            });
        }
        if found < u64::from(supported) {
            return Err(LoadError::OlderDomain {
                id: id.to_owned(),
                found,
                supported,
            });
        }
    }
    Ok(())
}

/// Processes every entity payload with its domain codec. The re-encoded
/// payload must equal the stored payload (rule 11). Returns the record IDs
/// each payload contains, which are resolved with the other invariants.
fn normalize_payloads(
    document: &ProjectDocument,
    registry: &DomainRegistry,
) -> Result<Vec<(EntityId, Vec<RecordRef>)>, LoadError> {
    let mut references = Vec::with_capacity(document.entities.len());
    for entity in &document.entities {
        let codec = invariants::resolve_kind(&entity.kind, KindUse::Entity, document, registry)?;
        let normalized = codec
            .normalize_entity(entity.kind.name(), entity.data.as_value())
            .map_err(|error| LoadError::Payload {
                entity: entity.id,
                error,
            })?;
        if normalized.value != *entity.data.as_value() {
            return Err(InvariantError {
                rule: Rule::CanonicalPayload,
                message: format!(
                    "the payload of entity `{}` is not in canonical form",
                    entity.id
                ),
            }
            .into());
        }
        references.push((entity.id, normalized.references));
    }
    Ok(references)
}
