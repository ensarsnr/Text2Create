//! Migrations of documents written with an older format or domain version.
//!
//! A migration runs on the strictly parsed JSON value, because an older
//! document does not match the current Rust types. Format steps run first,
//! then the payload migrations of each domain. Every step is recorded as a
//! provenance record of the system, and the result must pass the complete
//! loading pipeline. The input bytes are never modified.

use crate::domain::{CORE_DOMAIN, DomainCodec, DomainError};
use crate::ids::ProvenanceId;
use crate::model::{FORMAT_SCHEMA, FORMAT_VERSION, ProjectDocument};
use crate::path::FieldPath;
use crate::project::{LoadError, Project};
use crate::scalar::{DomainId, Text};
use crate::source::Source;
use crate::strict::{self, at};
use crate::write::{WriteEnv, bump, provenance_record, sort};
use serde_json::Value;
use std::collections::BTreeSet;

/// The `system` component recorded for format migrations.
pub const FORMAT_MIGRATION_COMPONENT: &str = "project-ir";

/// Prefix of the `system` component recorded for domain migrations; the
/// domain identifier follows, for example `domain:electronics`.
pub const DOMAIN_MIGRATION_PREFIX: &str = "domain:";

/// A format migration step: upgrades a document from version `from` to
/// `from + 1`. The step does not change `format.version`; the caller does.
pub(crate) struct FormatStep {
    pub from: u32,
    pub upgrade: fn(&mut Value) -> Result<(), StepError>,
}

/// Why a format step failed, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StepError {
    pub path: FieldPath,
    pub message: String,
}

/// The format migrations of this build. Format version 1 is the first one,
/// so there are none yet; the framework is tested with synthetic steps.
const FORMAT_STEPS: &[FormatStep] = &[];

/// Why a document could not be migrated. The input is never modified.
#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    /// The input was rejected before any migration ran: it is not a Project
    /// IR document, or it uses a newer or unknown format or domain.
    #[error(transparent)]
    Load(LoadError),
    /// The document already uses the current format and domain versions.
    #[error("the project already uses the current format and domain versions")]
    UpToDate,
    #[error("no migration from format version {from} is available")]
    NoFormatMigration { from: u64 },
    #[error(
        "the format migration from version {from} failed {}{message}",
        at(path)
    )]
    Format {
        from: u32,
        path: FieldPath,
        message: String,
    },
    /// A domain codec could not migrate a payload. `path` is the payload.
    #[error("the migration of domain `{domain}` from version {from} failed at `{path}`: {error}")]
    Domain {
        domain: String,
        from: u32,
        path: FieldPath,
        error: DomainError,
    },
    /// The migrated document does not pass the loading pipeline.
    #[error("the migrated project is invalid: {0}")]
    Invalid(LoadError),
}

impl MigrateError {
    /// The location of the problem, as a JSON Pointer into the document
    /// (before migration for [`MigrateError::Load`], after it otherwise).
    pub fn path(&self) -> FieldPath {
        match self {
            Self::Load(error) | Self::Invalid(error) => error.path(),
            Self::UpToDate => FieldPath::root(),
            Self::NoFormatMigration { .. } => FieldPath::root().key("format").key("version"),
            Self::Format { path, .. } => path.clone(),
            Self::Domain { path, error, .. } => match error {
                DomainError::Invalid { path: inner, .. } => path.join(inner),
                _ => path.clone(),
            },
        }
    }
}

/// One migration step that was applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MigrationStep {
    /// `None` for a format step, or the migrated domain.
    pub domain: Option<DomainId>,
    pub from: u32,
    pub to: u32,
    /// The provenance record created for the step.
    pub provenance: ProvenanceId,
}

/// A migrated project and the steps that produced it.
#[derive(Clone, Debug)]
pub struct Migrated {
    pub project: Project,
    /// In the order they were applied.
    pub steps: Vec<MigrationStep>,
}

impl Project {
    /// Migrates a document written with an older format or domain version
    /// and validates the result with the complete loading pipeline.
    ///
    /// Use it when [`Project::load`] fails with an error for which
    /// [`LoadError::needs_migration`] holds. Each step creates a provenance
    /// record of the `system` actor with a `migration` activity. Records whose
    /// data a step changed get that record as `modified`, and the project
    /// record gets the last one. Documents from a newer or unknown version
    /// are rejected. The result depends only on the input, the registry, and
    /// the clock and identifier generator.
    pub fn migrate(bytes: &[u8], env: &mut WriteEnv<'_>) -> Result<Migrated, MigrateError> {
        migrate_with(bytes, env, FORMAT_STEPS)
    }
}

