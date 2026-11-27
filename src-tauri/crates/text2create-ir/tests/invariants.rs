//! A negative test for every invariant in the specification, and the
//! documented exceptions (historical provenance references, unreferenced
//! provenance records, task-run references).

mod common;

use common::{EXAMPLE, assert_rule, id, load, mutate, read};
use serde_json::{Value, json};
use text2create_ir::Rule;
use text2create_ir::model::MAX_DIAGNOSTICS;

// Provenance records in the example fixture.
const MODEL: u32 = 2;
const USER: u32 = 3;
const TOOL: u32 = 5;
const SYSTEM: u32 = 7;

/// A change applied to the fixture before loading it.
type Change = Box<dyn FnOnce(&mut Value)>;

fn array(value: &mut Value) -> &mut Vec<Value> {
    value.as_array_mut().unwrap()
}

#[test]
fn the_example_fixture_satisfies_every_invariant() {
    load(&read(EXAMPLE)).unwrap();
}

#[test]
fn rule_1_collections_are_sorted_and_unique() {
    assert_rule(
        mutate(|v| array(&mut v["requirements"]).swap(0, 1)),
        Rule::SortedUnique,
    );
    assert_rule(
        mutate(|v| array(&mut v["provenance"]).swap(0, 1)),
        Rule::SortedUnique,
    );
    assert_rule(
        mutate(|v| {
            let first = v["entities"][0].clone();
            array(&mut v["entities"]).insert(0, first);
        }),
        Rule::SortedUnique,
    );
    assert_rule(
        mutate(|v| array(&mut v["plans"][0]["tasks"]).swap(0, 1)),
        Rule::SortedUnique,
    );
    // Task IDs are unique across plans.
    assert_rule(
        mutate(|v| {
            let task = v["plans"][1]["tasks"][0].clone();
            array(&mut v["plans"][2]["tasks"]).push(task);
        }),
        Rule::SortedUnique,
    );
}

#[test]
fn rule_1_domains_are_sorted_unique_and_exclude_core() {
    assert_rule(
        mutate(|v| array(&mut v["domains"]).push(json!({"id": "example", "version": 1}))),
        Rule::Domains,
    );
    assert_rule(
        mutate(|v| array(&mut v["domains"]).push(json!({"id": "core", "version": 1}))),
        Rule::Domains,
    );
}

#[test]
fn rule_2_references_resolve() {
    for change in [
        |v: &mut Value| v["links"][0]["to"] = json!(id("req", 99)),
        |v: &mut Value| v["links"][2]["from"] = json!(id("art", 99)),
        |v: &mut Value| v["plans"][0]["tasks"][0]["inputs"][0] = json!(id("ent", 99)),
        |v: &mut Value| v["plans"][0]["tasks"][0]["expected_outputs"][0] = json!(id("art", 99)),
        |v: &mut Value| v["plans"][0]["tasks"][0]["addresses"][0] = json!(id("req", 99)),
        |v: &mut Value| v["validations"][0]["subjects"][0]["artifact"] = json!(id("art", 99)),
        |v: &mut Value| v["validations"][0]["requirements"][0] = json!(id("req", 99)),
        |v: &mut Value| v["requirements"][2]["status"]["replacement"] = json!(id("req", 99)),
        |v: &mut Value| v["entities"][2]["data"]["parts"][0] = json!(id("ent", 10)),
    ] {
        assert_rule(mutate(change), Rule::ReferenceResolves);
    }
}

#[test]
fn rule_3_kinds_are_registered_for_their_use() {
    for change in [
        |v: &mut Value| v["artifacts"][0]["kind"] = json!("core.spreadsheet"),
        |v: &mut Value| v["artifacts"][0]["kind"] = json!("example.widget"),
        |v: &mut Value| v["plans"][0]["tasks"][0]["action"] = json!("example.report"),
        |v: &mut Value| v["validations"][0]["check"] = json!("example.build"),
        |v: &mut Value| v["links"][0]["kind"] = json!("core.verifies"),
        |v: &mut Value| v["links"][0]["kind"] = json!("other.satisfies"),
    ] {
        assert_rule(mutate(change), Rule::KindRegistered);
    }
}

#[test]
fn rule_4_link_endpoints_match_the_link_rule() {
    // `core.satisfies` goes from an entity or artifact to a requirement.
    assert_rule(
        mutate(|v| v["links"][0]["from"] = json!(id("req", 14))),
        Rule::LinkEndpoints,
    );
    assert_rule(
        mutate(|v| v["links"][0]["to"] = json!(id("ent", 22))),
        Rule::LinkEndpoints,
    );
    // A domain link rule: `example.verifies` goes from a validation result.
    assert_rule(
        mutate(|v| v["links"][1]["from"] = json!(id("art", 32))),
        Rule::LinkEndpoints,
    );
}

#[test]
fn rule_5_task_dependencies_stay_in_their_plan_and_are_acyclic() {
    assert_rule(
        mutate(|v| v["plans"][0]["tasks"][0]["depends_on"] = json!([id("tsk", 64)])),
        Rule::TaskDependencies,
    );
    assert_rule(
        mutate(|v| v["plans"][0]["tasks"][0]["depends_on"] = json!([id("tsk", 63)])),
        Rule::TaskDependencies,
    );
    assert_rule(
        mutate(|v| v["plans"][1]["tasks"][0]["depends_on"] = json!([id("tsk", 64)])),
        Rule::TaskDependencies,
    );
    // Shared dependencies without a cycle are allowed.
    mutate(|v| v["plans"][0]["tasks"][2]["depends_on"] = json!([id("tsk", 61), id("tsk", 62)]))
        .unwrap();
}

