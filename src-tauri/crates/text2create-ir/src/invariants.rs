//! The invariants of a loaded project (`docs/PROJECT_IR.md`, "Invariants").

use crate::domain::{CORE_DOMAIN, DomainCodec, DomainRegistry};
use crate::ids::{EntityId, ProvenanceId, RecordRef, TaskId};
use crate::model::{
    Actor, DecisionStatus, MAX_DIAGNOSTICS, PlanStatus, ProjectDocument, RecordProvenance,
    RequirementStatus, TaskDisposition,
};
use crate::path::{FieldPath, Loc, ROOT};
use crate::scalar::Kind;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The invariant that was violated. The numbers refer to the list in the
/// specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// 1. Record IDs are unique and sorted; task IDs are unique across plans.
    SortedUnique,
    /// 1. `domains` is sorted, unique, and does not list `core`.
    Domains,
    /// 2. Every reference resolves.
    ReferenceResolves,
    /// 3. Every kind is registered for its use by a listed domain or `core`.
    KindRegistered,
    /// 4. Link endpoints have record types allowed by the link rule.
    LinkEndpoints,
    /// 5. Task dependencies stay within their plan and are acyclic.
    TaskDependencies,
    /// 6. Tasks address only confirmed requirements.
    AddressesConfirmed,
    /// 7. Every `by` field references a provenance record of the user.
    UserDecision,
    /// 8. Validation results reference a provenance record of a tool or the system.
    ValidationActor,
    /// 9. References to provenance records resolve.
    ProvenanceResolves,
    /// 10. `rev` is at least 1.
    Revision,
    /// 11. Entity payloads are in canonical form.
    CanonicalPayload,
    /// A resource limit, such as the number of diagnostics in a validation result.
    Limit,
    /// 13. A rule of a domain listed in `domains`, checked by
    ///     [`crate::domain::DomainCodec::check_project`].
    DomainRule,
}

/// A loaded document violates an invariant.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invariant violated ({rule:?}) {}{message}", crate::strict::at(path))]
pub struct InvariantError {
    pub rule: Rule,
    /// The location of the violation, as a JSON Pointer into the document.
    pub path: FieldPath,
    pub message: String,
}

fn fail<T>(rule: Rule, path: &Loc<'_>, message: impl fmt::Display) -> Result<T, InvariantError> {
    Err(InvariantError {
        rule,
        path: path.field_path(),
        message: message.to_string(),
    })
}

/// The use of a kind, which determines the list it must be registered in.
#[derive(Clone, Copy, Debug)]
pub(crate) enum KindUse {
    Entity,
    Artifact,
    Action,
    Check,
    Link,
}

/// Finds the codec of `kind` and checks that the kind is registered for `use_`
/// by a domain that is `core` or listed in `domains` (rule 3). `path` is the
/// location of the kind.
pub(crate) fn resolve_kind<'r>(
    kind: &Kind,
    use_: KindUse,
    doc: &ProjectDocument,
    registry: &'r DomainRegistry,
    path: &Loc<'_>,
) -> Result<&'r dyn DomainCodec, InvariantError> {
    let domain = kind.domain();
    let declared = domain == CORE_DOMAIN || doc.domains.iter().any(|d| d.id.as_str() == domain);
    let Some(codec) = registry.get(domain).filter(|_| declared) else {
        return fail(
            Rule::KindRegistered,
            path,
            format_args!(
                "kind `{}` belongs to domain `{domain}`, which is not listed in `domains`",
                kind.as_str()
            ),
        );
    };
    let name = kind.name();
    let registered = match use_ {
        KindUse::Entity => codec.entity_kinds().contains(&name),
        KindUse::Artifact => codec.artifact_kinds().contains(&name),
        KindUse::Action => codec.action_kinds().contains(&name),
        KindUse::Check => codec.check_kinds().contains(&name),
        KindUse::Link => codec.link_rules().iter().any(|rule| rule.name == name),
    };
    if !registered {
        return fail(
            Rule::KindRegistered,
            path,
            format_args!(
                "kind `{}` is not registered as a {use_:?} kind",
                kind.as_str()
            ),
        );
    }
    Ok(codec)
}

/// Checks that `items` at `path` are sorted by `key` without duplicates. The
/// error points at the first item that is out of order or repeated.
fn sorted_unique<T, K: Ord + fmt::Display>(
    items: &[T],
    key: impl Fn(&T) -> K,
    what: &str,
    rule: Rule,
    path: &Loc<'_>,
) -> Result<(), InvariantError> {
    for (index, pair) in items.windows(2).enumerate() {
        let (a, b) = (key(&pair[0]), key(&pair[1]));
        if a == b {
            return fail(
                rule,
                &path.index(index + 1),
                format_args!("duplicate {what} `{a}`"),
            );
        }
        if a > b {
            return fail(
                rule,
                &path.index(index + 1),
                format_args!("{what} `{b}` is not sorted after `{a}`"),
            );
        }
    }
    Ok(())
}

