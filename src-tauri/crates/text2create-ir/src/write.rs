//! The write path: the only way to create or change a validated project.
//!
//! A change set is applied to a copy of the project. The copy is checked with
//! the same payload normalization and invariants as a loaded project, and the
//! project is replaced only if every check passes. A failing change set leaves
//! the project unchanged.

use crate::change::{
    ChangeSet, EntityDecision, NewTask, Operation, PlanDecision, Ref, RequirementDecision,
    TaskDecision, TempId,
};
use crate::domain::{CORE_DOMAIN, DomainError, DomainRegistry};
use crate::ids::{
    ArtifactId, EntityId, LinkId, PlanId, ProjectId, ProvenanceId, RecordRef, RequirementId,
    TaskId, ValidationId,
};
use crate::invariants::{self, InvariantError, KindUse};
use crate::model::{
    Activity, Artifact, ContentRecord, DecisionStatus, DomainRef, Entity, FORMAT_SCHEMA,
    FORMAT_VERSION, FormatHeader, Link, Payload, Plan, PlanStatus, ProjectDocument, ProjectInfo,
    ProvenanceRecord, RecordProvenance, Requirement, RequirementStatus, Subject, Task,
    TaskDisposition, ValidationResult,
};
use crate::project::{LoadError, Project, normalize_payloads};
use crate::scalar::{DomainId, Kind, Text};
use crate::source::{Clock, IdGenerator, Source};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// What the core needs to apply a change: the available domains, and the
/// clock and identifier generator that allocate timestamps and identifiers.
pub struct WriteEnv<'a> {
    pub registry: &'a DomainRegistry,
    pub clock: &'a mut dyn Clock,
    pub ids: &'a mut dyn IdGenerator,
}

/// The result of an applied change set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    /// The provenance record created for the change set.
    pub provenance: ProvenanceId,
    /// The identifiers allocated for the temporary references.
    pub temps: BTreeMap<TempId, RecordRef>,
}

/// A change set was rejected. The project is unchanged.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}{kind}", .operation.map(|i| format!("operation {i}: ")).unwrap_or_default())]
pub struct WriteError {
    /// The index of the failing operation, if the error belongs to one.
    pub operation: Option<usize>,
    pub kind: WriteErrorKind,
}

