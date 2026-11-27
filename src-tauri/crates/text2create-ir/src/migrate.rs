//! Migrations of documents written with an older format or domain version.
//!
//! A migration runs on the strictly parsed JSON value, because an older
//! document does not match the current Rust types. Format steps run first,
//! then the payload migrations of each domain. Every step is recorded as a
//! provenance record of the system, and the result must pass the complete
//! loading pipeline. The input bytes are never modified.

use crate::domain::{CORE_DOMAIN, DomainCodec, DomainError, DomainRegistry};
use crate::ids::ProvenanceId;
use crate::model::{FORMAT_SCHEMA, FORMAT_VERSION, ProjectDocument, RecordProvenance};
use crate::path::FieldPath;
use crate::project::{LoadError, Project, domain_message};
use crate::scalar::{DomainId, Text};
use crate::source::Source;
use crate::strict::{self, at};
use crate::write::{WriteEnv, bump, provenance_record, sort};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

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
    #[error("{}the project already uses the current format and domain versions", at(&self.path()))]
    UpToDate,
    #[error("{}no migration from format version {from} is available", at(&self.path()))]
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
    /// A domain codec could not migrate a payload. `path` is the payload;
    /// [`MigrateError::path`] adds the location inside it.
    #[error(
        "{}the migration of domain `{domain}` from version {from} failed: {}",
        at(&self.path()),
        domain_message(error)
    )]
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

/// A migration step that [`Project::migrate`] would apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingStep {
    /// `None` for a format step, or the domain to migrate.
    pub domain: Option<DomainId>,
    pub from: u32,
    pub to: u32,
}

impl MigrationStep {
    /// The step without its provenance record, for comparison with a plan.
    pub fn pending(&self) -> PendingStep {
        PendingStep {
            domain: self.domain.clone(),
            from: self.from,
            to: self.to,
        }
    }
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

    /// The steps that [`Project::migrate`] would apply to `bytes`, without
    /// running them. A migration changes the stored project permanently, so
    /// the caller shows these steps to the user for approval first (ADR-0011).
    /// Fails like [`Project::migrate`] for newer, unknown, or current versions.
    pub fn pending_migrations(
        bytes: &[u8],
        registry: &DomainRegistry,
    ) -> Result<Vec<PendingStep>, MigrateError> {
        pending_with(bytes, registry, FORMAT_STEPS)
    }
}

pub(crate) fn pending_with(
    bytes: &[u8],
    registry: &DomainRegistry,
    format_steps: &[FormatStep],
) -> Result<Vec<PendingStep>, MigrateError> {
    let value = strict::parse(bytes).map_err(|e| MigrateError::Load(e.into()))?;
    let mut pending = Vec::new();
    let found = format_version(&value)?;
    for version in found..u64::from(FORMAT_VERSION) {
        let from = version as u32;
        if !format_steps.iter().any(|step| step.from == from) {
            return Err(MigrateError::NoFormatMigration { from: version });
        }
        pending.push(PendingStep {
            domain: None,
            from,
            to: from + 1,
        });
    }
    for (_, id, found, codec) in domain_versions(&value, registry)? {
        for from in found..codec.version() {
            pending.push(PendingStep {
                domain: Some(id.clone()),
                from,
                to: from + 1,
            });
        }
    }
    if pending.is_empty() {
        Project::load(bytes, registry).map_err(MigrateError::Load)?;
        return Err(MigrateError::UpToDate);
    }
    Ok(pending)
}

/// A step before its provenance record is allocated.
struct Planned {
    domain: Option<DomainId>,
    from: u32,
    /// Identifiers of the records whose data this step changed.
    changed: Vec<String>,
}

/// Collections whose records carry a `provenance` object with `modified`.
/// Changes to a task are changes to its plan. Validation results are
/// immutable records of a tool run, so they keep the tool's provenance.
const TRACKED_COLLECTIONS: &[&str] = &["requirements", "entities", "artifacts", "links", "plans"];

/// The identifiers of the records that differ between `before` and `after`,
/// including records that are new in `after`.
fn changed_records(before: &Value, after: &Value) -> Vec<String> {
    let mut changed = Vec::new();
    for collection in TRACKED_COLLECTIONS {
        let old: BTreeMap<&str, &Value> = before
            .get(collection)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|record| Some((record.get("id")?.as_str()?, record)))
            .collect();
        let new = after.get(collection).and_then(Value::as_array);
        for record in new.into_iter().flatten() {
            let Some(id) = record.get("id").and_then(Value::as_str) else {
                continue;
            };
            if old.get(id) != Some(&record) {
                changed.push(id.to_owned());
            }
        }
    }
    changed
}

