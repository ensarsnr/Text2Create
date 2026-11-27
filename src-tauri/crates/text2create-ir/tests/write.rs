//! The write path: operations, permissions, temporary references, atomicity,
//! provenance, and determinism. Every rejection is checked to leave the
//! project byte-identical (see `Harness::apply_set`).

mod common;

use common::{ESP32, Harness, changes, digest, id, load, model, read, system, text, tool, user};
use serde_json::{Value, json};
use text2create_ir::change::{ChangeSet, TempId};
use text2create_ir::ids::{RecordRef, RequirementId};
use text2create_ir::model::{
    Activity, Actor, DecisionStatus, InputRef, PlanStatus, RequirementStatus, TaskDisposition,
};
use text2create_ir::source::Source;
use text2create_ir::{Project, Rule, WriteEnv, WriteErrorKind};

fn temp(applied: &text2create_ir::Applied, name: &str) -> RecordRef {
    applied.temps[&TempId::try_from(name.to_owned()).unwrap()]
}

fn new_requirement(temp: &str) -> Value {
    json!({
        "op": "add_requirement", "temp": temp, "kind": "functional", "priority": "must",
        "title": "Report lists every bracket", "statement": "The report lists every bracket.",
        "origin": {"type": "inferred", "rationale": "Completeness."}, "acceptance": []
    })
}

fn edit_requirement(requirement: Value) -> Value {
    json!({
        "op": "edit_requirement", "requirement": requirement, "kind": "constraint",
        "priority": "should", "title": "Edited title", "statement": "Edited statement.",
        "origin": {"type": "user", "quote": "Edit it."}, "acceptance": ["Edited."]
    })
}

fn by_id(prefix: &str, n: u32) -> Value {
    json!({"id": id(prefix, n)})
}

fn by_temp(name: &str) -> Value {
    json!({"temp": name})
}

fn kind(error: text2create_ir::WriteError) -> WriteErrorKind {
    error.kind
}

fn rule(error: text2create_ir::WriteError) -> Rule {
    match error.kind {
        WriteErrorKind::Invariant(error) => error.rule,
        other => panic!("expected an invariant error, got {other}"),
    }
}

// ------------------------------------------------------------ creation

#[test]
fn create_allocates_identifier_timestamp_and_provenance() {
    let mut h = Harness::example();
    let mut env = WriteEnv {
        registry: &h.registry,
        clock: &mut h.clock,
        ids: &mut h.ids,
    };
    let (project, provenance) =
        Project::create(&user(), text("New project"), None, &mut env).unwrap();
    let document = project.document();
    assert_eq!(document.provenance.len(), 1);
    assert_eq!(document.provenance[0].id, provenance);
    assert_eq!(
        document.provenance[0].at.as_str(),
        "2026-11-28T00:00:00.000Z"
    );
    assert_eq!(document.project.provenance.created, provenance);
    assert_eq!(
        document.project.id.to_string(),
        "prj_01a21c90-0000-7000-8000-000001000002"
    );
    let reloaded = Project::load(project.to_canonical_json().as_bytes(), &h.registry).unwrap();
    assert_eq!(reloaded, project);
}

// ----------------------------------------------------- provenance records

#[test]
fn each_change_set_creates_exactly_one_core_allocated_provenance_record() {
    let mut h = Harness::example();
    let before = h.project.document().provenance.clone();
    let applied = h.ok(
        &model(),
        json!([new_requirement("a"), new_requirement("b"),
        {"op": "add_link", "kind": "core.refines", "from": by_temp("a"), "to": by_temp("b")}]),
    );
    let document = h.project.document();
    assert_eq!(document.provenance.len(), before.len() + 1);
    // Existing provenance records are unchanged.
    for record in &before {
        assert!(document.provenance.contains(record));
    }
    let record = document
        .provenance
        .iter()
        .find(|p| p.id == applied.provenance)
        .unwrap();
    assert_eq!(
        record.id.to_string(),
        "prv_01a21c90-0000-7000-8000-000001000001"
    );
    assert_eq!(record.at.as_str(), "2026-11-28T00:00:00.000Z");
    assert!(matches!(record.actor, Actor::Model { .. }));
    assert_eq!(record.activity, Activity::Interpretation {});
    assert!(record.model.is_some());
    // Every record created by the change set refers to that provenance record.
    let RecordRef::Requirement(a) = temp(&applied, "a") else {
        panic!()
    };
    let requirement = document.requirements.iter().find(|r| r.id == a).unwrap();
    assert_eq!(requirement.provenance.rev, 1);
    assert_eq!(requirement.provenance.created, applied.provenance);
    assert_eq!(requirement.provenance.modified, applied.provenance);
    assert_eq!(requirement.status, RequirementStatus::Proposed {});
}