/// Why a change set was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WriteErrorKind {
    #[error("the change set has no operations")]
    EmptyChangeSet,
    #[error("temporary reference `{0}` is declared twice")]
    DuplicateTemp(TempId),
    #[error("temporary reference `{0}` is not declared by an earlier operation")]
    UnknownTemp(TempId),
    #[error("temporary reference `{temp}` does not refer to a {expected}")]
    TempTypeMismatch {
        temp: TempId,
        expected: &'static str,
    },
    #[error("temporary reference `{0}` is declared but never used")]
    UnusedTemp(TempId),
    #[error("record `{0}` does not exist")]
    NotFound(String),
    #[error("not permitted: {0}")]
    NotPermitted(&'static str),
    #[error("invalid state change: {0}")]
    InvalidTransition(&'static str),
    #[error("domain `{0}` is not available in this build or is reserved")]
    UnknownDomain(String),
    #[error("domain `{}` is already declared", .0.as_str())]
    DomainAlreadyDeclared(DomainId),
    #[error("payload rejected: {0}")]
    Payload(DomainError),
    #[error(transparent)]
    Invariant(InvariantError),
}

fn fail<T>(kind: WriteErrorKind) -> Result<T, WriteErrorKind> {
    Err(kind)
}

impl Project {
    /// Creates a new, empty project. Its single provenance record and the
    /// project identifier are allocated by the core.
    pub fn create(
        source: &Source,
        name: Text<120>,
        description: Option<Text<2000>>,
        env: &mut WriteEnv<'_>,
    ) -> Result<(Project, ProvenanceId), WriteError> {
        let provenance = ProvenanceId::from_uuid(env.ids.next_uuid());
        let record = provenance_record(provenance, source, Vec::new(), env.clock);
        let document = ProjectDocument {
            format: FormatHeader {
                schema: FORMAT_SCHEMA.to_owned(),
                version: FORMAT_VERSION,
            },
            domains: Vec::new(),
            project: ProjectInfo {
                id: ProjectId::from_uuid(env.ids.next_uuid()),
                provenance: new_record(provenance),
                name,
                description,
            },
            provenance: vec![record],
            requirements: Vec::new(),
            entities: Vec::new(),
            artifacts: Vec::new(),
            links: Vec::new(),
            plans: Vec::new(),
            validations: Vec::new(),
        };
        validate(&document, env.registry).map_err(|kind| WriteError {
            operation: None,
            kind,
        })?;
        Ok((Project { document }, provenance))
    }

    /// Applies a change set from `source` all or nothing.
    ///
    /// Creates exactly one provenance record. On error, the project is
    /// unchanged.
    pub fn apply(
        &mut self,
        source: &Source,
        changes: &ChangeSet,
        env: &mut WriteEnv<'_>,
    ) -> Result<Applied, WriteError> {
        let mut writer = Writer::new(&self.document, source, env)?;
        for (index, operation) in changes.operations.iter().enumerate() {
            writer.apply(operation).map_err(|kind| WriteError {
                operation: Some(index),
                kind,
            })?;
        }
        let (document, applied) = writer.finish(&self.document, changes)?;
        self.document = document;
        Ok(applied)
    }
}

fn provenance_record(
    id: ProvenanceId,
    source: &Source,
    inputs: Vec<crate::model::InputRef>,
    clock: &mut dyn Clock,
) -> ProvenanceRecord {
    ProvenanceRecord {
        id,
        at: clock.now(),
        actor: source.actor.clone(),
        activity: source.activity.clone(),
        inputs,
        model: source.model.clone(),
    }
}

fn new_record(provenance: ProvenanceId) -> RecordProvenance {
    RecordProvenance {
        rev: 1,
        created: provenance,
        modified: provenance,
    }
}

/// Payload normalization and invariants, exactly as during loading.
fn validate(document: &ProjectDocument, registry: &DomainRegistry) -> Result<(), WriteErrorKind> {
    let references = normalize_payloads(document, registry).map_err(|error| match error {
        LoadError::Payload { error, .. } => WriteErrorKind::Payload(error),
        LoadError::Invariant(error) => WriteErrorKind::Invariant(error),
        other => unreachable!("payload normalization only fails on payloads: {other}"),
    })?;
    invariants::check(document, registry, &references).map_err(WriteErrorKind::Invariant)
}

fn record_exists(document: &ProjectDocument, record: RecordRef) -> bool {
    match record {
        RecordRef::Requirement(id) => document.requirements.iter().any(|r| r.id == id),
        RecordRef::Entity(id) => document.entities.iter().any(|r| r.id == id),
        RecordRef::Artifact(id) => document.artifacts.iter().any(|r| r.id == id),
        RecordRef::Plan(id) => document.plans.iter().any(|r| r.id == id),
        RecordRef::Task(id) => document
            .plans
            .iter()
            .flat_map(|p| &p.tasks)
            .any(|t| t.id == id),
        RecordRef::Validation(id) => document.validations.iter().any(|r| r.id == id),
    }
}

/// A record type that a [`Ref`] can point to.
trait RefTarget: Copy {
    const NAME: &'static str;
    fn from_record(record: RecordRef) -> Option<Self>;
    fn record(self) -> RecordRef;
}

macro_rules! ref_target {
    ($($id:ident => $variant:ident, $name:literal;)*) => {$(
        impl RefTarget for $id {
            const NAME: &'static str = $name;
            fn from_record(record: RecordRef) -> Option<Self> {
                match record {
                    RecordRef::$variant(id) => Some(id),
                    _ => None,
                }
            }
            fn record(self) -> RecordRef {
                RecordRef::$variant(self)
            }
        }
    )*};
}

ref_target! {
    RequirementId => Requirement, "requirement";
    EntityId => Entity, "entity";
    ArtifactId => Artifact, "artifact";
    PlanId => Plan, "plan";
    TaskId => Task, "task";
    ValidationId => Validation, "validation result";
}

impl RefTarget for RecordRef {
    const NAME: &'static str = "record";
    fn from_record(record: RecordRef) -> Option<Self> {
        Some(record)
    }
    fn record(self) -> RecordRef {
        self
    }
}

/// Applies operations to a copy of the document.
struct Writer<'s, 'e, 'r> {
    document: ProjectDocument,
    source: &'s Source,
    env: &'e mut WriteEnv<'r>,
    provenance: ProvenanceId,
    temps: BTreeMap<TempId, (RecordRef, bool)>,
    /// Records created by this change set. They keep `rev` 1.
    created: BTreeSet<RecordRef>,
    /// Existing records already modified by this change set.
    modified: BTreeSet<RecordRef>,
    project_modified: bool,
}