/// Checks every invariant. `payload_references` holds, for each entity in
/// order, the record IDs that its domain codec reported for its payload.
///
/// Locations are built on the stack and converted into paths only when a
/// check fails, so a valid document allocates nothing for them.
pub(crate) fn check(
    doc: &ProjectDocument,
    registry: &DomainRegistry,
    payload_references: &[(EntityId, Vec<RecordRef>)],
) -> Result<(), InvariantError> {
    let domains = ROOT.key("domains");
    let requirements = ROOT.key("requirements");
    let entities = ROOT.key("entities");
    let artifacts = ROOT.key("artifacts");
    let links = ROOT.key("links");
    let plans = ROOT.key("plans");
    let validations = ROOT.key("validations");

    // Rule 1.
    sorted_unique(
        &doc.domains,
        |d| d.id.as_str().to_owned(),
        "domain",
        Rule::Domains,
        &domains,
    )?;
    if let Some(index) = doc
        .domains
        .iter()
        .position(|d| d.id.as_str() == CORE_DOMAIN)
    {
        return fail(
            Rule::Domains,
            &domains.index(index),
            "`core` is built in and must not be listed",
        );
    }
    sorted_unique(
        &doc.provenance,
        |r| r.id,
        "provenance record",
        Rule::SortedUnique,
        &ROOT.key("provenance"),
    )?;
    sorted_unique(
        &doc.requirements,
        |r| r.id,
        "requirement",
        Rule::SortedUnique,
        &requirements,
    )?;
    sorted_unique(
        &doc.entities,
        |r| r.id,
        "entity",
        Rule::SortedUnique,
        &entities,
    )?;
    sorted_unique(
        &doc.artifacts,
        |r| r.id,
        "artifact",
        Rule::SortedUnique,
        &artifacts,
    )?;
    sorted_unique(&doc.links, |r| r.id, "link", Rule::SortedUnique, &links)?;
    sorted_unique(&doc.plans, |r| r.id, "plan", Rule::SortedUnique, &plans)?;
    sorted_unique(
        &doc.validations,
        |r| r.id,
        "validation result",
        Rule::SortedUnique,
        &validations,
    )?;
    let mut task_ids = BTreeSet::new();
    for (p, plan) in doc.plans.iter().enumerate() {
        let plan_at = plans.index(p);
        let tasks = plan_at.key("tasks");
        sorted_unique(&plan.tasks, |t| t.id, "task", Rule::SortedUnique, &tasks)?;
        for (t, task) in plan.tasks.iter().enumerate() {
            if !task_ids.insert(task.id) {
                return fail(
                    Rule::SortedUnique,
                    &tasks.index(t),
                    format_args!("duplicate task `{}`", task.id),
                );
            }
        }
    }

    let records: BTreeSet<RecordRef> = doc
        .requirements
        .iter()
        .map(|r| RecordRef::Requirement(r.id))
        .chain(doc.entities.iter().map(|r| RecordRef::Entity(r.id)))
        .chain(doc.artifacts.iter().map(|r| RecordRef::Artifact(r.id)))
        .chain(doc.plans.iter().map(|r| RecordRef::Plan(r.id)))
        .chain(task_ids.iter().map(|id| RecordRef::Task(*id)))
        .chain(doc.validations.iter().map(|r| RecordRef::Validation(r.id)))
        .collect();
    let resolve = |path: &Loc<'_>, owner: &dyn fmt::Display, reference: RecordRef| {
        if records.contains(&reference) {
            Ok(())
        } else {
            fail(
                Rule::ReferenceResolves,
                path,
                format_args!("`{owner}` references missing record `{reference}`"),
            )
        }
    };

    // Rules 7, 9, and 10.
    let actors: BTreeMap<ProvenanceId, &Actor> =
        doc.provenance.iter().map(|p| (p.id, &p.actor)).collect();
    let provenance = |path: &Loc<'_>, owner: &dyn fmt::Display, id: ProvenanceId| {
        actors.get(&id).copied().map_or_else(
            || {
                fail(
                    Rule::ProvenanceResolves,
                    path,
                    format_args!("`{owner}` references missing provenance record `{id}`"),
                )
            },
            Ok,
        )
    };
    // `path` is the location of the `provenance` object.
    let record_provenance = |path: &Loc<'_>, owner: &dyn fmt::Display, p: &RecordProvenance| {
        if p.rev == 0 {
            return fail(
                Rule::Revision,
                &path.key("rev"),
                format_args!("`{owner}` has rev 0; rev starts at 1"),
            );
        }
        provenance(&path.key("created"), owner, p.created)?;
        provenance(&path.key("modified"), owner, p.modified)?;
        Ok(())
    };
    // `path` is the location of the `by` field.
    let user_decision = |path: &Loc<'_>, owner: &dyn fmt::Display, by: ProvenanceId| {
        if matches!(provenance(path, owner, by)?, Actor::User {}) {
            Ok(())
        } else {
            fail(
                Rule::UserDecision,
                path,
                format_args!(
                    "decision on `{owner}` references `{by}`, whose actor is not the user"
                ),
            )
        }
    };

    let project = ROOT.key("project");
    record_provenance(
        &project.key("provenance"),
        &doc.project.id,
        &doc.project.provenance,
    )?;

    for (index, requirement) in doc.requirements.iter().enumerate() {
        let at = requirements.index(index);
        let id = &requirement.id;
        record_provenance(&at.key("provenance"), id, &requirement.provenance)?;
        let status = at.key("status");
        match &requirement.status {
            RequirementStatus::Proposed {} => {}
            RequirementStatus::Confirmed { by } | RequirementStatus::Rejected { by } => {
                user_decision(&status.key("by"), id, *by)?;
            }
            RequirementStatus::Superseded { replacement, by } => {
                user_decision(&status.key("by"), id, *by)?;
                resolve(
                    &status.key("replacement"),
                    id,
                    RecordRef::Requirement(*replacement),
                )?;
            }
        }
    }

    for (index, entity) in doc.entities.iter().enumerate() {
        let at = entities.index(index);
        record_provenance(&at.key("provenance"), &entity.id, &entity.provenance)?;
        resolve_kind(
            &entity.kind,
            KindUse::Entity,
            doc,
            registry,
            &at.key("kind"),
        )?;
        if let DecisionStatus::Accepted { by } | DecisionStatus::Rejected { by } = &entity.status {
            user_decision(&at.key("status").key("by"), &entity.id, *by)?;
        }
    }
    for (index, (entity, references)) in payload_references.iter().enumerate() {
        let at = entities.index(index);
        for reference in references {
            resolve(&at.key("data"), entity, *reference)?;
        }
    }

    for (index, artifact) in doc.artifacts.iter().enumerate() {
        let at = artifacts.index(index);
        record_provenance(&at.key("provenance"), &artifact.id, &artifact.provenance)?;
        resolve_kind(
            &artifact.kind,
            KindUse::Artifact,
            doc,
            registry,
            &at.key("kind"),
        )?;
        if let Some(content) = &artifact.content {
            provenance(
                &at.key("content").key("provenance"),
                &artifact.id,
                content.provenance,
            )?;
        }
    }

    // Rule 4.
    for (index, link) in doc.links.iter().enumerate() {
        let at = links.index(index);
        record_provenance(&at.key("provenance"), &link.id, &link.provenance)?;
        let codec = resolve_kind(&link.kind, KindUse::Link, doc, registry, &at.key("kind"))?;
        let rule = codec
            .link_rules()
            .iter()
            .find(|rule| rule.name == link.kind.name())
            .expect("resolve_kind found the link rule");
        for (end, reference, allowed) in [("from", link.from, rule.from), ("to", link.to, rule.to)]
        {
            if !allowed.contains(&reference.record_type()) {
                return fail(
                    Rule::LinkEndpoints,
                    &at.key(end),
                    format_args!(
                        "link `{}` of kind `{}` does not allow {:?} -> {:?}",
                        link.id,
                        link.kind.as_str(),
                        link.from.record_type(),
                        link.to.record_type()
                    ),
                );
            }
        }
        resolve(&at.key("from"), &link.id, link.from)?;
        resolve(&at.key("to"), &link.id, link.to)?;
    }

    for (p, plan) in doc.plans.iter().enumerate() {
        let plan_at = plans.index(p);
        let tasks = plan_at.key("tasks");
        record_provenance(&plan_at.key("provenance"), &plan.id, &plan.provenance)?;
        if let PlanStatus::Approved { by } | PlanStatus::Archived { by } = &plan.status {
            user_decision(&plan_at.key("status").key("by"), &plan.id, *by)?;
        }
        let local: BTreeSet<TaskId> = plan.tasks.iter().map(|t| t.id).collect();
        for (t, task) in plan.tasks.iter().enumerate() {
            let at = tasks.index(t);
            let id = &task.id;
            resolve_kind(
                &task.action,
                KindUse::Action,
                doc,
                registry,
                &at.key("action"),
            )?;
            if let TaskDisposition::Skipped { by } | TaskDisposition::Cancelled { by } =
                &task.disposition
            {
                user_decision(&at.key("disposition").key("by"), id, *by)?;
            }
            for (i, dependency) in task.depends_on.iter().enumerate() {
                if !local.contains(dependency) {
                    return fail(
                        Rule::TaskDependencies,
                        &at.key("depends_on").index(i),
                        format_args!(
                            "task `{id}` depends on `{dependency}`, which is not in its plan"
                        ),
                    );
                }
            }
            for (i, input) in task.inputs.iter().enumerate() {
                resolve(&at.key("inputs").index(i), id, *input)?;
            }
            for (i, output) in task.expected_outputs.iter().enumerate() {
                resolve(
                    &at.key("expected_outputs").index(i),
                    id,
                    RecordRef::Artifact(*output),
                )?;
            }
            // Rule 6.
            let addresses = at.key("addresses");
            for (i, addressed) in task.addresses.iter().enumerate() {
                resolve(&addresses.index(i), id, RecordRef::Requirement(*addressed))?;
                let requirement = doc
                    .requirements
                    .iter()
                    .find(|r| r.id == *addressed)
                    .expect("resolved above");
                if !matches!(requirement.status, RequirementStatus::Confirmed { .. }) {
                    return fail(
                        Rule::AddressesConfirmed,
                        &addresses.index(i),
                        format_args!("task `{id}` addresses `{addressed}`, which is not confirmed"),
                    );
                }
            }
        }
        check_acyclic(plan, &tasks)?;
    }

    // Rule 8.
    for (index, validation) in doc.validations.iter().enumerate() {
        let at = validations.index(index);
        let id = &validation.id;
        resolve_kind(
            &validation.check,
            KindUse::Check,
            doc,
            registry,
            &at.key("check"),
        )?;
        match provenance(&at.key("provenance"), id, validation.provenance)? {
            Actor::Tool { .. } | Actor::System { .. } => {}
            _ => {
                return fail(
                    Rule::ValidationActor,
                    &at.key("provenance"),
                    format_args!("validation result `{id}` must come from a tool or the system"),
                );
            }
        }
        for (i, subject) in validation.subjects.iter().enumerate() {
            let subject_at = at.key("subjects");
            resolve(
                &subject_at.index(i).key("artifact"),
                id,
                RecordRef::Artifact(subject.artifact),
            )?;
        }
        for (i, requirement) in validation.requirements.iter().enumerate() {
            resolve(
                &at.key("requirements").index(i),
                id,
                RecordRef::Requirement(*requirement),
            )?;
        }
        if validation.diagnostics.len() > MAX_DIAGNOSTICS {
            return fail(
                Rule::Limit,
                &at.key("diagnostics"),
                format_args!(
                    "validation result `{id}` has {} diagnostics; the limit is {MAX_DIAGNOSTICS}",
                    validation.diagnostics.len()
                ),
            );
        }
    }

    // References from provenance records (`inputs`, task-run activities) are
    // historical and intentionally not resolved. Unreferenced provenance
    // records are kept, because provenance is append-only.

    // Rule 13, after the core rules, so that domain rules can rely on them.
    for entry in &doc.domains {
        let codec = registry
            .get(entry.id.as_str())
            .expect("the version check found every listed domain");
        if let Err(error) = codec.check_project(doc) {
            let (path, message) = match error {
                crate::domain::DomainError::Invalid { path, message } => (path, message),
                other => (FieldPath::root(), other.to_string()),
            };
            return Err(InvariantError {
                rule: Rule::DomainRule,
                path,
                message: format!("domain `{}`: {message}", entry.id.as_str()),
            });
        }
    }
    Ok(())
}

