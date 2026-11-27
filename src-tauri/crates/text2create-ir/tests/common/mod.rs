//! Test-only example domain that demonstrates the domain codec interface, and
//! helpers shared by the integration tests.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use text2create_ir::domain::{
    DomainCodec, DomainError, DomainRegistry, EntityPayload, LinkRule, NormalizedPayload,
    normalize_typed,
};
use text2create_ir::ids::{EntityId, RecordRef, RecordType};
use text2create_ir::scalar::{Decimal, Text};
use text2create_ir::{FieldPath, InvariantError, LoadError, Project, Rule};

pub const ESP32: &str = "tests/fixtures/esp32-devkit-v1.json";
pub const EXAMPLE: &str = "tests/fixtures/example-domain.json";

/// `example.widget`: a part with an optional tolerance.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Widget {
    pub label: Text<64>,
    pub size: u32,
    pub tolerance: Option<Decimal>,
}

impl EntityPayload for Widget {}

/// `example.assembly`: references other entities.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assembly {
    pub parts: Vec<EntityId>,
    pub note: Option<Text<200>>,
}

impl EntityPayload for Assembly {
    fn validate(&self) -> Result<(), DomainError> {
        if self.parts.is_empty() {
            return Err(DomainError::invalid_at(
                FieldPath::root().key("parts"),
                "an assembly needs parts",
            ));
        }
        if !self.parts.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(DomainError::invalid_at(
                FieldPath::root().key("parts"),
                "parts must be sorted and unique",
            ));
        }
        Ok(())
    }

    fn references(&self) -> Vec<RecordRef> {
        self.parts.iter().copied().map(RecordRef::Entity).collect()
    }
}

pub struct ExampleDomain;

impl DomainCodec for ExampleDomain {
    fn id(&self) -> &'static str {
        "example"
    }

    fn version(&self) -> u32 {
        1
    }

    fn entity_kinds(&self) -> &'static [&'static str] {
        &["widget", "assembly"]
    }

    fn artifact_kinds(&self) -> &'static [&'static str] {
        &["report"]
    }

    fn action_kinds(&self) -> &'static [&'static str] {
        &["build", "inspect"]
    }

    fn check_kinds(&self) -> &'static [&'static str] {
        &["inspect"]
    }

    fn link_rules(&self) -> &'static [LinkRule] {
        &[LinkRule {
            name: "verifies",
            from: &[RecordType::Validation],
            to: &[RecordType::Requirement],
        }]
    }

    fn normalize_entity(&self, kind: &str, data: &Value) -> Result<NormalizedPayload, DomainError> {
        match kind {
            "widget" => normalize_typed::<Widget>(data),
            "assembly" => normalize_typed::<Assembly>(data),
            other => Err(DomainError::UnknownKind(other.to_owned())),
        }
    }
}

pub fn registry() -> DomainRegistry {
    DomainRegistry::new().register(ExampleDomain).unwrap()
}