impl<'s, 'e, 'r> Writer<'s, 'e, 'r> {
    fn new(
        base: &ProjectDocument,
        source: &'s Source,
        env: &'e mut WriteEnv<'r>,
    ) -> Result<Self, WriteError> {
        let error = |kind| WriteError {
            operation: None,
            kind,
        };
        // The task of a task run must exist when the run is recorded.
        if let Activity::TaskRun { task, .. } = &source.activity
            && !record_exists(base, RecordRef::Task(*task))
        {
            return Err(error(WriteErrorKind::NotFound(task.to_string())));
        }
        let provenance = ProvenanceId::from_uuid(env.ids.next_uuid());
        Ok(Self {
            document: base.clone(),
            source,
            env,
            provenance,
            temps: BTreeMap::new(),
            created: BTreeSet::new(),
            modified: BTreeSet::new(),
            project_modified: false,
        })
    }

    fn finish(
        mut self,
        base: &ProjectDocument,
        changes: &ChangeSet,
    ) -> Result<(ProjectDocument, Applied), WriteError> {
        let error = |kind| WriteError {
            operation: None,
            kind,
        };
        if changes.operations.is_empty() {
            return Err(error(WriteErrorKind::EmptyChangeSet));
        }
        for input in &changes.inputs {
            if !record_exists(base, input.record) {
                return Err(error(WriteErrorKind::NotFound(input.record.to_string())));
            }
        }
        if let Some((temp, _)) = self.temps.iter().find(|(_, (_, used))| !used) {
            return Err(error(WriteErrorKind::UnusedTemp(temp.clone())));
        }
        if self.project_modified {
            bump(&mut self.document.project.provenance, self.provenance);
        }
        let record = provenance_record(
            self.provenance,
            self.source,
            changes.inputs.clone(),
            self.env.clock,
        );
        let mut document = self.document;
        document.provenance.push(record);
        sort(&mut document);
        validate(&document, self.env.registry).map_err(error)?;
        // Append-only provenance: the existing records are kept unchanged.
        debug_assert!(
            base.provenance
                .iter()
                .all(|p| document.provenance.contains(p))
        );
        let temps = self
            .temps
            .into_iter()
            .map(|(temp, (record, _))| (temp, record))
            .collect();
        Ok((
            document,
            Applied {
                provenance: self.provenance,
                temps,
            },
        ))
    }

    fn allocate<T>(&mut self, wrap: fn(Uuid) -> T) -> T {
        wrap(self.env.ids.next_uuid())
    }

    fn declare(&mut self, temp: &Option<TempId>, record: RecordRef) -> Result<(), WriteErrorKind> {
        self.created.insert(record);
        if let Some(temp) = temp {
            if self.temps.contains_key(temp) {
                return fail(WriteErrorKind::DuplicateTemp(temp.clone()));
            }
            self.temps.insert(temp.clone(), (record, false));
        }
        Ok(())
    }

    fn resolve<T: RefTarget>(&mut self, reference: &Ref<T>) -> Result<T, WriteErrorKind> {
        match reference {
            Ref::Id(id) => {
                if record_exists(&self.document, id.record()) {
                    Ok(*id)
                } else {
                    fail(WriteErrorKind::NotFound(id.record().to_string()))
                }
            }
            Ref::Temp(temp) => {
                let Some((record, used)) = self.temps.get_mut(temp) else {
                    return fail(WriteErrorKind::UnknownTemp(temp.clone()));
                };
                *used = true;
                T::from_record(*record).ok_or_else(|| WriteErrorKind::TempTypeMismatch {
                    temp: temp.clone(),
                    expected: T::NAME,
                })
            }
        }
    }

    fn resolve_all<T: RefTarget>(
        &mut self,
        references: &[Ref<T>],
    ) -> Result<Vec<T>, WriteErrorKind> {
        references.iter().map(|r| self.resolve(r)).collect()
    }

    /// Marks an existing record as modified once per change set.
    fn touch(&mut self, record: RecordRef) -> bool {
        !self.created.contains(&record) && self.modified.insert(record)
    }

    fn require_user(&self, what: &'static str) -> Result<(), WriteErrorKind> {
        if self.source.is_user() {
            Ok(())
        } else {
            fail(WriteErrorKind::NotPermitted(what))
        }
    }