/// Rule 5: the dependency graph of a plan has no cycles.
fn check_acyclic(plan: &crate::model::Plan, tasks: &Loc<'_>) -> Result<(), InvariantError> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        InProgress,
        Done,
    }
    let dependencies: BTreeMap<TaskId, &[TaskId]> = plan
        .tasks
        .iter()
        .map(|t| (t.id, t.depends_on.as_slice()))
        .collect();
    let mut state: BTreeMap<TaskId, State> = BTreeMap::new();
    let cycle = |task: TaskId| {
        let index = plan
            .tasks
            .iter()
            .position(|t| t.id == task)
            .expect("a task of the plan");
        fail(
            Rule::TaskDependencies,
            &tasks.index(index).key("depends_on"),
            format_args!("plan `{}` has a dependency cycle through `{task}`", plan.id),
        )
    };
    for &start in dependencies.keys() {
        // Iterative depth-first search; `true` marks the exit of a task.
        let mut stack = vec![(start, false)];
        while let Some((task, exiting)) = stack.pop() {
            if exiting {
                state.insert(task, State::Done);
                continue;
            }
            match state.get(&task) {
                Some(State::Done) => continue,
                Some(State::InProgress) => return cycle(task),
                None => {}
            }
            state.insert(task, State::InProgress);
            stack.push((task, true));
            for &dependency in dependencies[&task] {
                match state.get(&dependency) {
                    Some(State::InProgress) => return cycle(dependency),
                    Some(State::Done) => {}
                    None => stack.push((dependency, false)),
                }
            }
        }
    }
    Ok(())
}
