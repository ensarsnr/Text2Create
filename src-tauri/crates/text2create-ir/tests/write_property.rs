//! Property test: sequences of random change sets either fail without changing
//! the project, or produce a project that loads again after canonical
//! serialization (both checked by `Harness::apply_set`).
//!
//! Uses a small deterministic generator instead of a property-testing
//! dependency, so failures are reproducible from the printed seed.

mod common;

use common::{Harness, digest, id, model, system, tool, user};
use serde_json::{Value, json};
use text2create_ir::change::ChangeSet;
use text2create_ir::source::Source;

/// xorshift64*: deterministic and dependency-free.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }
}

/// Identifiers of the current records, by prefix.
struct Records {
    requirements: Vec<String>,
    entities: Vec<String>,
    artifacts: Vec<String>,
    plans: Vec<String>,
    tasks: Vec<String>,
}

impl Records {
    fn of(h: &Harness) -> Self {
        let d = h.project.document();
        Self {
            requirements: d.requirements.iter().map(|r| r.id.to_string()).collect(),
            entities: d.entities.iter().map(|r| r.id.to_string()).collect(),
            artifacts: d.artifacts.iter().map(|r| r.id.to_string()).collect(),
            plans: d.plans.iter().map(|r| r.id.to_string()).collect(),
            tasks: d
                .plans
                .iter()
                .flat_map(|p| &p.tasks)
                .map(|t| t.id.to_string())
                .collect(),
        }
    }
}

/// A reference to an existing record, a missing record, or a temporary reference.
fn reference(rng: &mut Rng, existing: &[String], prefix: &str, temps: &[String]) -> Value {
    match rng.below(10) {
        0 => json!({"id": id(prefix, 99)}),
        1 | 2 if !temps.is_empty() => json!({"temp": rng.pick(temps).unwrap()}),
        _ => match rng.pick(existing) {
            Some(existing) => json!({"id": existing}),
            None => json!({"id": id(prefix, 98)}),
        },
    }
}

fn any_reference(rng: &mut Rng, records: &Records, temps: &[String]) -> Value {
    match rng.below(4) {
        0 => reference(rng, &records.requirements, "req", temps),
        1 => reference(rng, &records.entities, "ent", temps),
        2 => reference(rng, &records.artifacts, "art", temps),
        _ => reference(rng, &records.tasks, "tsk", temps),
    }
}