#[test]
fn the_write_path_never_changes_or_removes_provenance_records() {
    let mut h = Harness::example();
    let mut history = h.project.document().provenance.clone();
    for _ in 0..3 {
        let applied = h.ok(
            &user(),
            json!([{"op": "set_project_info", "name": "Renamed", "description": null}]),
        );
        let current = &h.project.document().provenance;
        for record in &history {
            assert!(
                current.contains(record),
                "provenance record {} changed or removed",
                record.id
            );
        }
        history.push(
            current
                .iter()
                .find(|p| p.id == applied.provenance)
                .unwrap()
                .clone(),
        );
    }
    assert_eq!(h.project.document().provenance.len(), history.len());
}

#[test]
fn inputs_are_recorded_and_must_exist() {
    let mut h = Harness::example();
    let mut set =
        changes(json!([{"op": "set_project_info", "name": "Derived", "description": null}]));
    set.inputs = vec![InputRef {
        record: id("art", 31).parse().unwrap(),
        rev: Some(1),
        digest: None,
    }];
    let applied = h.apply_set(&model(), &set).unwrap();
    let record = h
        .project
        .document()
        .provenance
        .iter()
        .find(|p| p.id == applied.provenance)
        .unwrap();
    assert_eq!(record.inputs, set.inputs);
    set.inputs[0].record = id("art", 99).parse().unwrap();
    assert_eq!(
        kind(h.apply_set(&model(), &set).unwrap_err()),
        WriteErrorKind::NotFound(id("art", 99))
    );
}

#[test]
fn task_run_sources_must_reference_an_existing_task() {
    let mut h = Harness::example();
    let source = Source::tool_task_run(
        text("example-tool"),
        text("1"),
        id("tsk", 99).parse().unwrap(),
        id("run", 99).parse().unwrap(),
    );
    let error = h.rejected(
        &source,
        json!([{"op": "set_project_info", "name": "x", "description": null}]),
    );
    assert_eq!(kind(error), WriteErrorKind::NotFound(id("tsk", 99)));
    let applied = h.ok(
        &tool(),
        json!([{"op": "record_artifact_content", "artifact": by_id("art", 33),
        "digest": digest('c'), "size_bytes": 10}]),
    );
    let record = h
        .project
        .document()
        .provenance
        .iter()
        .find(|p| p.id == applied.provenance)
        .unwrap();
    assert!(matches!(record.activity, Activity::TaskRun { .. }));
}

#[test]
fn empty_change_sets_are_rejected() {
    let mut h = Harness::example();
    assert_eq!(
        kind(h.rejected(&user(), json!([]))),
        WriteErrorKind::EmptyChangeSet
    );
}

// ----------------------------------------------------------- operations

#[test]
fn set_project_info_bumps_the_project_revision_once() {
    let mut h = Harness::example();
    let applied = h.ok(
        &user(),
        json!([
            {"op": "set_project_info", "name": "First", "description": "D"},
            {"op": "set_project_info", "name": "Second", "description": null}
        ]),
    );
    let project = &h.project.document().project;
    assert_eq!(project.name.as_str(), "Second");
    assert_eq!(project.description, None);
    assert_eq!(project.provenance.rev, 2);
    assert_eq!(project.provenance.modified, applied.provenance);
}