/// The provenance of the record with identifier `id`, if it exists.
fn provenance_of<'d>(
    document: &'d mut ProjectDocument,
    id: &str,
) -> Option<&'d mut RecordProvenance> {
    let matches = |record: &dyn std::fmt::Display| record.to_string() == id;
    if let Some(r) = document.requirements.iter_mut().find(|r| matches(&r.id)) {
        return Some(&mut r.provenance);
    }
    if let Some(r) = document.entities.iter_mut().find(|r| matches(&r.id)) {
        return Some(&mut r.provenance);
    }
    if let Some(r) = document.artifacts.iter_mut().find(|r| matches(&r.id)) {
        return Some(&mut r.provenance);
    }
    if let Some(r) = document.links.iter_mut().find(|r| matches(&r.id)) {
        return Some(&mut r.provenance);
    }
    document
        .plans
        .iter_mut()
        .find(|r| matches(&r.id))
        .map(|r| &mut r.provenance)
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
        let before = value.clone();
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
            changed: changed_records(&before, &value),
        });
    }

    // Domain payload migrations, on a document in the current format.
    let domains = domain_versions(&value, env.registry)?;
    for (index, id, found, codec) in domains {
        let supported = codec.version();
        if found == supported {
            continue;
        }
        let mut changed = Vec::new();
        for from in found..supported {
            let step = migrate_domain(&mut value, codec, from)?;
            changed.extend(step.iter().copied());
            let ids = step
                .iter()
                .filter_map(|&i| value["entities"][i]["id"].as_str().map(str::to_owned))
                .collect();
            planned.push(Planned {
                domain: Some(id.clone()),
                from,
                changed: ids,
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
        // A record removed by a later step is no longer there to update.
        for record in &step.changed {
            if let Some(provenance) = provenance_of(&mut document, record) {
                bump(provenance, id);
            }
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
    registry: &'r DomainRegistry,
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
        let codec = registry.get(id).ok_or_else(|| {
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
    /// `title` and entity names as `label`. The step renames them.
    fn rename_title(value: &mut Value) -> Result<(), StepError> {
        let entities = value.get_mut("entities").and_then(Value::as_array_mut);
        for entity in entities.into_iter().flatten() {
            let entity = entity.as_object_mut().expect("entities are objects");
            if let Some(label) = entity.remove("label") {
                entity.insert("name".to_owned(), label);
            }
        }
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
    /// Format 0 with domain `sample` version 1 and two entities.
    const V0_ENTITIES: &str = include_str!("../tests/fixtures/synthetic-format-v0-entities.json");

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct PartV1 {
        size: u32,
    }
    impl crate::domain::EntityPayload for PartV1 {}

    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct PartV2 {
        size_mm: u32,
    }
    impl crate::domain::EntityPayload for PartV2 {}

    /// Domain `sample`: version 2 renamed `size` to `size_mm`.
    struct Sample {
        version: u32,
        fail: bool,
    }

    impl DomainCodec for Sample {
        fn id(&self) -> &'static str {
            "sample"
        }
        fn version(&self) -> u32 {
            self.version
        }
        fn entity_kinds(&self) -> &'static [&'static str] {
            &["part"]
        }
        fn normalize_entity(
            &self,
            _kind: &str,
            data: &Value,
        ) -> Result<crate::domain::NormalizedPayload, DomainError> {
            match self.version {
                1 => crate::domain::normalize_typed::<PartV1>(data),
                _ => crate::domain::normalize_typed::<PartV2>(data),
            }
        }
        fn migrate_entity(
            &self,
            _kind: &str,
            _from: u32,
            data: Value,
        ) -> Result<Value, DomainError> {
            if self.fail {
                return Err(DomainError::invalid_at(
                    FieldPath::root().key("size"),
                    "cannot convert",
                ));
            }
            Ok(serde_json::json!({"size_mm": data["size"]}))
        }
    }

    fn sample(version: u32, fail: bool) -> DomainRegistry {
        DomainRegistry::new()
            .register(Sample { version, fail })
            .unwrap()
    }

    fn run_with(
        bytes: &[u8],
        registry: &DomainRegistry,
        steps: &[FormatStep],
    ) -> Result<Migrated, MigrateError> {
        let mut env = WriteEnv {
            registry,
            clock: &mut FixedClock,
            ids: &mut Ids(0),
        };
        migrate_with(bytes, &mut env, steps)
    }

    /// The provenance of every record with a `provenance` object, by ID.
    fn provenance_by_id(
        document: &ProjectDocument,
    ) -> BTreeMap<String, crate::model::RecordProvenance> {
        let mut map = BTreeMap::new();
        map.insert(
            document.project.id.to_string(),
            document.project.provenance.clone(),
        );
        for r in &document.requirements {
            map.insert(r.id.to_string(), r.provenance.clone());
        }
        for r in &document.entities {
            map.insert(r.id.to_string(), r.provenance.clone());
        }
        for r in &document.artifacts {
            map.insert(r.id.to_string(), r.provenance.clone());
        }
        for r in &document.links {
            map.insert(r.id.to_string(), r.provenance.clone());
        }
        for r in &document.plans {
            map.insert(r.id.to_string(), r.provenance.clone());
        }
        map
    }

    /// The record provenance of the format 0 fixture, as written.
    fn stored_provenance(text: &str) -> BTreeMap<String, crate::model::RecordProvenance> {
        let value: Value = serde_json::from_str(text).unwrap();
        let mut map = BTreeMap::new();
        let mut add = |record: &Value| {
            map.insert(
                record["id"].as_str().unwrap().to_owned(),
                serde_json::from_value(record["provenance"].clone()).unwrap(),
            );
        };
        add(&value["project"]);
        for collection in TRACKED_COLLECTIONS {
            value[collection]
                .as_array()
                .unwrap()
                .iter()
                .for_each(&mut add);
        }
        map
    }

    /// Asserts that exactly the records in `changed` (and the project record)
    /// point to `step` as their last modification, with `rev` increased by
    /// one, and that all other records are unchanged.
    #[track_caller]
    fn assert_bumped(before: &str, migrated: &Migrated, changed: &[&str], step: ProvenanceId) {
        let before = stored_provenance(before);
        let after = provenance_by_id(migrated.project.document());
        let project = migrated.project.document().project.id.to_string();
        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>()
        );
        for (id, old) in &before {
            let new = &after[id];
            if changed.contains(&id.as_str()) || *id == project {
                assert_eq!(
                    (new.rev, new.created, new.modified),
                    (old.rev + 1, old.created, step),
                    "{id} was changed by the step"
                );
            } else {
                assert_eq!(new, old, "{id} was not changed by the step");
            }
        }
    }

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

    const FAN: &str = "ent_01a21c90-0000-7000-8000-000000000021";
    const SENSOR: &str = "ent_01a21c90-0000-7000-8000-000000000022";

    #[test]
    fn a_format_step_updates_the_provenance_of_the_records_it_changes() {
        let registry = sample(1, false);
        let migrated = run_with(V0_ENTITIES.as_bytes(), &registry, STEPS).unwrap();
        let step = migrated.steps[0].provenance;
        assert_eq!(migrated.project.document().entities[0].name.as_str(), "Fan");
        assert_bumped(V0_ENTITIES, &migrated, &[FAN, SENSOR], step);

        // Only the entity whose stored data differs is changed.
        let partly = V0_ENTITIES.replacen("\"label\": \"Sensor\"", "\"name\": \"Sensor\"", 1);
        let migrated = run_with(partly.as_bytes(), &registry, STEPS).unwrap();
        assert_bumped(&partly, &migrated, &[FAN], migrated.steps[0].provenance);
    }

    #[test]
    fn format_and_domain_steps_each_record_their_own_changes() {
        // A format step that changes a requirement and the payload of one entity.
        fn edit(value: &mut Value) -> Result<(), StepError> {
            rename_title(value)?;
            value["requirements"][1]["title"] = Value::from("Renamed by the format");
            value["entities"][1]["data"]["size"] = Value::from(5);
            Ok(())
        }
        let steps = [FormatStep {
            from: 0,
            upgrade: edit,
        }];
        let migrated = run_with(
            V0_ENTITIES
                .replacen("\"label\": \"Fan\"", "\"name\": \"Fan\"", 1)
                .replacen("\"label\": \"Sensor\"", "\"name\": \"Sensor\"", 1)
                .as_bytes(),
            &sample(2, false),
            &steps,
        )
        .unwrap();
        let [format, domain] = &migrated.steps[..] else {
            panic!("expected two steps: {:?}", migrated.steps);
        };
        assert_eq!(
            (
                format.domain.as_ref(),
                domain.domain.as_ref().unwrap().as_str()
            ),
            (None, "sample")
        );
        let document = migrated.project.document();
        // Only the requirement the format step edited points to it.
        let before = stored_provenance(V0_ENTITIES);
        let (edited, untouched) = (&document.requirements[1], &document.requirements[0]);
        let old = &before[&edited.id.to_string()];
        assert_eq!(
            (edited.provenance.rev, edited.provenance.modified),
            (old.rev + 1, format.provenance)
        );
        assert_eq!(untouched.provenance, before[&untouched.id.to_string()]);
        // The sensor was changed by both steps, the fan only by the domain step.
        let (fan, sensor) = (
            &document.entities[0].provenance,
            &document.entities[1].provenance,
        );
        assert_eq!((fan.rev, fan.modified), (3, domain.provenance));
        assert_eq!((sensor.rev, sensor.modified), (3, domain.provenance));
        assert_eq!(
            document.entities[1].data.as_value(),
            &serde_json::json!({"size_mm": 5})
        );
        assert_eq!(document.project.provenance.modified, domain.provenance);
        assert_eq!(document.project.provenance.rev, 3);
    }

    #[test]
    fn pending_steps_are_listed_without_migrating() {
        assert_eq!(
            pending_with(V0.as_bytes(), &DomainRegistry::new(), STEPS).unwrap(),
            vec![PendingStep {
                domain: None,
                from: 0,
                to: 1
            }]
        );
        let pending = pending_with(V0_ENTITIES.as_bytes(), &sample(3, false), STEPS).unwrap();
        let sample_id = DomainId::try_from("sample".to_owned()).unwrap();
        assert_eq!(
            pending,
            vec![
                PendingStep {
                    domain: None,
                    from: 0,
                    to: 1
                },
                PendingStep {
                    domain: Some(sample_id.clone()),
                    from: 1,
                    to: 2
                },
                PendingStep {
                    domain: Some(sample_id),
                    from: 2,
                    to: 3
                },
            ]
        );
        // The plan matches the steps a migration applies.
        let migrated = run_with(V0_ENTITIES.as_bytes(), &sample(2, false), STEPS).unwrap();
        assert_eq!(
            migrated
                .steps
                .iter()
                .map(MigrationStep::pending)
                .collect::<Vec<_>>(),
            pending_with(V0_ENTITIES.as_bytes(), &sample(2, false), STEPS).unwrap()
        );
        assert!(matches!(
            pending_with(V0.as_bytes(), &DomainRegistry::new(), &[]),
            Err(MigrateError::NoFormatMigration { from: 0 })
        ));
        let current = migrated.project.to_canonical_json();
        assert!(matches!(
            pending_with(current.as_bytes(), &sample(2, false), STEPS),
            Err(MigrateError::UpToDate)
        ));
        assert!(matches!(
            pending_with(current.as_bytes(), &sample(1, false), STEPS),
            Err(MigrateError::Load(LoadError::NewerDomain { .. }))
        ));
    }

    /// Asserts that the message of `error` contains its path.
    #[track_caller]
    fn assert_located(error: &MigrateError, path: &str) {
        assert_eq!(error.path().as_str(), path, "{error}");
        let message = error.to_string();
        let expected = if path.is_empty() {
            "at the document root".to_owned()
        } else {
            format!("at `{path}`")
        };
        assert!(
            message.contains(&expected),
            "missing `{expected}` in: {message}"
        );
    }

    #[test]
    fn every_migration_error_names_its_location() {
        let registry = DomainRegistry::new();
        let current = run(V0.as_bytes(), STEPS)
            .unwrap()
            .project
            .to_canonical_json();
        assert_located(&run(current.as_bytes(), STEPS).unwrap_err(), "");
        assert_located(&run(V0.as_bytes(), &[]).unwrap_err(), "/format/version");
        let broken = V0.replace("\"title\":", "\"heading\":");
        assert_located(
            &run(broken.as_bytes(), STEPS).unwrap_err(),
            "/project/title",
        );
        let newer = current.replacen("\"version\": 1", "\"version\": 2", 1);
        assert_located(
            &run(newer.as_bytes(), STEPS).unwrap_err(),
            "/format/version",
        );
        let error = run_with(V0_ENTITIES.as_bytes(), &sample(2, true), STEPS).unwrap_err();
        assert!(matches!(error, MigrateError::Domain { .. }), "{error}");
        assert_located(&error, "/entities/0/data/size");
        let error = run_with(V0_ENTITIES.as_bytes(), &registry, STEPS).unwrap_err();
        assert!(
            matches!(error, MigrateError::Load(LoadError::UnknownDomain { .. })),
            "{error}"
        );
        assert_located(&error, "/domains/0/id");
        fn invalid(value: &mut Value) -> Result<(), StepError> {
            rename_title(value)?;
            value["project"]["name"] = Value::from(" padded");
            Ok(())
        }
        let error = run(
            V0.as_bytes(),
            &[FormatStep {
                from: 0,
                upgrade: invalid,
            }],
        )
        .unwrap_err();
        assert!(matches!(error, MigrateError::Invalid(_)), "{error}");
        assert_located(&error, "/project/name");
    }
}