fn operation(rng: &mut Rng, r: &Records, temps: &mut Vec<String>, counter: &mut u32) -> Value {
    let mut declare = |rng: &mut Rng, temps: &mut Vec<String>| -> Option<String> {
        rng.chance(40).then(|| {
            *counter += 1;
            let temp = format!("t{counter}");
            temps.push(temp.clone());
            temp
        })
    };
    let decision = |rng: &mut Rng, a: &str, b: &str| if rng.chance(50) { a } else { b }.to_owned();
    let mut op = match rng.below(15) {
        0 => json!({"op": "set_project_info", "name": "Renamed", "description": null}),
        1 => {
            json!({"op": "add_requirement", "kind": "functional", "priority": "must", "title": "R",
            "statement": "S.", "origin": {"type": "inferred", "rationale": "Q."}, "acceptance": []})
        }
        2 => {
            json!({"op": "edit_requirement", "requirement": reference(rng, &r.requirements, "req", temps),
            "kind": "constraint", "priority": "could", "title": "E", "statement": "E.",
            "origin": {"type": "user", "quote": "E."}, "acceptance": ["E."]})
        }
        3 => {
            json!({"op": "decide_requirement", "requirement": reference(rng, &r.requirements, "req", temps),
            "decision": decision(rng, "confirm", "reject")})
        }
        4 => {
            json!({"op": "supersede_requirement", "requirement": reference(rng, &r.requirements, "req", temps),
            "replacement": reference(rng, &r.requirements, "req", temps)})
        }
        5 => {
            let data = if rng.chance(80) {
                json!({"label": "W", "size": rng.below(100), "tolerance": null})
            } else {
                json!({"label": "W", "size": -1, "tolerance": null})
            };
            json!({"op": "add_entity", "kind": "example.widget", "name": "W", "data": data})
        }
        6 => {
            json!({"op": "edit_entity", "entity": reference(rng, &r.entities, "ent", temps), "name": "E",
            "data": {"label": "E", "size": 5, "tolerance": "0.5"}})
        }
        7 => json!({"op": "decide_entity", "entity": reference(rng, &r.entities, "ent", temps),
            "decision": decision(rng, "accept", "reject")}),
        8 => {
            let kind = ["core.document", "example.report", "core.unknown"][rng.below(3)];
            json!({"op": "add_artifact", "kind": kind, "path": format!("f/{}.txt", rng.below(1000)), "ownership": "generated"})
        }
        9 => {
            json!({"op": "record_artifact_content", "artifact": reference(rng, &r.artifacts, "art", temps),
            "digest": digest('9'), "size_bytes": rng.below(1 << 20)})
        }
        10 => {
            let kind = [
                "core.satisfies",
                "core.derived_from",
                "core.refines",
                "core.describes",
            ][rng.below(4)];
            json!({"op": "add_link", "kind": kind, "from": any_reference(rng, r, temps), "to": any_reference(rng, r, temps)})
        }
        11 => {
            let tasks: Vec<Value> = (0..rng.below(3))
                .map(|_| {
                    let addresses = if rng.chance(50) {
                        vec![reference(rng, &r.requirements, "req", &[])]
                    } else {
                        vec![]
                    };
                    json!({"title": "T", "action": "example.build", "depends_on": [], "inputs": [],
                        "expected_outputs": [], "addresses": addresses})
                })
                .collect();
            json!({"op": "add_plan", "goal": "G", "tasks": tasks})
        }
        12 => json!({"op": "decide_plan", "plan": reference(rng, &r.plans, "pln", temps),
            "decision": decision(rng, "approve", "archive")}),
        13 => json!({"op": "set_task_disposition", "task": reference(rng, &r.tasks, "tsk", temps),
            "disposition": decision(rng, "skip", "cancel")}),
        _ => json!({"op": "record_validation", "check": "example.inspect",
            "subjects": [{"artifact": reference(rng, &r.artifacts, "art", temps), "digest": digest('8')}],
            "requirements": [], "outcome": "passed", "summary": "S.", "diagnostics": []}),
    };
    // Some creating operations declare a temporary reference.
    let creates = [
        "add_requirement",
        "add_entity",
        "add_artifact",
        "add_plan",
        "record_validation",
    ];
    if creates.contains(&op["op"].as_str().unwrap())
        && let Some(temp) = declare(rng, temps)
    {
        op["temp"] = json!(temp);
    }
    op
}

fn source(rng: &mut Rng) -> Source {
    match rng.below(4) {
        0 => user(),
        1 => model(),
        2 => tool(),
        _ => system(),
    }
}

#[test]
fn random_change_sets_either_fail_cleanly_or_produce_a_loadable_project() {
    let (mut applied, mut rejected) = (0, 0);
    for seed in 1..=150u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let mut h = Harness::example();
        let mut counter = 0;
        for _ in 0..6 {
            let records = Records::of(&h);
            let mut temps = Vec::new();
            let operations: Vec<Value> = (0..1 + rng.below(4))
                .map(|_| operation(&mut rng, &records, &mut temps, &mut counter))
                .collect();
            let set = ChangeSet::from_json(
                json!({"inputs": [], "operations": operations})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap_or_else(|e| panic!("seed {seed}: generated an unparsable change set: {e}"));
            let source = source(&mut rng);
            // `apply_set` asserts byte-identity on failure and loadability on success.
            match h.apply_set(&source, &set) {
                Ok(_) => applied += 1,
                Err(_) => rejected += 1,
            }
        }
    }
    // Both outcomes must be exercised for the test to mean anything.
    println!("applied {applied}, rejected {rejected}");
    assert!(applied > 100, "only {applied} change sets applied");
    assert!(rejected > 100, "only {rejected} change sets rejected");
}
