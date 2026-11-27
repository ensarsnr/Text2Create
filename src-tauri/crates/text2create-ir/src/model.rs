//! Project IR v1 records (`docs/PROJECT_IR.md`).
//!
//! These types define the schema. They reject unknown fields and unknown enum
//! values. Fields of type `Option` must be present in the input (as `null` when
//! empty), so every document contains every field.
//!
//! Holding a [`ProjectDocument`] does not mean it is valid. Only
//! [`crate::Project`] guarantees that all invariants hold.
//!
//! Variants without data in internally tagged enums are written as empty
//! struct variants (`User {}`), because serde ignores unknown fields on unit
//! variants of internally tagged enums even with `deny_unknown_fields`.

use crate::ids::{
    ArtifactId, EntityId, LinkId, PlanId, ProjectId, ProvenanceId, RecordRef, RequirementId,
    TaskId, TaskRunId, ValidationId,
};
use crate::scalar::{Digest, DomainId, Kind, ProjectPath, Text, Timestamp};
use serde::{Deserialize, Deserializer, Serialize};

/// Value of `format.schema`.
pub const FORMAT_SCHEMA: &str = "text2create.project-ir";
/// The format version this crate reads and writes.
pub const FORMAT_VERSION: u32 = 1;
/// Maximum number of diagnostics in one validation result.
pub const MAX_DIAGNOSTICS: usize = 200;

/// Deserializes an `Option` field without treating a missing field as `None`.
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// `format`: identifies the document and its format version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatHeader {
    pub schema: String,
    pub version: u32,
}

/// An entry of `domains`: a domain used by the project and its schema version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainRef {
    pub id: DomainId,
    pub version: u32,
}

/// A complete Project IR document. Field order is the serialization order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectDocument {
    pub format: FormatHeader,
    /// Sorted by `id`. The built-in `core` domain is implied and not listed.
    pub domains: Vec<DomainRef>,
    pub project: ProjectInfo,
    /// Append-only until the change journal (#14) exists.
    pub provenance: Vec<ProvenanceRecord>,
    pub requirements: Vec<Requirement>,
    pub entities: Vec<Entity>,
    pub artifacts: Vec<Artifact>,
    pub links: Vec<Link>,
    pub plans: Vec<Plan>,
    pub validations: Vec<ValidationResult>,
}

/// `project`: project information.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectInfo {
    pub id: ProjectId,
    pub provenance: RecordProvenance,
    pub name: Text<120>,
    #[serde(deserialize_with = "nullable")]
    pub description: Option<Text<2000>>,
}

// ---------------------------------------------------------------- Provenance

/// Provenance of a mutable record: its revision and the provenance records of
/// its creation and last modification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordProvenance {
    /// Starts at 1 and increases with each modification.
    pub rev: u32,
    pub created: ProvenanceId,
    pub modified: ProvenanceId,
}

/// Who or what caused a change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvenanceRecord {
    pub id: ProvenanceId,
    pub at: Timestamp,
    pub actor: Actor,
    pub activity: Activity,
    /// Records the change was derived from. These references are historical
    /// and may point to records that have since been removed.
    pub inputs: Vec<InputRef>,
    #[serde(deserialize_with = "nullable")]
    pub model: Option<ModelInvocation>,
}

/// The source of a change. Assigned by trusted Rust code, never taken from
/// frontend or model input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Actor {
    User {},
    Model {
        provider: Text<64>,
        model: Text<128>,
    },
    Tool {
        name: Text<64>,
        version: Text<64>,
    },
    System {
        component: Text<64>,
    },
}

/// What kind of activity produced a change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Activity {
    UserEdit {},
    /// A model turned user input into structured records.
    Interpretation {},
    /// A task run (stored outside the IR). The task reference is historical.
    TaskRun {
        task: TaskId,
        run: TaskRunId,
    },
    Import {},
    Migration {
        from: u32,
        to: u32,
    },
}