#[test]
fn declare_domain_uses_the_version_of_this_build() {
    let mut h = Harness::new(load(&read(ESP32)).unwrap());
    h.ok(
        &user(),
        json!([{"op": "declare_domain", "domain": "example"},
        {"op": "add_entity", "kind": "example.widget", "name": "W",
         "data": {"label": "W", "size": 1, "tolerance": null}}]),
    );
    let domains = &h.project.document().domains;
    assert_eq!((domains[0].id.as_str(), domains[0].version), ("example", 1));
    assert_eq!(
        kind(h.rejected(
            &user(),
            json!([{"op": "declare_domain", "domain": "example"}])
        )),
        WriteErrorKind::DomainAlreadyDeclared("example".to_owned().try_into().unwrap())
    );
    for domain in ["core", "robotics"] {
        assert_eq!(
            kind(h.rejected(&user(), json!([{"op": "declare_domain", "domain": domain}]))),
            WriteErrorKind::UnknownDomain(domain.to_owned())
        );
    }
}

#[test]
fn requirements_can_be_added_edited_and_decided() {
    let mut h = Harness::example();
    // A model may edit a proposed requirement.
    let applied = h.ok(&model(), json!([edit_requirement(by_id("req", 14))]));
    let r = &h.project.document().requirements[3];
    assert_eq!(r.title.as_str(), "Edited title");
    assert_eq!(
        (r.provenance.rev, r.provenance.modified),
        (2, applied.provenance)
    );
    // The user confirms it; the decision references the user's provenance record.
    let applied = h.ok(&user(), json!([{"op": "decide_requirement", "requirement": by_id("req", 14), "decision": "confirm"}]));
    let r = &h.project.document().requirements[3];
    assert_eq!(
        r.status,
        RequirementStatus::Confirmed {
            by: applied.provenance
        }
    );
    assert_eq!(r.provenance.rev, 3);
    // The user may edit a decided requirement; the decision stays.
    h.ok(&user(), json!([edit_requirement(by_id("req", 11))]));
    assert!(matches!(
        h.project.document().requirements[0].status,
        RequirementStatus::Confirmed { .. }
    ));
}

#[test]
fn decided_records_can_only_be_edited_by_the_user() {
    let mut h = Harness::example();
    for source in [model(), tool(), system()] {
        assert!(matches!(
            kind(h.rejected(&source, json!([edit_requirement(by_id("req", 11))]))),
            WriteErrorKind::NotPermitted(_)
        ));
        assert!(matches!(
            kind(h.rejected(
                &source,
                json!([{"op": "edit_entity", "entity": by_id("ent", 21), "name": "X",
                "data": {"label": "X", "size": 1, "tolerance": null}}])
            )),
            WriteErrorKind::NotPermitted(_)
        ));
    }
    assert!(matches!(
        kind(h.rejected(&user(), json!([edit_requirement(by_id("req", 13))]))),
        WriteErrorKind::InvalidTransition(_)
    ));
}

#[test]
fn superseding_requirements() {
    let mut h = Harness::example();
    let applied = h.ok(&user(), json!([new_requirement("n"),
        {"op": "supersede_requirement", "requirement": by_id("req", 14), "replacement": by_temp("n")}]));
    let RecordRef::Requirement(n) = temp(&applied, "n") else {
        panic!()
    };
    let superseded = h
        .project
        .document()
        .requirements
        .iter()
        .find(|r| r.id.to_string() == id("req", 14))
        .unwrap();
    assert_eq!(
        superseded.status,
        RequirementStatus::Superseded {
            replacement: n,
            by: applied.provenance
        }
    );
    for operation in [
        json!({"op": "supersede_requirement", "requirement": by_id("req", 11), "replacement": by_id("req", 11)}),
        json!({"op": "supersede_requirement", "requirement": by_id("req", 13), "replacement": by_id("req", 11)}),
        json!({"op": "decide_requirement", "requirement": by_id("req", 13), "decision": "confirm"}),
    ] {
        assert!(matches!(
            kind(h.rejected(&user(), json!([operation]))),
            WriteErrorKind::InvalidTransition(_)
        ));
    }
}

