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
use text2create_ir::{InvariantError, LoadError, Project, Rule};

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
            return Err(DomainError::Invalid("an assembly needs parts".to_owned()));
        }
        if !self.parts.windows(2).all(|pair| pair[0] < pair[1]) {
            return Err(DomainError::Invalid(
                "parts must be sorted and unique".to_owned(),
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
