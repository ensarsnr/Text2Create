//! The loading pipeline and canonical serialization.

use crate::domain::{CORE_DOMAIN, DomainError, DomainRegistry};
use crate::ids::{EntityId, RecordRef};
use crate::invariants::{self, InvariantError, KindUse, Rule};
use crate::model::{FORMAT_SCHEMA, FORMAT_VERSION, ProjectDocument, ValidationResult};
use crate::path::{FieldPath, ROOT};
use crate::strict::{self, StrictJsonError, at};
use serde_json::Value;

/// Why a document could not be loaded. Loading never modifies its input.
///
/// [`LoadError::path`] gives the location of the invalid data as a JSON
/// Pointer, and the `Display` text is a message that can be shown to the user.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// Size limit or strict JSON parsing.
    #[error(transparent)]
    Json(#[from] StrictJsonError),
    #[error("not a Project IR document: {}{reason}", at(path))]
    NotProjectIr {
        path: FieldPath,
        reason: &'static str,
    },
    #[error("format version {found} is newer than the supported version {supported}")]
    NewerFormat { found: u64, supported: u32 },
    /// The document must be migrated with [`Project::migrate`] first.
    #[error(
        "format version {found} is older than the supported version {supported}; the project must be migrated"
    )]
    OlderFormat { found: u64, supported: u32 },
    /// `index` is the position in `domains`.
    #[error("domain `{id}` is not available in this build")]
    UnknownDomain { index: usize, id: String },
    #[error("domain `{id}` version {found} is newer than the supported version {supported}")]
    NewerDomain {
        index: usize,
        id: String,
        found: u64,
        supported: u32,
    },
    /// The document must be migrated with [`Project::migrate`] first.
    #[error(
        "domain `{id}` version {found} is older than the supported version {supported}; the project must be migrated"
    )]
    OlderDomain {
        index: usize,
        id: String,
        found: u64,
        supported: u32,
    },
    /// Typed parsing failed: a missing or unknown field, or an invalid value.
    /// The message includes the line and column.
    #[error("schema violation {}{error}", at(path))]
    Schema {
        path: FieldPath,
        #[source]
        error: serde_json::Error,
    },
    /// A domain codec rejected the payload of the entity at `index`.
    #[error("entity `{entity}` {}{}", at(&self.path()), payload_message(error))]
    Payload {
        index: usize,
        entity: EntityId,
        error: DomainError,
    },
    #[error(transparent)]
    Invariant(#[from] InvariantError),
}

/// The message of a payload error, without the payload-relative path,
/// which [`LoadError::path`] reports relative to the document instead.
fn payload_message(error: &DomainError) -> String {
    match error {
        DomainError::Invalid { message, .. } => format!("invalid payload: {message}"),
        other => other.to_string(),
    }
}

impl LoadError {
    /// The location of the invalid data, as a JSON Pointer into the document.
    pub fn path(&self) -> FieldPath {
        let root = FieldPath::root();
        match self {
            Self::Json(StrictJsonError::Json { path, .. }) => path.clone(),
            Self::Json(StrictJsonError::TooLarge { .. }) => root,
            Self::NotProjectIr { path, .. } | Self::Schema { path, .. } => path.clone(),
            Self::NewerFormat { .. } | Self::OlderFormat { .. } => {
                root.key("format").key("version")
            }
            Self::UnknownDomain { index, .. } => root.key("domains").index(*index).key("id"),
            Self::NewerDomain { index, .. } | Self::OlderDomain { index, .. } => {
                root.key("domains").index(*index).key("version")
            }
            Self::Payload { index, error, .. } => {
                let entity = root.key("entities").index(*index);
                match error {
                    DomainError::UnknownKind(_) => entity.key("kind"),
                    DomainError::Invalid { path, .. } => entity.key("data").join(path),
                    DomainError::NoMigration { .. } => entity.key("data"),
                }
            }
            Self::Invariant(error) => error.path.clone(),
        }
    }

    /// Whether the document was written with an older format or domain
    /// version. Such a document can only be opened after [`Project::migrate`].
    pub fn needs_migration(&self) -> bool {
        matches!(self, Self::OlderFormat { .. } | Self::OlderDomain { .. })
    }
}

/// A Project IR document that passed every check of the loading pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub(crate) document: ProjectDocument,
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
        let document: ProjectDocument = crate::path::from_slice(bytes)
            .map_err(|(path, error)| LoadError::Schema { path, error })?;
        let payload_references = normalize_payloads(&document, registry)?;
        invariants::check(&document, registry, &payload_references)?;
        Ok(Self { document })
    }

    /// The validated document.
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
    let format_path = FieldPath::root().key("format");
    let format = raw.get("format").ok_or(LoadError::NotProjectIr {
        path: format_path.clone(),
        reason: "missing `format`",
    })?;
    if format.get("schema").and_then(Value::as_str) != Some(FORMAT_SCHEMA) {
        return Err(LoadError::NotProjectIr {
            path: format_path.key("schema"),
            reason: "unexpected `format.schema`",
        });
    }
    let found = format
        .get("version")
        .and_then(Value::as_u64)
        .ok_or(LoadError::NotProjectIr {
            path: format_path.key("version"),
            reason: "invalid `format.version`",
        })?;
    let supported = FORMAT_VERSION;
    if found > u64::from(supported) {
        return Err(LoadError::NewerFormat { found, supported });
    }
    if found < u64::from(supported) {
        return Err(LoadError::OlderFormat { found, supported });
    }

    let entries = raw.get("domains").and_then(Value::as_array);
    for (index, entry) in entries.into_iter().flatten().enumerate() {
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
        let codec = registry.get(id).ok_or_else(|| LoadError::UnknownDomain {
            index,
            id: id.to_owned(),
        })?;
        let supported = codec.version();
        if found > u64::from(supported) {
            return Err(LoadError::NewerDomain {
                index,
                id: id.to_owned(),
                found,
                supported,
            });
        }
        if found < u64::from(supported) {
            return Err(LoadError::OlderDomain {
                index,
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
pub(crate) fn normalize_payloads(
    document: &ProjectDocument,
    registry: &DomainRegistry,
) -> Result<Vec<(EntityId, Vec<RecordRef>)>, LoadError> {
    let mut references = Vec::with_capacity(document.entities.len());
    let entities = ROOT.key("entities");
    for (index, entity) in document.entities.iter().enumerate() {
        let at = entities.index(index);
        let codec = invariants::resolve_kind(
            &entity.kind,
            KindUse::Entity,
            document,
            registry,
            &at.key("kind"),
        )?;
        let normalized = codec
            .normalize_entity(entity.kind.name(), entity.data.as_value())
            .map_err(|error| LoadError::Payload {
                index,
                entity: entity.id,
                error,
            })?;
        if normalized.value != *entity.data.as_value() {
            return Err(InvariantError {
                rule: Rule::CanonicalPayload,
                path: at.key("data").field_path(),
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