#[test]
fn invariants_hold_after_every_change_set() {
    let mut h = Harness::example();
    // req_11 is addressed by tasks; rejecting it would violate rule 6.
    let error = h.rejected(&user(), json!([{"op": "decide_requirement", "requirement": by_id("req", 11), "decision": "reject"}]));
    assert_eq!(rule(error), Rule::AddressesConfirmed);
    let error = h.rejected(
        &user(),
        json!([{"op": "add_link", "kind": "core.satisfies",
        "from": by_id("req", 14), "to": by_id("req", 11)}]),
    );
    assert_eq!(rule(error), Rule::LinkEndpoints);
    let error = h.rejected(
        &user(),
        json!([{"op": "add_artifact", "kind": "core.spreadsheet",
        "path": "a.csv", "ownership": "user"}]),
    );
    assert_eq!(rule(error), Rule::KindRegistered);
}

#[test]
fn entities_are_normalized_by_their_domain_codec() {
    let mut h = Harness::example();
    // The optional field is omitted; the codec writes it as `null`.
    let applied = h.ok(&model(), json!([{"op": "add_entity", "temp": "w", "kind": "example.widget",
        "name": "Spare bracket", "data": {"size": 3, "label": "Spare"}},
        {"op": "add_link", "kind": "core.satisfies", "from": by_temp("w"), "to": by_id("req", 11)}]));
    let RecordRef::Entity(w) = temp(&applied, "w") else {
        panic!()
    };
    let entity = h
        .project
        .document()
        .entities
        .iter()
        .find(|e| e.id == w)
        .unwrap();
    assert_eq!(
        entity.data.as_value(),
        &json!({"label": "Spare", "size": 3, "tolerance": null})
    );
    assert_eq!(entity.status, DecisionStatus::Proposed {});
    assert!(matches!(
        kind(h.rejected(
            &model(),
            json!([{"op": "add_entity", "kind": "example.widget", "name": "X",
            "data": {"label": "X", "size": 1, "tolerance": null, "color": "red"}}])
        )),
        WriteErrorKind::Payload(_)
    ));
    assert!(matches!(
        kind(h.rejected(
            &model(),
            json!([{"op": "add_entity", "kind": "example.assembly", "name": "X",
            "data": {"parts": [], "note": null}}])
        )),
        WriteErrorKind::Payload(_)
    ));
    let error = h.rejected(
        &model(),
        json!([{"op": "add_entity", "kind": "example.gadget", "name": "X", "data": {}}]),
    );
    assert_eq!(rule(error), Rule::KindRegistered);
    // References inside payloads must resolve.
    let error = h.rejected(
        &model(),
        json!([{"op": "add_entity", "kind": "example.assembly", "name": "X",
        "data": {"parts": [id("ent", 21), id("ent", 99)], "note": null}}]),
    );
    assert_eq!(rule(error), Rule::ReferenceResolves);
}

#[test]
fn entities_can_be_edited_and_decided() {
    let mut h = Harness::example();
    h.ok(
        &model(),
        json!([{"op": "edit_entity", "entity": by_id("ent", 22), "name": "Right bracket v2",
        "data": {"label": "Right", "size": 13, "tolerance": "0.1"}}]),
    );
    let applied = h.ok(
        &user(),
        json!([{"op": "decide_entity", "entity": by_id("ent", 22), "decision": "accept"}]),
    );
    let entity = &h.project.document().entities[1];
    assert_eq!(
        entity.status,
        DecisionStatus::Accepted {
            by: applied.provenance
        }
    );
    assert_eq!(entity.provenance.rev, 3);
}

#[test]
fn artifacts_and_their_content() {
    let mut h = Harness::example();
    let applied = h.ok(&model(), json!([{"op": "add_artifact", "temp": "a", "kind": "example.report",
        "path": "out/new.txt", "ownership": "generated"},
        {"op": "add_link", "kind": "core.describes", "from": by_temp("a"), "to": by_id("ent", 21)}]));
    let RecordRef::Artifact(a) = temp(&applied, "a") else {
        panic!()
    };
    assert!(
        h.project
            .document()
            .artifacts
            .iter()
            .find(|x| x.id == a)
            .unwrap()
            .content
            .is_none()
    );
    let applied = h.ok(
        &tool(),
        json!([{"op": "record_artifact_content", "artifact": {"id": a.to_string()},
        "digest": digest('d'), "size_bytes": 42}]),
    );
    let artifact = h
        .project
        .document()
        .artifacts
        .iter()
        .find(|x| x.id == a)
        .unwrap();
    let content = artifact.content.as_ref().unwrap();
    assert_eq!(
        (content.size_bytes, content.provenance),
        (42, applied.provenance)
    );
    assert_eq!(artifact.provenance.rev, 2);
    // Invalid scalars are rejected while the change set is parsed.
    let escape = json!({"inputs": [], "operations": [{"op": "add_artifact", "kind": "core.document",
        "path": "../escape.md", "ownership": "user"}]});
    assert!(ChangeSet::from_json(escape.to_string().as_bytes()).is_err());
}