/// A step before its provenance record is allocated.
struct Planned {
    domain: Option<DomainId>,
    from: u32,
    /// Entities whose payload this step changed, by position.
    changed: Vec<usize>,
}

pub(crate) fn migrate_with(
    bytes: &[u8],
    env: &mut WriteEnv<'_>,
    format_steps: &[FormatStep],
) -> Result<Migrated, MigrateError> {
    let mut value = strict::parse(bytes).map_err(|e| MigrateError::Load(e.into()))?;
    let mut planned = Vec::new();

    // Format steps.
    let mut version = format_version(&value)?;
    while version < u64::from(FORMAT_VERSION) {
        let from = version as u32;
        let step = format_steps
            .iter()
            .find(|step| step.from == from)
            .ok_or(MigrateError::NoFormatMigration { from: version })?;
        (step.upgrade)(&mut value).map_err(|e| MigrateError::Format {
            from,
            path: e.path,
            message: e.message,
        })?;
        version += 1;
        value["format"]["version"] = Value::from(version);
        planned.push(Planned {
            domain: None,
            from,
            changed: Vec::new(),
        });
    }

    // Domain payload migrations, on a document in the current format.
    let domains = domain_versions(&value, env)?;
    for (index, id, found, codec) in domains {
        let supported = codec.version();
        if found == supported {
            continue;
        }
        let mut changed = Vec::new();
        for from in found..supported {
            let step = migrate_domain(&mut value, codec, from)?;
            changed.extend(step.iter().copied());
            planned.push(Planned {
                domain: Some(id.clone()),
                from,
                changed: step,
            });
        }
        normalize_domain(&mut value, codec, &changed, supported)?;
        value["domains"][index]["version"] = Value::from(supported);
    }

    if planned.is_empty() {
        // Report why a current document does not load, if it does not.
        Project::load(bytes, env.registry).map_err(MigrateError::Load)?;
        return Err(MigrateError::UpToDate);
    }

    let mut document: ProjectDocument = crate::path::from_value(&value)
        .map_err(|(path, error)| MigrateError::Invalid(LoadError::Schema { path, error }))?;
    let mut steps = Vec::with_capacity(planned.len());
    for step in planned {
        let to = step.from + 1;
        let component = match &step.domain {
            None => FORMAT_MIGRATION_COMPONENT.to_owned(),
            Some(id) => format!("{DOMAIN_MIGRATION_PREFIX}{}", id.as_str()),
        };
        let component = Text::try_from(component).expect("component names are valid text");
        let source = Source::migration(component, step.from, to);
        let id = ProvenanceId::from_uuid(env.ids.next_uuid());
        document
            .provenance
            .push(provenance_record(id, &source, Vec::new(), env.clock));
        for index in step.changed {
            bump(&mut document.entities[index].provenance, id);
        }
        bump(&mut document.project.provenance, id);
        steps.push(MigrationStep {
            domain: step.domain,
            from: step.from,
            to,
            provenance: id,
        });
    }
    sort(&mut document);

    // The complete loading pipeline, on the bytes that will be stored.
    let project = Project { document };
    let project = Project::load(project.to_canonical_json().as_bytes(), env.registry)
        .map_err(MigrateError::Invalid)?;
    Ok(Migrated { project, steps })
}

fn format_version(value: &Value) -> Result<u64, MigrateError> {
    let not_ir =
        |path: FieldPath, reason| MigrateError::Load(LoadError::NotProjectIr { path, reason });
    let format = FieldPath::root().key("format");
    let header = value
        .get("format")
        .ok_or_else(|| not_ir(format.clone(), "missing `format`"))?;
    if header.get("schema").and_then(Value::as_str) != Some(FORMAT_SCHEMA) {
        return Err(not_ir(format.key("schema"), "unexpected `format.schema`"));
    }
    let found = header
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| not_ir(format.key("version"), "invalid `format.version`"))?;
    if found > u64::from(FORMAT_VERSION) {
        return Err(MigrateError::Load(LoadError::NewerFormat {
            found,
            supported: FORMAT_VERSION,
        }));
    }
    Ok(found)
}

/// A listed domain: its position in `domains`, stored version, and codec.
type ListedDomain<'r> = (usize, DomainId, u32, &'r dyn DomainCodec);

