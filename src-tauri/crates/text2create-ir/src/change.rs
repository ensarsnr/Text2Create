//! Change sets: the serializable description of a change to a project.
//!
//! A change set contains the records a change was derived from and an ordered
//! list of typed operations. It deliberately contains no actor, activity,
//! identifiers of new records, or timestamps: the source of a change is a
//! [`crate::source::Source`] chosen by trusted Rust code, and identifiers and
//! timestamps are allocated by the core when the change set is applied.
//!
//! New records can be named with a temporary reference ([`TempId`]) so that
//! later operations in the same change set can refer to them.

use crate::ids::{ArtifactId, EntityId, PlanId, RecordRef, RequirementId, TaskId};
use crate::model::{
    Diagnostic, InputRef, Outcome, Ownership, Priority, RequirementKind, RequirementOrigin,
};
use crate::scalar::{Digest, DomainId, Kind, ProjectPath, ScalarError, Text};
use crate::strict::{self, StrictJsonError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maximum length of a [`TempId`].
pub const MAX_TEMP_ID_LEN: usize = 64;

/// A temporary name for a record created in the same change set:
/// 1 to 64 characters from `[A-Za-z0-9_-]`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TempId(String);

impl TryFrom<String> for TempId {
    type Error = ScalarError;
    fn try_from(value: String) -> Result<Self, ScalarError> {
        let valid = !value.is_empty()
            && value.len() <= MAX_TEMP_ID_LEN
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if valid {
            Ok(Self(value))
        } else {
            Err(ScalarError {
                what: "temporary reference",
                reason: "expected 1 to 64 characters from [A-Za-z0-9_-]",
            })
        }
    }
}

impl From<TempId> for String {
    fn from(value: TempId) -> Self {
        value.0
    }
}

impl TempId {
    /// The temporary name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TempId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A reference to an existing record (`{"id": "req_…"}`) or to a record
/// created earlier in the same change set (`{"temp": "r1"}`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Ref<T> {
    Id(T),
    Temp(TempId),
}

/// Errors produced by [`ChangeSet::from_json`].
#[derive(Debug, thiserror::Error)]
pub enum ChangeSetParseError {
    /// Size limit or strict JSON parsing.
    #[error(transparent)]
    Json(#[from] StrictJsonError),
    /// The JSON does not describe a change set. The message includes the line
    /// and column.
    #[error("invalid change set {}{error}", crate::strict::at(path))]
    Schema {
        path: crate::path::FieldPath,
        #[source]
        error: serde_json::Error,
    },
}

/// A change to a project: an ordered list of operations, applied all or
/// nothing by [`crate::Project::apply`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeSet {
    /// Existing records the change was derived from. Recorded in the
    /// provenance record of the change.
    pub inputs: Vec<InputRef>,
    pub operations: Vec<Operation>,
}

impl ChangeSet {
    /// A change set without inputs.
    pub fn new(operations: Vec<Operation>) -> Self {
        Self {
            inputs: Vec::new(),
            operations,
        }
    }

    /// Parses a change set from untrusted bytes (for example a model response
    /// converted by #20/#21) with the strict JSON parser.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ChangeSetParseError> {
        strict::parse(bytes)?;
        // Parse the original bytes again so that errors report a line and column.
        crate::path::from_slice(bytes)
            .map_err(|(path, error)| ChangeSetParseError::Schema { path, error })
    }
}