#[test]
fn plans_and_tasks() {
    let mut h = Harness::example();
    // Dependencies may refer to tasks listed later in the same plan.
    let applied = h.ok(&model(), json!([{"op": "add_plan", "goal": "Rebuild", "tasks": [
        {"title": "Inspect", "action": "example.inspect", "depends_on": [by_temp("t1")],
         "inputs": [], "expected_outputs": [], "addresses": [by_id("req", 11)]},
        {"temp": "t1", "title": "Build", "action": "example.build", "depends_on": [],
         "inputs": [by_id("ent", 21), by_id("art", 31)], "expected_outputs": [by_id("art", 32)], "addresses": []}
    ]}, {"op": "add_link", "kind": "core.derived_from", "from": by_id("req", 14), "to": by_id("req", 11)}]));
    let plan = h
        .project
        .document()
        .plans
        .iter()
        .find(|x| x.goal.as_str() == "Rebuild")
        .unwrap();
    assert_eq!(plan.provenance.created, applied.provenance);
    assert_eq!(plan.status, PlanStatus::Draft {});
    assert!(plan.tasks.windows(2).all(|w| w[0].id < w[1].id));
    assert!(
        plan.tasks
            .iter()
            .all(|t| t.disposition == TaskDisposition::Active {})
    );
    // The task temp was resolved; an unused one would have been rejected.
    assert_eq!(applied.temps.len(), 1);

    let error = h.rejected(&model(), json!([{"op": "add_plan", "goal": "Cycle", "tasks": [
        {"temp": "a", "title": "A", "action": "example.build", "depends_on": [by_temp("b")], "inputs": [], "expected_outputs": [], "addresses": []},
        {"temp": "b", "title": "B", "action": "example.build", "depends_on": [by_temp("a")], "inputs": [], "expected_outputs": [], "addresses": []}
    ]}]));
    assert_eq!(rule(error), Rule::TaskDependencies);
    let error = h.rejected(&model(), json!([{"op": "add_plan", "goal": "Proposed", "tasks": [
        {"title": "A", "action": "example.build", "depends_on": [], "inputs": [], "expected_outputs": [], "addresses": [by_id("req", 14)]}
    ]}]));
    assert_eq!(rule(error), Rule::AddressesConfirmed);
}

#[test]
fn plan_decisions_follow_their_lifecycle() {
    let mut h = Harness::example();
    let applied = h.ok(
        &user(),
        json!([{"op": "decide_plan", "plan": by_id("pln", 53), "decision": "approve"}]),
    );
    assert_eq!(
        h.project.document().plans[2].status,
        PlanStatus::Approved {
            by: applied.provenance
        }
    );
    for (plan, decision) in [(53, "approve"), (52, "archive"), (52, "approve")] {
        assert!(matches!(
            kind(h.rejected(
                &user(),
                json!([{"op": "decide_plan", "plan": by_id("pln", plan), "decision": decision}])
            )),
            WriteErrorKind::InvalidTransition(_)
        ));
    }
    h.ok(
        &user(),
        json!([{"op": "decide_plan", "plan": by_id("pln", 53), "decision": "archive"}]),
    );
}

