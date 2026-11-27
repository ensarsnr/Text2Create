//! Domain payload migrations and domain validation rules, on the example
//! fixture (domain `example` version 1). Format migrations are tested with a
//! synthetic older format in `src/migrate.rs`, because format 1 is the first.

mod common;

use common::{Assembly, EXAMPLE, ExampleDomain, FixedClock, SequentialIds, read};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use text2create_ir::domain::{
    DomainCodec, DomainError, DomainRegistry, EntityPayload, LinkRule, NormalizedPayload,
    normalize_typed,
};
use text2create_ir::model::{Activity, Actor, ProjectDocument};
use text2create_ir::scalar::Text;
use text2create_ir::{
    DOMAIN_MIGRATION_PREFIX, FieldPath, LoadError, MigrateError, Migrated, Project, Rule, WriteEnv,
    WriteErrorKind,
};

/// `example.widget` in version 2: `size` was renamed to `size_mm`.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WidgetV2 {
    label: Text<64>,
    size_mm: u32,
    tolerance: Option<text2create_ir::scalar::Decimal>,
}

impl EntityPayload for WidgetV2 {}

/// How the version 1 to 2 migration behaves, to test failures.
#[derive(Clone, Copy, PartialEq)]
enum Behavior {
    Correct,
    /// Rejects the payload of the second widget.
    Fails,
    /// Produces a payload that version 2 does not accept.
    Invalid,
    /// Defines no migration.
    Missing,
}

struct ExampleV2(Behavior);

impl DomainCodec for ExampleV2 {
    fn id(&self) -> &'static str {
        "example"
    }
    fn version(&self) -> u32 {
        2
    }
    fn entity_kinds(&self) -> &'static [&'static str] {
        ExampleDomain.entity_kinds()
    }
    fn artifact_kinds(&self) -> &'static [&'static str] {
        ExampleDomain.artifact_kinds()
    }
    fn action_kinds(&self) -> &'static [&'static str] {
        ExampleDomain.action_kinds()
    }
    fn check_kinds(&self) -> &'static [&'static str] {
        ExampleDomain.check_kinds()
    }
    fn link_rules(&self) -> &'static [LinkRule] {
        ExampleDomain.link_rules()
    }
    fn normalize_entity(&self, kind: &str, data: &Value) -> Result<NormalizedPayload, DomainError> {
        match kind {
            "widget" => normalize_typed::<WidgetV2>(data),
            "assembly" => normalize_typed::<Assembly>(data),
            other => Err(DomainError::UnknownKind(other.to_owned())),
        }
    }
    fn migrate_entity(&self, kind: &str, from: u32, mut data: Value) -> Result<Value, DomainError> {
        if self.0 == Behavior::Missing || from != 1 {
            return Err(DomainError::NoMigration { from });
        }
        if kind != "widget" {
            return Ok(data);
        }
        if self.0 == Behavior::Fails && data["label"] == "Right bracket" {
            return Err(DomainError::invalid_at(
                FieldPath::root().key("label"),
                "cannot migrate this widget",
            ));
        }
        let object = data
            .as_object_mut()
            .ok_or(DomainError::invalid("not an object"))?;
        let size = object.remove("size").ok_or(DomainError::invalid_at(
            FieldPath::root().key("size"),
            "missing size",
        ))?;
        if self.0 != Behavior::Invalid {
            object.insert("size_mm".to_owned(), size);
        }
        Ok(data)
    }
}

fn registry_v2(behavior: Behavior) -> DomainRegistry {
    DomainRegistry::new().register(ExampleV2(behavior)).unwrap()
}

fn migrate(bytes: &[u8], registry: &DomainRegistry) -> Result<Migrated, MigrateError> {
    let mut env = WriteEnv {
        registry,
        clock: &mut FixedClock(0),
        ids: &mut SequentialIds(0),
    };
    Project::migrate(bytes, &mut env)
}