/// An input of a change, pinned to a revision or content digest where known.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputRef {
    pub record: RecordRef,
    #[serde(deserialize_with = "nullable")]
    pub rev: Option<u32>,
    #[serde(deserialize_with = "nullable")]
    pub digest: Option<Digest>,
}

/// Digests of the prompt and response of a model invocation. The texts
/// themselves are not stored in the IR.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelInvocation {
    pub prompt_digest: Digest,
    pub response_digest: Digest,
}

// -------------------------------------------------------------- Requirements

/// A requirement. Requirements inferred by a model stay `proposed` until the
/// user confirms them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub id: RequirementId,
    pub provenance: RecordProvenance,
    pub kind: RequirementKind,
    pub priority: Priority,
    pub status: RequirementStatus,
    pub title: Text<120>,
    pub statement: Text<2000>,
    pub origin: RequirementOrigin,
    pub acceptance: Vec<Text<500>>,
}

/// The kind of a requirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementKind {
    Functional,
    Performance,
    Constraint,
    Interface,
    Safety,
}

/// The priority of a requirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Must,
    Should,
    Could,
}

/// Every `by` must reference a provenance record whose actor is the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequirementStatus {
    Proposed {},
    Confirmed {
        by: ProvenanceId,
    },
    Rejected {
        by: ProvenanceId,
    },
    Superseded {
        replacement: RequirementId,
        by: ProvenanceId,
    },
}

/// Where a requirement came from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequirementOrigin {
    User {
        quote: Text<2000>,
    },
    Clarification {
        question: Text<500>,
        answer: Text<2000>,
    },
    Inferred {
        rationale: Text<1000>,
    },
}

// ------------------------------------------------------------------ Entities

/// A domain object, such as a component or a firmware target. Its payload is
/// defined by the domain that registers its kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: EntityId,
    pub provenance: RecordProvenance,
    pub kind: Kind,
    pub name: Text<120>,
    pub status: DecisionStatus,
    pub data: Payload,
}

/// The user's decision about an entity. Not a technical validation state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecisionStatus {
    Proposed {},
    Accepted { by: ProvenanceId },
    Rejected { by: ProvenanceId },
}

/// A domain payload. Validated and normalized by its domain codec whenever a
/// project is loaded; a loaded project only contains canonical payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Payload(serde_json::Value);

impl Payload {
    /// Wraps a payload produced by a domain codec.
    pub(crate) fn from_normalized(value: serde_json::Value) -> Self {
        Self(value)
    }

    /// The payload as JSON.
    pub fn as_value(&self) -> &serde_json::Value {
        &self.0
    }
}

// ----------------------------------------------------------------- Artifacts

/// A reference to a file in the project directory. File content is never
/// stored in the IR.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub id: ArtifactId,
    pub provenance: RecordProvenance,
    pub kind: Kind,
    pub path: ProjectPath,
    pub ownership: Ownership,
    /// The last recorded content, or `None` if the file has not been produced yet.
    #[serde(deserialize_with = "nullable")]
    pub content: Option<ContentRecord>,
}

/// Who owns the file an artifact references.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    /// Managed by Text2Create.
    Generated,
    /// Owned by the user; never overwritten without approval.
    User,
    ToolOutput,
}

/// The last recorded content of an artifact. Immutable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentRecord {
    pub digest: Digest,
    pub size_bytes: u64,
    pub provenance: ProvenanceId,
}

// --------------------------------------------------------------------- Links

/// A typed traceability link between records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub id: LinkId,
    pub provenance: RecordProvenance,
    pub kind: Kind,
    pub from: RecordRef,
    pub to: RecordRef,
}

// --------------------------------------------------------------------- Plans

/// A plan and the tasks it contains. Completion is derived, not stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub id: PlanId,
    pub provenance: RecordProvenance,
    pub goal: Text<500>,
    pub status: PlanStatus,
    /// Sorted by `id`.
    pub tasks: Vec<Task>,
}