    fn normalize(&self, kind: &Kind, data: &Value) -> Result<Payload, WriteErrorKind> {
        let codec =
            invariants::resolve_kind(kind, KindUse::Entity, &self.document, self.env.registry)
                .map_err(WriteErrorKind::Invariant)?;
        let normalized = codec
            .normalize_entity(kind.name(), data)
            .map_err(WriteErrorKind::Payload)?;
        Ok(Payload::from_normalized(normalized.value))
    }

    fn apply(&mut self, operation: &Operation) -> Result<(), WriteErrorKind> {
        let prv = self.provenance;
        match operation {
            Operation::SetProjectInfo { name, description } => {
                self.document.project.name = name.clone();
                self.document.project.description = description.clone();
                self.project_modified = true;
            }
            Operation::DeclareDomain { domain } => {
                let codec = self
                    .env
                    .registry
                    .get(domain.as_str())
                    .filter(|_| domain.as_str() != CORE_DOMAIN)
                    .ok_or_else(|| WriteErrorKind::UnknownDomain(domain.as_str().to_owned()))?;
                if self.document.domains.iter().any(|d| d.id == *domain) {
                    return fail(WriteErrorKind::DomainAlreadyDeclared(domain.clone()));
                }
                let version = codec.version();
                self.document.domains.push(DomainRef {
                    id: domain.clone(),
                    version,
                });
                self.project_modified = true;
            }
            Operation::AddRequirement {
                temp,
                kind,
                priority,
                title,
                statement,
                origin,
                acceptance,
            } => {
                let id = self.allocate(RequirementId::from_uuid);
                self.declare(temp, id.record())?;
                self.document.requirements.push(Requirement {
                    id,
                    provenance: new_record(prv),
                    kind: *kind,
                    priority: *priority,
                    status: RequirementStatus::Proposed {},
                    title: title.clone(),
                    statement: statement.clone(),
                    origin: origin.clone(),
                    acceptance: acceptance.clone(),
                });
            }
            Operation::EditRequirement {
                requirement,
                kind,
                priority,
                title,
                statement,
                origin,
                acceptance,
            } => {
                let id = self.resolve(requirement)?;
                let is_user = self.source.is_user();
                let touched = self.touch(id.record());
                let r = requirement_mut(&mut self.document, id);
                match r.status {
                    RequirementStatus::Superseded { .. } => {
                        return fail(WriteErrorKind::InvalidTransition(
                            "a superseded requirement cannot be edited",
                        ));
                    }
                    RequirementStatus::Proposed {} => {}
                    _ if !is_user => {
                        return fail(WriteErrorKind::NotPermitted(
                            "only the user can edit a decided requirement",
                        ));
                    }
                    _ => {}
                }
                r.kind = *kind;
                r.priority = *priority;
                r.title = title.clone();
                r.statement = statement.clone();
                r.origin = origin.clone();
                r.acceptance = acceptance.clone();
                bump_if(touched, &mut r.provenance, prv);
            }
            Operation::DecideRequirement {
                requirement,
                decision,
            } => {
                self.require_user("only the user can confirm or reject a requirement")?;
                let id = self.resolve(requirement)?;
                let touched = self.touch(id.record());
                let r = requirement_mut(&mut self.document, id);
                if matches!(r.status, RequirementStatus::Superseded { .. }) {
                    return fail(WriteErrorKind::InvalidTransition(
                        "a superseded requirement cannot be decided again",
                    ));
                }
                r.status = match decision {
                    RequirementDecision::Confirm => RequirementStatus::Confirmed { by: prv },
                    RequirementDecision::Reject => RequirementStatus::Rejected { by: prv },
                };
                bump_if(touched, &mut r.provenance, prv);
            }
            Operation::SupersedeRequirement {
                requirement,
                replacement,
            } => {
                self.require_user("only the user can supersede a requirement")?;
                let id = self.resolve(requirement)?;
                let replacement = self.resolve(replacement)?;
                if id == replacement {
                    return fail(WriteErrorKind::InvalidTransition(
                        "a requirement cannot supersede itself",
                    ));
                }
                let touched = self.touch(id.record());
                let r = requirement_mut(&mut self.document, id);
                if matches!(r.status, RequirementStatus::Superseded { .. }) {
                    return fail(WriteErrorKind::InvalidTransition(
                        "the requirement is already superseded",
                    ));
                }
                r.status = RequirementStatus::Superseded {
                    replacement,
                    by: prv,
                };
                bump_if(touched, &mut r.provenance, prv);
            }
            Operation::AddEntity {
                temp,
                kind,
                name,
                data,
            } => {
                let data = self.normalize(kind, data)?;
                let id = self.allocate(EntityId::from_uuid);
                self.declare(temp, id.record())?;
                self.document.entities.push(Entity {
                    id,
                    provenance: new_record(prv),
                    kind: kind.clone(),
                    name: name.clone(),
                    status: DecisionStatus::Proposed {},
                    data,
                });
            }
            Operation::EditEntity { entity, name, data } => {
                let id = self.resolve(entity)?;
                let kind = entity_mut(&mut self.document, id).kind.clone();
                let data = self.normalize(&kind, data)?;
                let is_user = self.source.is_user();
                let touched = self.touch(id.record());
                let e = entity_mut(&mut self.document, id);
                if !is_user && !matches!(e.status, DecisionStatus::Proposed {}) {
                    return fail(WriteErrorKind::NotPermitted(
                        "only the user can edit a decided entity",
                    ));
                }
                e.name = name.clone();
                e.data = data;
                bump_if(touched, &mut e.provenance, prv);
            }
            Operation::DecideEntity { entity, decision } => {
                self.require_user("only the user can accept or reject an entity")?;
                let id = self.resolve(entity)?;
                let touched = self.touch(id.record());
                let e = entity_mut(&mut self.document, id);
                e.status = match decision {
                    EntityDecision::Accept => DecisionStatus::Accepted { by: prv },
                    EntityDecision::Reject => DecisionStatus::Rejected { by: prv },
                };
                bump_if(touched, &mut e.provenance, prv);
            }
            Operation::AddArtifact {
                temp,
                kind,
                path,
                ownership,
            } => {
                let id = self.allocate(ArtifactId::from_uuid);
                self.declare(temp, id.record())?;
                self.document.artifacts.push(Artifact {
                    id,
                    provenance: new_record(prv),
                    kind: kind.clone(),
                    path: path.clone(),
                    ownership: *ownership,
                    content: None,
                });
            }
            Operation::RecordArtifactContent {
                artifact,
                digest,
                size_bytes,
            } => {
                let id = self.resolve(artifact)?;
                let touched = self.touch(id.record());
                let a = self
                    .document
                    .artifacts
                    .iter_mut()
                    .find(|a| a.id == id)
                    .expect("resolved");
                a.content = Some(ContentRecord {
                    digest: digest.clone(),
                    size_bytes: *size_bytes,
                    provenance: prv,
                });
                bump_if(touched, &mut a.provenance, prv);
            }
            Operation::AddLink { kind, from, to } => {
                let from = self.resolve(from)?;
                let to = self.resolve(to)?;
                let id = self.allocate(LinkId::from_uuid);
                self.document.links.push(Link {
                    id,
                    provenance: new_record(prv),
                    kind: kind.clone(),
                    from,
                    to,
                });
            }
            Operation::AddPlan { temp, goal, tasks } => {
                let id = self.allocate(PlanId::from_uuid);
                self.declare(temp, id.record())?;
                // Declare every task first so that dependencies can refer to
                // tasks listed later in the same plan.
                let ids: Vec<TaskId> = tasks
                    .iter()
                    .map(|_| self.allocate(TaskId::from_uuid))
                    .collect();
                for (task, id) in tasks.iter().zip(&ids) {
                    self.declare(&task.temp, id.record())?;
                }
                let mut new_tasks = Vec::with_capacity(tasks.len());
                for (task, id) in tasks.iter().zip(ids) {
                    new_tasks.push(self.new_task(task, id)?);
                }
                new_tasks.sort_by_key(|t| t.id);
                self.document.plans.push(Plan {
                    id,
                    provenance: new_record(prv),
                    goal: goal.clone(),
                    status: PlanStatus::Draft {},
                    tasks: new_tasks,
                });
            }
            Operation::DecidePlan { plan, decision } => {
                self.require_user("only the user can approve or archive a plan")?;
                let id = self.resolve(plan)?;
                let touched = self.touch(id.record());
                let p = plan_mut(&mut self.document, id);
                p.status = match (decision, &p.status) {
                    (PlanDecision::Approve, PlanStatus::Draft {}) => {
                        PlanStatus::Approved { by: prv }
                    }
                    (PlanDecision::Approve, _) => {
                        return fail(WriteErrorKind::InvalidTransition(
                            "only a draft plan can be approved",
                        ));
                    }
                    (PlanDecision::Archive, PlanStatus::Archived { .. }) => {
                        return fail(WriteErrorKind::InvalidTransition(
                            "the plan is already archived",
                        ));
                    }
                    (PlanDecision::Archive, _) => PlanStatus::Archived { by: prv },
                };
                bump_if(touched, &mut p.provenance, prv);
            }
            Operation::SetTaskDisposition { task, disposition } => {
                self.require_user("only the user can skip or cancel a task")?;
                let id = self.resolve(task)?;
                let plan_id = self
                    .document
                    .plans
                    .iter()
                    .find(|p| p.tasks.iter().any(|t| t.id == id))
                    .expect("resolved")
                    .id;
                let touched = self.touch(plan_id.record());
                let p = plan_mut(&mut self.document, plan_id);
                if matches!(p.status, PlanStatus::Archived { .. }) {
                    return fail(WriteErrorKind::InvalidTransition(
                        "tasks of an archived plan cannot be changed",
                    ));
                }
                let t = p.tasks.iter_mut().find(|t| t.id == id).expect("resolved");
                if !matches!(t.disposition, TaskDisposition::Active {}) {
                    return fail(WriteErrorKind::InvalidTransition(
                        "only an active task can be skipped or cancelled",
                    ));
                }
                t.disposition = match disposition {
                    TaskDecision::Skip => TaskDisposition::Skipped { by: prv },
                    TaskDecision::Cancel => TaskDisposition::Cancelled { by: prv },
                };
                bump_if(touched, &mut p.provenance, prv);
            }
            Operation::RecordValidation {
                temp,
                check,
                subjects,
                requirements,
                outcome,
                summary,
                diagnostics,
            } => {
                if !self.source.is_tool_or_system() {
                    return fail(WriteErrorKind::NotPermitted(
                        "only a tool or the system can record a validation result",
                    ));
                }
                let mut resolved = Vec::with_capacity(subjects.len());
                for subject in subjects {
                    resolved.push(Subject {
                        artifact: self.resolve(&subject.artifact)?,
                        digest: subject.digest.clone(),
                    });
                }
                let requirements = self.resolve_all(requirements)?;
                let id = self.allocate(ValidationId::from_uuid);
                self.declare(temp, id.record())?;
                self.document.validations.push(ValidationResult {
                    id,
                    provenance: prv,
                    check: check.clone(),
                    subjects: resolved,
                    requirements,
                    outcome: *outcome,
                    summary: summary.clone(),
                    diagnostics: diagnostics.clone(),
                });
            }
        }
        Ok(())
    }

