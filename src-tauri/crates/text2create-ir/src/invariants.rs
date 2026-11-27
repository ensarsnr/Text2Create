//! The invariants of a loaded project (`docs/PROJECT_IR.md`, "Invariants").

use crate::domain::{CORE_DOMAIN, DomainCodec, DomainRegistry};
use crate::ids::{EntityId, ProvenanceId, RecordRef, TaskId};
use crate::model::{
    Actor, DecisionStatus, MAX_DIAGNOSTICS, PlanStatus, ProjectDocument, RecordProvenance,
    RequirementStatus, TaskDisposition,
};
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
}

/// A loaded document violates an invariant.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invariant violated ({rule:?}): {message}")]
pub struct InvariantError {
    pub rule: Rule,
    pub message: String,
}

fn fail<T>(rule: Rule, message: impl fmt::Display) -> Result<T, InvariantError> {
    Err(InvariantError {
        rule,
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
/// by a domain that is `core` or listed in `domains` (rule 3).
pub(crate) fn resolve_kind<'r>(
    kind: &Kind,
    use_: KindUse,
    doc: &ProjectDocument,
    registry: &'r DomainRegistry,
) -> Result<&'r dyn DomainCodec, InvariantError> {
    let domain = kind.domain();
    let declared = domain == CORE_DOMAIN || doc.domains.iter().any(|d| d.id.as_str() == domain);
    let Some(codec) = registry.get(domain).filter(|_| declared) else {
        return fail(
            Rule::KindRegistered,
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
            format_args!(
                "kind `{}` is not registered as a {use_:?} kind",
                kind.as_str()
            ),
        );
    }
    Ok(codec)
}

fn sorted_unique<T, K: Ord + fmt::Display>(
    items: &[T],
    key: impl Fn(&T) -> K,
    what: &str,
    rule: Rule,
) -> Result<(), InvariantError> {
    for pair in items.windows(2) {
        let (a, b) = (key(&pair[0]), key(&pair[1]));
        if a == b {
            return fail(rule, format_args!("duplicate {what} `{a}`"));
        }
        if a > b {
            return fail(rule, format_args!("{what} `{b}` is not sorted after `{a}`"));
        }
    }
    Ok(())
}

/// Checks every invariant. `payload_references` holds the record IDs that the
/// domain codecs reported for each entity payload.
pub(crate) fn check(
    doc: &ProjectDocument,
    registry: &DomainRegistry,
    payload_references: &[(EntityId, Vec<RecordRef>)],
) -> Result<(), InvariantError> {
    // Rule 1.
    sorted_unique(
        &doc.domains,
        |d| d.id.as_str().to_owned(),
        "domain",
        Rule::Domains,
    )?;
    if doc.domains.iter().any(|d| d.id.as_str() == CORE_DOMAIN) {
        return fail(Rule::Domains, "`core` is built in and must not be listed");
    }
    sorted_unique(
        &doc.provenance,
        |r| r.id,
        "provenance record",
        Rule::SortedUnique,
    )?;
    sorted_unique(
        &doc.requirements,
        |r| r.id,
        "requirement",
        Rule::SortedUnique,
    )?;
    sorted_unique(&doc.entities, |r| r.id, "entity", Rule::SortedUnique)?;
    sorted_unique(&doc.artifacts, |r| r.id, "artifact", Rule::SortedUnique)?;
    sorted_unique(&doc.links, |r| r.id, "link", Rule::SortedUnique)?;
    sorted_unique(&doc.plans, |r| r.id, "plan", Rule::SortedUnique)?;
    sorted_unique(
        &doc.validations,
        |r| r.id,
        "validation result",
        Rule::SortedUnique,
    )?;
    let mut task_ids = BTreeSet::new();
    for plan in &doc.plans {
        sorted_unique(&plan.tasks, |t| t.id, "task", Rule::SortedUnique)?;
        for task in &plan.tasks {
            if !task_ids.insert(task.id) {
                return fail(
                    Rule::SortedUnique,
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
    let resolve = |owner: &dyn fmt::Display, reference: RecordRef| {
        if records.contains(&reference) {
            Ok(())
        } else {
            fail(
                Rule::ReferenceResolves,
                format_args!("`{owner}` references missing record `{reference}`"),
            )
        }
    };

    // Rules 7, 9, and 10.
    let actors: BTreeMap<ProvenanceId, &Actor> =
        doc.provenance.iter().map(|p| (p.id, &p.actor)).collect();
    let provenance = |owner: &dyn fmt::Display, id: ProvenanceId| {
        actors.get(&id).copied().map_or_else(
            || {
                fail(
                    Rule::ProvenanceResolves,
                    format_args!("`{owner}` references missing provenance record `{id}`"),
                )
            },
            Ok,
        )
    };
    let record_provenance = |owner: &dyn fmt::Display, p: &RecordProvenance| {
        if p.rev == 0 {
            return fail(
                Rule::Revision,
                format_args!("`{owner}` has rev 0; rev starts at 1"),
            );
        }
        provenance(owner, p.created)?;
        provenance(owner, p.modified)?;
        Ok(())
    };
    let user_decision = |owner: &dyn fmt::Display, by: ProvenanceId| {
        if matches!(provenance(owner, by)?, Actor::User {}) {
            Ok(())
        } else {
            fail(
                Rule::UserDecision,
                format_args!(
                    "decision on `{owner}` references `{by}`, whose actor is not the user"
                ),
            )
        }
    };

    record_provenance(&doc.project.id, &doc.project.provenance)?;

    for requirement in &doc.requirements {
        let id = &requirement.id;
        record_provenance(id, &requirement.provenance)?;
        match &requirement.status {
            RequirementStatus::Proposed {} => {}
            RequirementStatus::Confirmed { by } | RequirementStatus::Rejected { by } => {
                user_decision(id, *by)?;
            }
            RequirementStatus::Superseded { replacement, by } => {
                user_decision(id, *by)?;
                resolve(id, RecordRef::Requirement(*replacement))?;
            }
        }
    }

    for entity in &doc.entities {
        record_provenance(&entity.id, &entity.provenance)?;
        resolve_kind(&entity.kind, KindUse::Entity, doc, registry)?;
        if let DecisionStatus::Accepted { by } | DecisionStatus::Rejected { by } = &entity.status {
            user_decision(&entity.id, *by)?;
        }
    }
    for (entity, references) in payload_references {
        for reference in references {
            resolve(entity, *reference)?;
        }
    }

    for artifact in &doc.artifacts {
        record_provenance(&artifact.id, &artifact.provenance)?;
        resolve_kind(&artifact.kind, KindUse::Artifact, doc, registry)?;
        if let Some(content) = &artifact.content {
            provenance(&artifact.id, content.provenance)?;
        }
    }

    // Rule 4.
    for link in &doc.links {
        record_provenance(&link.id, &link.provenance)?;
        let codec = resolve_kind(&link.kind, KindUse::Link, doc, registry)?;
        let rule = codec
            .link_rules()
            .iter()
            .find(|rule| rule.name == link.kind.name())
            .expect("resolve_kind found the link rule");
        if !rule.from.contains(&link.from.record_type())
            || !rule.to.contains(&link.to.record_type())
        {
            return fail(
                Rule::LinkEndpoints,
                format_args!(
                    "link `{}` of kind `{}` does not allow {:?} -> {:?}",
                    link.id,
                    link.kind.as_str(),
                    link.from.record_type(),
                    link.to.record_type()
                ),
            );
        }
        resolve(&link.id, link.from)?;
        resolve(&link.id, link.to)?;
    }

    for plan in &doc.plans {
        record_provenance(&plan.id, &plan.provenance)?;
        if let PlanStatus::Approved { by } | PlanStatus::Archived { by } = &plan.status {
            user_decision(&plan.id, *by)?;
        }
        let local: BTreeSet<TaskId> = plan.tasks.iter().map(|t| t.id).collect();
        for task in &plan.tasks {
            let id = &task.id;
            resolve_kind(&task.action, KindUse::Action, doc, registry)?;
            if let TaskDisposition::Skipped { by } | TaskDisposition::Cancelled { by } =
                &task.disposition
            {
                user_decision(id, *by)?;
            }
            for dependency in &task.depends_on {
                if !local.contains(dependency) {
                    return fail(
                        Rule::TaskDependencies,
                        format_args!(
                            "task `{id}` depends on `{dependency}`, which is not in its plan"
                        ),
                    );
                }
            }
            for input in &task.inputs {
                resolve(id, *input)?;
            }
            for output in &task.expected_outputs {
                resolve(id, RecordRef::Artifact(*output))?;
            }
            // Rule 6.
            for addressed in &task.addresses {
                resolve(id, RecordRef::Requirement(*addressed))?;
                let requirement = doc
                    .requirements
                    .iter()
                    .find(|r| r.id == *addressed)
                    .expect("resolved above");
                if !matches!(requirement.status, RequirementStatus::Confirmed { .. }) {
                    return fail(
                        Rule::AddressesConfirmed,
                        format_args!("task `{id}` addresses `{addressed}`, which is not confirmed"),
                    );
                }
            }
        }
        check_acyclic(plan)?;
    }

    // Rule 8.
    for validation in &doc.validations {
        let id = &validation.id;
        resolve_kind(&validation.check, KindUse::Check, doc, registry)?;
        match provenance(id, validation.provenance)? {
            Actor::Tool { .. } | Actor::System { .. } => {}
            _ => {
                return fail(
                    Rule::ValidationActor,
                    format_args!("validation result `{id}` must come from a tool or the system"),
                );
            }
        }
        for subject in &validation.subjects {
            resolve(id, RecordRef::Artifact(subject.artifact))?;
        }
        for requirement in &validation.requirements {
            resolve(id, RecordRef::Requirement(*requirement))?;
        }
        if validation.diagnostics.len() > MAX_DIAGNOSTICS {
            return fail(
                Rule::Limit,
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
    Ok(())
}

/// Rule 5: the dependency graph of a plan has no cycles.
fn check_acyclic(plan: &crate::model::Plan) -> Result<(), InvariantError> {
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
        fail(
            Rule::TaskDependencies,
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