#[test]
fn an_older_domain_version_is_migrated_with_provenance() {
    let original = read(EXAMPLE);
    let registry = registry_v2(Behavior::Correct);
    let error = Project::load(&original, &registry).unwrap_err();
    assert!(error.needs_migration(), "{error}");
    assert_eq!(error.path().as_str(), "/domains/0/version");

    let migrated = migrate(&original, &registry).unwrap();
    let before: ProjectDocument = common::load(&original).unwrap().document().clone();
    let after = migrated.project.document();
    assert_eq!(after.domains[0].version, 2);

    // Payloads have the new structure.
    assert_eq!(
        after.entities[0].data.as_value(),
        &json!({"label": "Left bracket", "size_mm": 12, "tolerance": "0.05"})
    );
    assert_eq!(
        after.entities[2].data.as_value(),
        before.entities[2].data.as_value()
    );

    // One provenance record for the step; all previous records are kept.
    assert_eq!(migrated.steps.len(), 1);
    let step = &migrated.steps[0];
    assert_eq!(
        (step.domain.as_ref().unwrap().as_str(), step.from, step.to),
        ("example", 1, 2)
    );
    assert!(
        before
            .provenance
            .iter()
            .all(|p| after.provenance.contains(p))
    );
    assert_eq!(after.provenance.len(), before.provenance.len() + 1);
    let record = after
        .provenance
        .iter()
        .find(|p| p.id == step.provenance)
        .unwrap();
    assert_eq!(
        record.actor,
        Actor::System {
            component: Text::try_from(format!("{DOMAIN_MIGRATION_PREFIX}example")).unwrap()
        }
    );
    assert_eq!(record.activity, Activity::Migration { from: 1, to: 2 });

    // Changed records point to the migration; unchanged ones keep their history.
    for index in [0, 1] {
        let (old, new) = (
            &before.entities[index].provenance,
            &after.entities[index].provenance,
        );
        assert_eq!(
            (new.rev, new.created, new.modified),
            (old.rev + 1, old.created, step.provenance)
        );
    }
    assert_eq!(after.entities[2].provenance, before.entities[2].provenance);
    assert_eq!(after.project.provenance.modified, step.provenance);
    assert_eq!(after.requirements, before.requirements);

    // The result passes the complete loading pipeline of the new version only.
    let canonical = migrated.project.to_canonical_json();
    assert_eq!(
        Project::load(canonical.as_bytes(), &registry).unwrap(),
        migrated.project
    );
    assert!(common::load(canonical.as_bytes()).is_err());
}

#[test]
fn domain_migration_is_deterministic() {
    let registry = registry_v2(Behavior::Correct);
    let first = migrate(&read(EXAMPLE), &registry).unwrap();
    let second = migrate(&read(EXAMPLE), &registry).unwrap();
    assert_eq!(
        first.project.to_canonical_json(),
        second.project.to_canonical_json()
    );
    assert_eq!(first.steps, second.steps);
}

#[test]
fn failing_migrations_report_the_payload_and_change_nothing() {
    let original = read(EXAMPLE);
    let error = migrate(&original, &registry_v2(Behavior::Fails)).unwrap_err();
    assert!(
        matches!(&error, MigrateError::Domain { domain, from: 1, .. } if domain == "example"),
        "{error}"
    );
    assert_eq!(error.path().as_str(), "/entities/1/data/label");

    // Output that the new version rejects is caught by its codec.
    let error = migrate(&original, &registry_v2(Behavior::Invalid)).unwrap_err();
    assert!(matches!(error, MigrateError::Domain { .. }), "{error}");
    // A missing field is reported at the object that lacks it.
    assert_eq!(error.path().as_str(), "/entities/0/data");
    assert!(
        error.to_string().contains("missing field `size_mm`"),
        "{error}"
    );

    let error = migrate(&original, &registry_v2(Behavior::Missing)).unwrap_err();
    assert!(
        matches!(
            error,
            MigrateError::Domain {
                error: DomainError::NoMigration { from: 1 },
                ..
            }
        ),
        "{error}"
    );
    // The input is untouched and still loads with version 1.
    assert_eq!(original, read(EXAMPLE));
    common::load(&original).unwrap();
}