    fn new_task(&mut self, task: &NewTask, id: TaskId) -> Result<Task, WriteErrorKind> {
        Ok(Task {
            id,
            title: task.title.clone(),
            action: task.action.clone(),
            depends_on: self.resolve_all(&task.depends_on)?,
            inputs: self.resolve_all(&task.inputs)?,
            expected_outputs: self.resolve_all(&task.expected_outputs)?,
            addresses: self.resolve_all(&task.addresses)?,
            disposition: TaskDisposition::Active {},
        })
    }
}

fn bump(provenance: &mut RecordProvenance, by: ProvenanceId) {
    provenance.rev = provenance.rev.saturating_add(1);
    provenance.modified = by;
}

fn bump_if(touched: bool, provenance: &mut RecordProvenance, by: ProvenanceId) {
    if touched {
        bump(provenance, by);
    }
}

fn requirement_mut(document: &mut ProjectDocument, id: RequirementId) -> &mut Requirement {
    document
        .requirements
        .iter_mut()
        .find(|r| r.id == id)
        .expect("resolved")
}

fn entity_mut(document: &mut ProjectDocument, id: EntityId) -> &mut Entity {
    document
        .entities
        .iter_mut()
        .find(|e| e.id == id)
        .expect("resolved")
}

fn plan_mut(document: &mut ProjectDocument, id: PlanId) -> &mut Plan {
    document
        .plans
        .iter_mut()
        .find(|p| p.id == id)
        .expect("resolved")
}

/// Restores the canonical order of every collection.
fn sort(document: &mut ProjectDocument) {
    document.domains.sort_by(|a, b| a.id.cmp(&b.id));
    document.provenance.sort_by_key(|r| r.id);
    document.requirements.sort_by_key(|r| r.id);
    document.entities.sort_by_key(|r| r.id);
    document.artifacts.sort_by_key(|r| r.id);
    document.links.sort_by_key(|r| r.id);
    document.plans.sort_by_key(|r| r.id);
    document.validations.sort_by_key(|r| r.id);
}