/// The user's decision about a plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanStatus {
    Draft {},
    Approved { by: ProvenanceId },
    Archived { by: ProvenanceId },
}

/// Planned work. Tasks have no execution status; it is derived from task runs,
/// which are stored outside the IR (#27).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: TaskId,
    pub title: Text<200>,
    pub action: Kind,
    /// Tasks in the same plan; must be acyclic.
    pub depends_on: Vec<TaskId>,
    pub inputs: Vec<RecordRef>,
    pub expected_outputs: Vec<ArtifactId>,
    /// Confirmed requirements only.
    pub addresses: Vec<RequirementId>,
    pub disposition: TaskDisposition,
}

/// Whether a task is still planned. Not an execution status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskDisposition {
    Active {},
    Skipped { by: ProvenanceId },
    Cancelled { by: ProvenanceId },
}

// --------------------------------------------------------------- Validation

/// An immutable record of a check that ran against exact artifact contents.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidationResult {
    pub id: ValidationId,
    /// Must reference a provenance record whose actor is a tool or the system.
    pub provenance: ProvenanceId,
    pub check: Kind,
    pub subjects: Vec<Subject>,
    pub requirements: Vec<RequirementId>,
    pub outcome: Outcome,
    pub summary: Text<500>,
    /// At most [`MAX_DIAGNOSTICS`] entries.
    pub diagnostics: Vec<Diagnostic>,
}

/// The exact artifact content a check ran against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub artifact: ArtifactId,
    pub digest: Digest,
}

/// The outcome of a check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed,
    Error,
    Skipped,
}

/// A message produced by a check.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: Text<1000>,
    #[serde(deserialize_with = "nullable")]
    pub location: Option<Text<300>>,
}

/// The severity of a diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_activity_and_actor_variant_round_trips() {
        let activities = [
            r#"{"type":"user_edit"}"#,
            r#"{"type":"interpretation"}"#,
            r#"{"type":"task_run","task":"tsk_01a21c90-0000-7000-8000-000000000061","run":"run_01a21c90-0000-7000-8000-000000000081"}"#,
            r#"{"type":"import"}"#,
            r#"{"type":"migration","from":1,"to":2}"#,
        ];
        for json in activities {
            let value: Activity = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
        }
        let actors = [
            r#"{"type":"user"}"#,
            r#"{"type":"model","provider":"ollama","model":"example-model"}"#,
            r#"{"type":"tool","name":"arduino-cli","version":"1"}"#,
            r#"{"type":"system","component":"loader"}"#,
        ];
        for json in actors {
            let value: Actor = serde_json::from_str(json).unwrap();
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
        }
    }

    #[test]
    fn tagged_enums_reject_unknown_fields_and_variants() {
        assert!(serde_json::from_str::<Actor>(r#"{"type":"user","name":"x"}"#).is_err());
        assert!(
            serde_json::from_str::<RequirementStatus>(r#"{"state":"proposed","by":null}"#).is_err()
        );
        assert!(serde_json::from_str::<Actor>(r#"{"type":"robot"}"#).is_err());
        assert!(serde_json::from_str::<PlanStatus>(r#"{"state":"approved"}"#).is_err());
    }

    #[test]
    fn option_fields_must_be_present() {
        let digest = format!("sha256:{}", "0".repeat(64));
        let with_null = format!(
            r#"{{"record":"req_01a21c90-0000-7000-8000-000000000011","rev":null,"digest":"{digest}"}}"#
        );
        assert!(serde_json::from_str::<InputRef>(&with_null).is_ok());
        let missing = format!(
            r#"{{"record":"req_01a21c90-0000-7000-8000-000000000011","digest":"{digest}"}}"#
        );
        let error = serde_json::from_str::<InputRef>(&missing).unwrap_err();
        assert!(error.to_string().contains("missing field `rev`"), "{error}");
    }
}