#[test]
fn task_dispositions() {
    let mut h = Harness::example();
    let applied = h.ok(
        &user(),
        json!([{"op": "set_task_disposition", "task": by_id("tsk", 61), "disposition": "skip"}]),
    );
    let plan = &h.project.document().plans[0];
    assert_eq!(
        plan.tasks[0].disposition,
        TaskDisposition::Skipped {
            by: applied.provenance
        }
    );
    assert_eq!(
        (plan.provenance.rev, plan.provenance.modified),
        (3, applied.provenance)
    );
    for task in [61, 63, 64] {
        // Already skipped, already skipped, and part of an archived plan.
        assert!(matches!(
            kind(h.rejected(&user(), json!([{"op": "set_task_disposition", "task": by_id("tsk", task), "disposition": "cancel"}]))),
            WriteErrorKind::InvalidTransition(_)
        ));
    }
}

#[test]
fn validation_results_from_tools_and_the_system() {
    let mut h = Harness::example();
    for source in [tool(), system()] {
        let applied = h.ok(&source, json!([{"op": "record_validation", "temp": "v", "check": "example.inspect",
            "subjects": [{"artifact": by_id("art", 32), "digest": digest('e')}], "requirements": [by_id("req", 11)],
            "outcome": "passed", "summary": "All good.", "diagnostics": []},
            {"op": "add_link", "kind": "example.verifies", "from": by_temp("v"), "to": by_id("req", 11)}]));
        let RecordRef::Validation(v) = temp(&applied, "v") else {
            panic!()
        };
        let result = h
            .project
            .document()
            .validations
            .iter()
            .find(|x| x.id == v)
            .unwrap();
        assert_eq!(result.provenance, applied.provenance);
    }
}

// ------------------------------------------- model-attributed change sets

#[test]
fn model_change_sets_cannot_make_user_decisions_or_record_validation_results() {
    let mut h = Harness::example();
    let operations = [
        json!({"op": "decide_requirement", "requirement": by_id("req", 14), "decision": "confirm"}),
        json!({"op": "decide_requirement", "requirement": by_id("req", 14), "decision": "reject"}),
        json!({"op": "supersede_requirement", "requirement": by_id("req", 14), "replacement": by_id("req", 11)}),
        json!({"op": "decide_entity", "entity": by_id("ent", 22), "decision": "accept"}),
        json!({"op": "decide_entity", "entity": by_id("ent", 22), "decision": "reject"}),
        json!({"op": "decide_plan", "plan": by_id("pln", 53), "decision": "approve"}),
        json!({"op": "decide_plan", "plan": by_id("pln", 53), "decision": "archive"}),
        json!({"op": "set_task_disposition", "task": by_id("tsk", 62), "disposition": "skip"}),
        json!({"op": "record_validation", "check": "example.inspect", "subjects": [], "requirements": [],
               "outcome": "passed", "summary": "Claimed.", "diagnostics": []}),
    ];
    for operation in operations {
        for source in [
            model(),
            Source::model_task_run(
                text("ollama"),
                text("m"),
                text2create_ir::model::ModelInvocation {
                    prompt_digest: digest('a'),
                    response_digest: digest('b'),
                },
                id("tsk", 61).parse().unwrap(),
                id("run", 80).parse().unwrap(),
            ),
        ] {
            let error = h.rejected(&source, json!([operation.clone()]));
            assert!(
                matches!(error.kind, WriteErrorKind::NotPermitted(_)),
                "{operation}: {error}"
            );
        }
    }
    // Tools and the system cannot make user decisions either; the user
    // cannot record validation results.
    for source in [tool(), system()] {
        let error = h.rejected(&source, json!([{"op": "decide_requirement", "requirement": by_id("req", 14), "decision": "confirm"}]));
        assert!(matches!(error.kind, WriteErrorKind::NotPermitted(_)));
    }
    let error = h.rejected(
        &user(),
        json!([{"op": "record_validation", "check": "example.inspect", "subjects": [],
        "requirements": [], "outcome": "passed", "summary": "Claimed.", "diagnostics": []}]),
    );
    assert!(matches!(error.kind, WriteErrorKind::NotPermitted(_)));
}

// ---------------------------------------------------- temporary references