pub fn read(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

pub fn load(bytes: &[u8]) -> Result<Project, LoadError> {
    Project::load(bytes, &registry())
}

/// The example fixture as a JSON value.
pub fn example_value() -> Value {
    serde_json::from_slice(&read(EXAMPLE)).unwrap()
}

/// Loads the example fixture after applying `change` to it.
pub fn mutate(change: impl FnOnce(&mut Value)) -> Result<Project, LoadError> {
    let mut value = example_value();
    change(&mut value);
    load(serde_json::to_string_pretty(&value).unwrap().as_bytes())
}

/// Asserts that loading failed with an invariant error for `rule`.
#[track_caller]
pub fn assert_rule(result: Result<Project, LoadError>, rule: Rule) {
    match result {
        Err(LoadError::Invariant(InvariantError { rule: found, .. })) if found == rule => {}
        other => panic!(
            "expected {rule:?}, got {:?}",
            other.map(|_| "a loaded project")
        ),
    }
}

/// The error message of a failed load.
#[track_caller]
pub fn error_text(result: Result<Project, LoadError>) -> String {
    result.expect_err("expected the load to fail").to_string()
}

/// A full identifier with the fixture's UUID pattern.
pub fn id(prefix: &str, n: u32) -> String {
    format!("{prefix}_01a21c90-0000-7000-8000-0000000000{n:02}")
}

// ------------------------------------------------------------- write path

use text2create_ir::change::ChangeSet;
use text2create_ir::model::ModelInvocation;
use text2create_ir::scalar::{Digest, Timestamp};
use text2create_ir::source::{Clock, IdGenerator, Source};
use text2create_ir::{Applied, WriteEnv, WriteError};

/// A clock that advances by one second per call, starting at a fixed time.
pub struct FixedClock(pub u32);

impl Clock for FixedClock {
    fn now(&mut self) -> Timestamp {
        let seconds = self.0;
        self.0 += 1;
        Timestamp::try_from(format!(
            "2026-11-28T{:02}:{:02}:{:02}.000Z",
            seconds / 3600 % 24,
            seconds / 60 % 60,
            seconds % 60
        ))
        .unwrap()
    }
}

/// Sequential UUIDs that never collide with the fixture identifiers.
pub struct SequentialIds(pub u128);

impl IdGenerator for SequentialIds {
    fn next_uuid(&mut self) -> uuid::Uuid {
        self.0 += 1;
        uuid::Uuid::from_u128(0x01a21c90_0000_7000_8000_000001000000 + self.0)
    }
}

pub fn text<const N: usize>(value: &str) -> Text<N> {
    Text::try_from(value.to_owned()).unwrap()
}

pub fn digest(seed: char) -> Digest {
    Digest::try_from(format!("sha256:{}", seed.to_string().repeat(64))).unwrap()
}

pub fn user() -> Source {
    Source::user_edit()
}

pub fn model() -> Source {
    Source::model_interpretation(
        text("ollama"),
        text("example-model"),
        ModelInvocation {
            prompt_digest: digest('a'),
            response_digest: digest('b'),
        },
    )
}

pub fn tool() -> Source {
    Source::tool_task_run(
        text("example-tool"),
        text("1.0.0"),
        id("tsk", 62).parse().unwrap(),
        id("run", 82).parse().unwrap(),
    )
}

pub fn system() -> Source {
    Source::system_import(text("example-checker"))
}

/// Builds a change set from JSON operations, through the strict parser.
pub fn changes(operations: Value) -> ChangeSet {
    let json = serde_json::json!({"inputs": [], "operations": operations});
    ChangeSet::from_json(json.to_string().as_bytes()).unwrap()
}

/// A project with a deterministic clock and identifier generator.
pub struct Harness {
    pub project: Project,
    pub registry: DomainRegistry,
    pub clock: FixedClock,
    pub ids: SequentialIds,
}

impl Harness {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            registry: registry(),
            clock: FixedClock(0),
            ids: SequentialIds(0),
        }
    }

    /// The example fixture.
    pub fn example() -> Self {
        Self::new(load(&read(EXAMPLE)).unwrap())
    }

    pub fn canonical(&self) -> String {
        self.project.to_canonical_json()
    }

    /// Applies a change set. A failure must leave the project byte-identical.
    pub fn apply_set(&mut self, source: &Source, set: &ChangeSet) -> Result<Applied, WriteError> {
        let before = self.canonical();
        let provenance = self.project.document().provenance.len();
        let mut env = WriteEnv {
            registry: &self.registry,
            clock: &mut self.clock,
            ids: &mut self.ids,
        };
        let result = self.project.apply(source, set, &mut env);
        match &result {
            Err(_) => assert_eq!(
                self.canonical(),
                before,
                "a failed change set changed the project"
            ),
            Ok(_) => {
                assert_eq!(self.project.document().provenance.len(), provenance + 1);
                let reloaded = Project::load(self.canonical().as_bytes(), &self.registry)
                    .expect("an applied change set must produce a loadable project");
                assert_eq!(reloaded, self.project);
            }
        }
        result
    }

    pub fn apply(&mut self, source: &Source, operations: Value) -> Result<Applied, WriteError> {
        self.apply_set(source, &changes(operations))
    }

    /// Applies and expects success.
    #[track_caller]
    pub fn ok(&mut self, source: &Source, operations: Value) -> Applied {
        match self.apply(source, operations) {
            Ok(applied) => applied,
            Err(error) => panic!("change set rejected: {error}"),
        }
    }

    /// Applies and expects a rejection.
    #[track_caller]
    pub fn rejected(&mut self, source: &Source, operations: Value) -> WriteError {
        self.apply(source, operations)
            .expect_err("expected the change set to be rejected")
    }
}