/// The listed domains. Rejects unknown domains and newer versions. Malformed
/// entries are left to the final load.
fn domain_versions<'r>(
    value: &Value,
    env: &WriteEnv<'r>,
) -> Result<Vec<ListedDomain<'r>>, MigrateError> {
    let mut result = Vec::new();
    let entries = value.get("domains").and_then(Value::as_array);
    for (index, entry) in entries.into_iter().flatten().enumerate() {
        let (Some(id), Some(found)) = (
            entry.get("id").and_then(Value::as_str),
            entry.get("version").and_then(Value::as_u64),
        ) else {
            continue;
        };
        if id == CORE_DOMAIN {
            continue;
        }
        let codec = env.registry.get(id).ok_or_else(|| {
            MigrateError::Load(LoadError::UnknownDomain {
                index,
                id: id.to_owned(),
            })
        })?;
        let supported = codec.version();
        if found > u64::from(supported) {
            return Err(MigrateError::Load(LoadError::NewerDomain {
                index,
                id: id.to_owned(),
                found,
                supported,
            }));
        }
        let Ok(domain) = DomainId::try_from(id.to_owned()) else {
            continue;
        };
        result.push((index, domain, found as u32, codec));
    }
    Ok(result)
}

/// The entities of `codec`'s domain: position and kind name.
fn domain_entities(value: &Value, codec: &dyn DomainCodec) -> Vec<(usize, String)> {
    let prefix = format!("{}.", codec.id());
    let entities = value.get("entities").and_then(Value::as_array);
    entities
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, entity)| {
            let kind = entity.get("kind").and_then(Value::as_str)?;
            let name = kind.strip_prefix(&prefix)?;
            entity.get("data")?;
            Some((index, name.to_owned()))
        })
        .collect()
}

fn payload_path(index: usize) -> FieldPath {
    FieldPath::root().key("entities").index(index).key("data")
}

/// Runs one version step of a domain on every payload of the domain. Returns
/// the positions of the entities whose payload changed.
fn migrate_domain(
    value: &mut Value,
    codec: &dyn DomainCodec,
    from: u32,
) -> Result<Vec<usize>, MigrateError> {
    let mut changed = Vec::new();
    for (index, name) in domain_entities(value, codec) {
        let data = &mut value["entities"][index]["data"];
        let before = data.clone();
        let after = codec
            .migrate_entity(&name, from, data.take())
            .map_err(|error| MigrateError::Domain {
                domain: codec.id().to_owned(),
                from,
                path: payload_path(index),
                error,
            })?;
        if after != before {
            changed.push(index);
        }
        *data = after;
    }
    Ok(changed)
}