/// A typed operation. There is no generic patch operation: each variant
/// changes one kind of record in one specific way.
///
/// Operations that record a decision (`decide_*`, `supersede_requirement`,
/// `set_task_disposition`) are accepted only from the user. Validation
/// results are accepted only from a tool or the system.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Replaces the project name and description.
    SetProjectInfo {
        name: Text<120>,
        #[serde(default)]
        description: Option<Text<2000>>,
    },
    /// Adds a domain to `domains`, at the version of this build.
    DeclareDomain { domain: DomainId },
    /// Adds a `proposed` requirement.
    AddRequirement {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temp: Option<TempId>,
        kind: RequirementKind,
        priority: Priority,
        title: Text<120>,
        statement: Text<2000>,
        origin: RequirementOrigin,
        acceptance: Vec<Text<500>>,
    },
    /// Replaces the content of a requirement. Only the user can edit a
    /// requirement that is no longer `proposed`; superseded requirements
    /// cannot be edited.
    EditRequirement {
        requirement: Ref<RequirementId>,
        kind: RequirementKind,
        priority: Priority,
        title: Text<120>,
        statement: Text<2000>,
        origin: RequirementOrigin,
        acceptance: Vec<Text<500>>,
    },
    /// Confirms or rejects a requirement. User only.
    DecideRequirement {
        requirement: Ref<RequirementId>,
        decision: RequirementDecision,
    },
    /// Marks a requirement as superseded by another. User only.
    SupersedeRequirement {
        requirement: Ref<RequirementId>,
        replacement: Ref<RequirementId>,
    },
    /// Adds a `proposed` entity. The payload is normalized by its domain codec.
    AddEntity {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temp: Option<TempId>,
        kind: Kind,
        name: Text<120>,
        data: Value,
    },
    /// Replaces the name and payload of an entity. Only the user can edit an
    /// entity that is no longer `proposed`.
    EditEntity {
        entity: Ref<EntityId>,
        name: Text<120>,
        data: Value,
    },
    /// Accepts or rejects an entity. User only.
    DecideEntity {
        entity: Ref<EntityId>,
        decision: EntityDecision,
    },
    /// Adds an artifact reference without content.
    AddArtifact {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temp: Option<TempId>,
        kind: Kind,
        path: ProjectPath,
        ownership: Ownership,
    },
    /// Records the current content of an artifact's file.
    RecordArtifactContent {
        artifact: Ref<ArtifactId>,
        digest: Digest,
        size_bytes: u64,
    },
    /// Adds a typed link.
    AddLink {
        kind: Kind,
        from: Ref<RecordRef>,
        to: Ref<RecordRef>,
    },
    /// Adds a `draft` plan with its tasks.
    AddPlan {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temp: Option<TempId>,
        goal: Text<500>,
        tasks: Vec<NewTask>,
    },
    /// Approves a draft plan or archives a plan. User only.
    DecidePlan {
        plan: Ref<PlanId>,
        decision: PlanDecision,
    },
    /// Skips or cancels an active task. User only.
    SetTaskDisposition {
        task: Ref<TaskId>,
        disposition: TaskDecision,
    },
    /// Records a validation result. Tool or system only.
    RecordValidation {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temp: Option<TempId>,
        check: Kind,
        subjects: Vec<NewSubject>,
        requirements: Vec<Ref<RequirementId>>,
        outcome: Outcome,
        summary: Text<500>,
        diagnostics: Vec<Diagnostic>,
    },
}

/// A task of a new plan. Its dependencies refer to other tasks of the same plan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewTask {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temp: Option<TempId>,
    pub title: Text<200>,
    pub action: Kind,
    pub depends_on: Vec<Ref<TaskId>>,
    pub inputs: Vec<Ref<RecordRef>>,
    pub expected_outputs: Vec<Ref<ArtifactId>>,
    pub addresses: Vec<Ref<RequirementId>>,
}

/// The artifact content a check ran against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewSubject {
    pub artifact: Ref<ArtifactId>,
    pub digest: Digest,
}

/// A decision about a requirement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementDecision {
    Confirm,
    Reject,
}

/// A decision about an entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityDecision {
    Accept,
    Reject,
}

/// A decision about a plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanDecision {
    Approve,
    Archive,
}

/// A decision to stop pursuing a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDecision {
    Skip,
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_id_format() {
        for good in ["r1", "A-b_9", &"x".repeat(64)] {
            assert!(TempId::try_from(good.to_owned()).is_ok(), "{good}");
        }
        for bad in ["", "a b", "a.b", "é", &"x".repeat(65)] {
            assert!(TempId::try_from(bad.to_owned()).is_err(), "{bad}");
        }
    }

    #[test]
    fn change_sets_cannot_carry_an_actor_identifiers_or_timestamps() {
        for json in [
            r#"{"inputs":[],"operations":[],"actor":{"type":"user"}}"#,
            r#"{"inputs":[],"operations":[],"provenance":"prv_01a21c90-0000-7000-8000-000000000001"}"#,
            r#"{"inputs":[],"operations":[{"op":"add_artifact","id":"art_01a21c90-0000-7000-8000-000000000031","kind":"core.document","path":"a.md","ownership":"user"}]}"#,
            r#"{"inputs":[],"operations":[{"op":"set_project_info","name":"x","at":"2026-11-27T10:00:00.000Z"}]}"#,
        ] {
            let error = ChangeSet::from_json(json.as_bytes()).unwrap_err();
            assert!(error.to_string().contains("unknown field"), "{error}");
        }
    }

    #[test]
    fn change_sets_round_trip_and_are_parsed_strictly() {
        let json = r#"{"inputs":[],"operations":[{"op":"add_link","kind":"core.refines","from":{"temp":"r2"},"to":{"id":"req_01a21c90-0000-7000-8000-000000000011"}}]}"#;
        let changes = ChangeSet::from_json(json.as_bytes()).unwrap();
        assert_eq!(serde_json::to_string(&changes).unwrap(), json);
        let repeated = json.replacen(
            r#""kind":"core.refines","#,
            r#""kind":"core.refines","kind":"x.y","#,
            1,
        );
        assert!(matches!(
            ChangeSet::from_json(repeated.as_bytes()),
            Err(ChangeSetParseError::Json(_))
        ));
        assert!(matches!(
            ChangeSet::from_json(br#"{"inputs":[],"operations":[{"op":"delete_everything"}]}"#),
            Err(ChangeSetParseError::Schema { .. })
        ));
    }
}