#[test]
fn rule_6_tasks_address_only_confirmed_requirements() {
    for requirement in [12, 13, 14] {
        assert_rule(
            mutate(|v| v["plans"][0]["tasks"][0]["addresses"] = json!([id("req", requirement)])),
            Rule::AddressesConfirmed,
        );
    }
}

#[test]
fn rule_7_decisions_are_made_by_the_user() {
    for actor in [MODEL, TOOL, SYSTEM] {
        let by = json!(id("prv", actor));
        let cases: Vec<Change> = vec![
            Box::new({
                let by = by.clone();
                move |v| v["requirements"][0]["status"]["by"] = by
            }),
            Box::new({
                let by = by.clone();
                move |v| v["requirements"][1]["status"]["by"] = by
            }),
            Box::new({
                let by = by.clone();
                move |v| v["requirements"][2]["status"]["by"] = by
            }),
            Box::new({
                let by = by.clone();
                move |v| v["entities"][0]["status"]["by"] = by
            }),
            Box::new({
                let by = by.clone();
                move |v| v["entities"][2]["status"]["by"] = by
            }),
            Box::new({
                let by = by.clone();
                move |v| v["plans"][0]["status"]["by"] = by
            }),
            Box::new({
                let by = by.clone();
                move |v| v["plans"][1]["status"]["by"] = by
            }),
            Box::new({
                let by = by.clone();
                move |v| v["plans"][0]["tasks"][2]["disposition"]["by"] = by
            }),
            Box::new(move |v| v["plans"][1]["tasks"][0]["disposition"]["by"] = by),
        ];
        for change in cases {
            assert_rule(mutate(change), Rule::UserDecision);
        }
    }
}

#[test]
fn rule_8_validation_results_come_from_a_tool_or_the_system() {
    for actor in [MODEL, USER] {
        assert_rule(
            mutate(|v| v["validations"][0]["provenance"] = json!(id("prv", actor))),
            Rule::ValidationActor,
        );
    }
}

#[test]
fn rule_9_references_to_provenance_records_resolve() {
    let missing = json!(id("prv", 99));
    let cases: Vec<Change> = vec![
        Box::new({
            let m = missing.clone();
            move |v| v["project"]["provenance"]["created"] = m
        }),
        Box::new({
            let m = missing.clone();
            move |v| v["requirements"][0]["provenance"]["modified"] = m
        }),
        Box::new({
            let m = missing.clone();
            move |v| v["requirements"][0]["status"]["by"] = m
        }),
        Box::new({
            let m = missing.clone();
            move |v| v["artifacts"][0]["content"]["provenance"] = m
        }),
        Box::new({
            let m = missing.clone();
            move |v| v["validations"][0]["provenance"] = m
        }),
        Box::new(move |v| v["plans"][0]["tasks"][2]["disposition"]["by"] = missing),
    ];
    for change in cases {
        assert_rule(mutate(change), Rule::ProvenanceResolves);
    }
}

#[test]
fn rule_9_unreferenced_provenance_records_are_kept() {
    // prv_…08 is not referenced by any record in the fixture.
    let project = load(&read(EXAMPLE)).unwrap();
    assert!(
        project
            .document()
            .provenance
            .iter()
            .any(|p| p.id.to_string() == id("prv", 8))
    );
    // Removing the last reference to a provenance record is allowed.
    mutate(|v| v["validations"][1]["provenance"] = json!(id("prv", 5))).unwrap();
}

#[test]
fn references_from_provenance_records_are_historical() {
    // The fixture already contains an input that references a removed requirement.
    mutate(|v| v["provenance"][4]["inputs"][0]["record"] = json!(id("art", 99))).unwrap();
    mutate(|v| v["provenance"][3]["activity"]["task"] = json!(id("tsk", 99))).unwrap();
    // Rule 12: task-run IDs are never resolved.
    mutate(|v| v["provenance"][3]["activity"]["run"] = json!(id("run", 99))).unwrap();
}

#[test]
fn rule_10_rev_is_at_least_1() {
    assert_rule(
        mutate(|v| v["entities"][0]["provenance"]["rev"] = json!(0)),
        Rule::Revision,
    );
    assert_rule(
        mutate(|v| v["project"]["provenance"]["rev"] = json!(0)),
        Rule::Revision,
    );
}

#[test]
fn diagnostics_per_validation_result_are_limited() {
    let diagnostic = json!({"severity": "info", "message": "Note.", "location": null});
    let at_limit = diagnostic.clone();
    mutate(move |v| v["validations"][0]["diagnostics"] = json!(vec![at_limit; MAX_DIAGNOSTICS]))
        .unwrap();
    assert_rule(
        mutate(move |v| {
            v["validations"][0]["diagnostics"] = json!(vec![diagnostic; MAX_DIAGNOSTICS + 1]);
        }),
        Rule::Limit,
    );
}