/// Brings the migrated payloads into canonical form with the current codec,
/// which also validates them.
fn normalize_domain(
    value: &mut Value,
    codec: &dyn DomainCodec,
    changed: &[usize],
    version: u32,
) -> Result<(), MigrateError> {
    let changed: BTreeSet<usize> = changed.iter().copied().collect();
    for (index, name) in domain_entities(value, codec) {
        if !changed.contains(&index) {
            continue;
        }
        let data = &mut value["entities"][index]["data"];
        let normalized =
            codec
                .normalize_entity(&name, data)
                .map_err(|error| MigrateError::Domain {
                    domain: codec.id().to_owned(),
                    from: version - 1,
                    path: payload_path(index),
                    error,
                })?;
        *data = normalized.value;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DomainRegistry;
    use crate::scalar::Timestamp;
    use crate::source::{Clock, IdGenerator};

    /// Synthetic format 0: like format 1, but the project name is stored as
    /// `title`. The step renames it.
    fn rename_title(value: &mut Value) -> Result<(), StepError> {
        let project = value
            .get_mut("project")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| StepError {
                path: FieldPath::root().key("project"),
                message: "missing project".to_owned(),
            })?;
        let title = project.remove("title").ok_or_else(|| StepError {
            path: FieldPath::root().key("project").key("title"),
            message: "missing title".to_owned(),
        })?;
        project.insert("name".to_owned(), title);
        Ok(())
    }

    const STEPS: &[FormatStep] = &[FormatStep {
        from: 0,
        upgrade: rename_title,
    }];

    const V0: &str = include_str!("../tests/fixtures/synthetic-format-v0.json");

    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&mut self) -> Timestamp {
            Timestamp::try_from("2026-11-28T09:00:00.000Z".to_owned()).unwrap()
        }
    }

    struct Ids(u128);
    impl IdGenerator for Ids {
        fn next_uuid(&mut self) -> uuid::Uuid {
            self.0 += 1;
            uuid::Uuid::from_u128(0x0193_ffff_0000_7000_8000_0000_0000_0000 + self.0)
        }
    }

    fn run(bytes: &[u8], steps: &[FormatStep]) -> Result<Migrated, MigrateError> {
        let registry = DomainRegistry::new();
        let mut env = WriteEnv {
            registry: &registry,
            clock: &mut FixedClock,
            ids: &mut Ids(0),
        };
        migrate_with(bytes, &mut env, steps)
    }

    #[test]
    fn a_synthetic_older_format_is_migrated_and_validated() {
        // The current build refuses the fixture until it is migrated.
        let error = Project::load(V0.as_bytes(), &DomainRegistry::new()).unwrap_err();
        assert!(error.needs_migration(), "{error}");
        let migrated = run(V0.as_bytes(), STEPS).unwrap();
        let document = migrated.project.document();
        assert_eq!(document.format.version, 1);
        assert_eq!(
            document.project.name.as_str(),
            "ESP32 climate fan controller"
        );
        // Provenance is kept and the migration is recorded.
        let before: Value = serde_json::from_str(V0).unwrap();
        assert_eq!(
            document.provenance.len(),
            before["provenance"].as_array().unwrap().len() + 1
        );
        let step = &migrated.steps[0];
        assert_eq!((step.domain.as_ref(), step.from, step.to), (None, 0, 1));
        let record = document
            .provenance
            .iter()
            .find(|p| p.id == step.provenance)
            .unwrap();
        assert_eq!(
            record.actor,
            crate::model::Actor::System {
                component: Text::try_from(FORMAT_MIGRATION_COMPONENT.to_owned()).unwrap()
            }
        );
        assert_eq!(
            record.activity,
            crate::model::Activity::Migration { from: 0, to: 1 }
        );
        assert_eq!(document.project.provenance.modified, step.provenance);
        assert_eq!(document.project.provenance.rev, 2);
        // The result is a valid current document.
        let canonical = migrated.project.to_canonical_json();
        assert_eq!(
            Project::load(canonical.as_bytes(), &DomainRegistry::new()).unwrap(),
            migrated.project
        );
    }

    #[test]
    fn migration_is_deterministic() {
        let first = run(V0.as_bytes(), STEPS).unwrap();
        let second = run(V0.as_bytes(), STEPS).unwrap();
        assert_eq!(
            first.project.to_canonical_json(),
            second.project.to_canonical_json()
        );
        assert_eq!(first.steps, second.steps);
    }

    #[test]
    fn missing_or_failing_steps_are_reported() {
        assert!(matches!(
            run(V0.as_bytes(), &[]),
            Err(MigrateError::NoFormatMigration { from: 0 })
        ));
        let broken = V0.replace("\"title\":", "\"heading\":");
        let error = run(broken.as_bytes(), STEPS).unwrap_err();
        assert!(
            matches!(error, MigrateError::Format { from: 0, .. }),
            "{error}"
        );
        assert_eq!(error.path().as_str(), "/project/title");
        // A step whose output is invalid is caught by the final load.
        fn drop_project(value: &mut Value) -> Result<(), StepError> {
            value.as_object_mut().unwrap().remove("project");
            Ok(())
        }
        let error = run(
            V0.as_bytes(),
            &[FormatStep {
                from: 0,
                upgrade: drop_project,
            }],
        )
        .unwrap_err();
        assert!(
            matches!(error, MigrateError::Invalid(LoadError::Schema { .. })),
            "{error}"
        );
    }

    #[test]
    fn current_newer_and_foreign_documents_are_not_migrated() {
        let current = run(V0.as_bytes(), STEPS)
            .unwrap()
            .project
            .to_canonical_json();
        assert!(matches!(
            run(current.as_bytes(), STEPS),
            Err(MigrateError::UpToDate)
        ));
        let newer = current.replacen("\"version\": 1", "\"version\": 2", 1);
        assert!(matches!(
            run(newer.as_bytes(), STEPS),
            Err(MigrateError::Load(LoadError::NewerFormat { found: 2, .. }))
        ));
        assert!(matches!(
            run(
                b"{\"format\": {\"schema\": \"other\", \"version\": 0}}",
                STEPS
            ),
            Err(MigrateError::Load(LoadError::NotProjectIr { .. }))
        ));
        // Strict parsing applies before migration.
        let repeated = V0.replacen("\"title\":", "\"title\": \"A\", \"title\":", 1);
        assert!(matches!(
            run(repeated.as_bytes(), STEPS),
            Err(MigrateError::Load(LoadError::Json(_)))
        ));
    }
}