#[test]
fn temporary_references() {
    let mut h = Harness::example();
    let refine = |from: &str, to: &str| json!({"op": "add_link", "kind": "core.refines", "from": by_temp(from), "to": by_temp(to)});
    // Unknown, including a reference before the declaration.
    assert_eq!(
        kind(h.rejected(
            &model(),
            json!([refine("a", "b"), new_requirement("a"), new_requirement("b")])
        )),
        WriteErrorKind::UnknownTemp(TempId::try_from("a".to_owned()).unwrap())
    );
    // Duplicate.
    assert_eq!(
        kind(h.rejected(
            &model(),
            json!([new_requirement("a"), new_requirement("a"), refine("a", "a")])
        )),
        WriteErrorKind::DuplicateTemp(TempId::try_from("a".to_owned()).unwrap())
    );
    // Unused.
    assert_eq!(
        kind(h.rejected(&model(), json!([new_requirement("a")]))),
        WriteErrorKind::UnusedTemp(TempId::try_from("a".to_owned()).unwrap())
    );
    // Wrong record type.
    assert!(matches!(
        kind(h.rejected(&model(), json!([{"op": "add_artifact", "temp": "x", "kind": "core.document",
            "path": "x.md", "ownership": "user"}, {"op": "edit_requirement", "requirement": by_temp("x"),
            "kind": "functional", "priority": "must", "title": "T", "statement": "S.",
            "origin": {"type": "inferred", "rationale": "R."}, "acceptance": []}]))),
        WriteErrorKind::TempTypeMismatch { .. }
    ));
    // Unknown existing identifiers.
    assert_eq!(
        kind(h.rejected(
            &user(),
            json!([{"op": "decide_entity", "entity": by_id("ent", 99), "decision": "accept"}])
        )),
        WriteErrorKind::NotFound(id("ent", 99))
    );
    // Resolved within the change set.
    let applied = h.ok(
        &model(),
        json!([new_requirement("a"), new_requirement("b"), refine("a", "b")]),
    );
    assert_eq!(applied.temps.len(), 2);
}

// ------------------------------------------------------------ atomicity

#[test]
fn a_failing_operation_discards_all_earlier_operations() {
    let mut h = Harness::example();
    let error = h.rejected(&model(), json!([
        {"op": "set_project_info", "name": "Changed", "description": null},
        new_requirement("a"),
        {"op": "add_link", "kind": "core.refines", "from": by_temp("a"), "to": by_id("req", 11)},
        {"op": "decide_requirement", "requirement": by_id("req", 14), "decision": "confirm"}
    ]));
    assert_eq!(error.operation, Some(3));
    assert_eq!(
        h.project.document().project.name.as_str(),
        "Example domain project"
    );
}

// ----------------------------------------------------------- determinism

#[test]
fn fixed_clock_and_identifiers_produce_byte_identical_results() {
    let sets: Vec<(Source, Value)> = vec![
        (
            model(),
            json!([new_requirement("a"), {"op": "add_entity", "temp": "e", "kind": "example.widget",
            "name": "W", "data": {"label": "W", "size": 2, "tolerance": "0.5"}},
            {"op": "add_link", "kind": "core.satisfies", "from": by_temp("e"), "to": by_temp("a")}]),
        ),
        (
            user(),
            json!([{"op": "decide_requirement", "requirement": by_id("req", 14), "decision": "confirm"}]),
        ),
        (
            tool(),
            json!([{"op": "record_artifact_content", "artifact": by_id("art", 33), "digest": digest('f'), "size_bytes": 7}]),
        ),
    ];
    let run = || {
        let mut h = Harness::example();
        for (source, operations) in &sets {
            h.ok(source, operations.clone());
        }
        h.canonical()
    };
    let first = run();
    assert_eq!(first, run());
    assert_ne!(first, String::from_utf8(read(common::EXAMPLE)).unwrap());
}

#[test]
fn change_sets_serialize_for_the_journal() {
    let set = changes(json!([new_requirement("a"),
        {"op": "add_link", "kind": "core.refines", "from": by_temp("a"), "to": by_id("req", 11)}]));
    let json = serde_json::to_vec(&set).unwrap();
    assert_eq!(ChangeSet::from_json(&json).unwrap(), set);
    let _: RequirementId = id("req", 11).parse().unwrap();
}