#[test]
fn newer_unknown_and_current_versions_are_not_migrated() {
    let mut value = common::example_value();
    value["domains"][0]["version"] = json!(3);
    let newer = serde_json::to_vec_pretty(&value).unwrap();
    assert!(matches!(
        migrate(&newer, &registry_v2(Behavior::Correct)),
        Err(MigrateError::Load(LoadError::NewerDomain {
            found: 3,
            supported: 2,
            ..
        }))
    ));
    assert!(matches!(
        migrate(&read(EXAMPLE), &DomainRegistry::new()),
        Err(MigrateError::Load(LoadError::UnknownDomain { .. }))
    ));
    assert!(matches!(
        migrate(&read(EXAMPLE), &common::registry()),
        Err(MigrateError::UpToDate)
    ));
    // A current but invalid document reports why it does not load.
    value["domains"][0]["version"] = json!(1);
    value["project"]["name"] = json!(" padded");
    let invalid = serde_json::to_vec_pretty(&value).unwrap();
    let error = migrate(&invalid, &common::registry()).unwrap_err();
    assert!(
        matches!(error, MigrateError::Load(LoadError::Schema { .. })),
        "{error}"
    );
    assert_eq!(error.path().as_str(), "/project/name");
}

// ------------------------------------------------------------- domain rules

/// The example domain with a project-wide rule: at most two widgets.
struct LimitedWidgets;

impl DomainCodec for LimitedWidgets {
    fn id(&self) -> &'static str {
        "example"
    }
    fn version(&self) -> u32 {
        1
    }
    fn entity_kinds(&self) -> &'static [&'static str] {
        ExampleDomain.entity_kinds()
    }
    fn artifact_kinds(&self) -> &'static [&'static str] {
        ExampleDomain.artifact_kinds()
    }
    fn action_kinds(&self) -> &'static [&'static str] {
        ExampleDomain.action_kinds()
    }
    fn check_kinds(&self) -> &'static [&'static str] {
        ExampleDomain.check_kinds()
    }
    fn link_rules(&self) -> &'static [LinkRule] {
        ExampleDomain.link_rules()
    }
    fn normalize_entity(&self, kind: &str, data: &Value) -> Result<NormalizedPayload, DomainError> {
        ExampleDomain.normalize_entity(kind, data)
    }
    fn check_project(&self, document: &ProjectDocument) -> Result<(), DomainError> {
        let widgets: Vec<usize> = (0..document.entities.len())
            .filter(|&i| document.entities[i].kind.as_str() == "example.widget")
            .collect();
        match widgets.get(2) {
            Some(&index) => Err(DomainError::invalid_at(
                FieldPath::root().key("entities").index(index),
                "a project has at most two widgets",
            )),
            None => Ok(()),
        }
    }
}

#[test]
fn domains_can_add_project_rules_for_loading_and_writing() {
    let registry = DomainRegistry::new().register(LimitedWidgets).unwrap();
    let project = Project::load(&read(EXAMPLE), &registry).unwrap();

    let mut value = common::example_value();
    let mut third = value["entities"][1].clone();
    third["id"] = json!(common::id("ent", 24));
    value["entities"].as_array_mut().unwrap().push(third);
    let error = Project::load(&serde_json::to_vec(&value).unwrap(), &registry).unwrap_err();
    match &error {
        LoadError::Invariant(e) => assert_eq!(e.rule, Rule::DomainRule),
        other => panic!("unexpected error: {other}"),
    }
    assert_eq!(error.path().as_str(), "/entities/3");
    assert!(error.to_string().contains("at most two widgets"), "{error}");

    // The write path applies the same rule.
    let mut project = project;
    let changes = common::changes(json!([{"op": "add_entity", "kind": "example.widget",
        "name": "Third", "data": {"label": "Third", "size": 1, "tolerance": null}}]));
    let mut env = WriteEnv {
        registry: &registry,
        clock: &mut FixedClock(0),
        ids: &mut SequentialIds(0),
    };
    let before = project.clone();
    let error = project
        .apply(&common::model(), &changes, &mut env)
        .unwrap_err();
    assert!(
        matches!(&error.kind, WriteErrorKind::Invariant(e) if e.rule == Rule::DomainRule),
        "{error}"
    );
    assert_eq!(project, before);
}
