//! Measures the cost of copy-on-apply. Ignored by default; run with
//! `cargo test --release -p text2create-ir --test write_cost -- --ignored --nocapture`.

mod common;

use common::{Harness, user};
use serde_json::{Value, json};
use std::time::Instant;

#[test]
#[ignore = "measurement, not a correctness test"]
fn copy_on_apply_cost() {
    for records in [100, 1_000, 5_000] {
        let mut h = Harness::example();
        let add = |n: usize| -> Vec<Value> {
            (0..n)
                .map(|i| {
                    json!({"op": "add_requirement", "kind": "functional", "priority": "must",
                        "title": format!("Requirement {i}"), "statement": "A requirement statement of moderate length.",
                        "origin": {"type": "inferred", "rationale": "Measurement."}, "acceptance": ["It holds."]})
                })
                .collect()
        };
        h.ok(&user(), json!(add(records)));
        let size = h.canonical().len();
        let runs = 20;
        let start = Instant::now();
        for _ in 0..runs {
            h.ok(&user(), json!(add(1)));
        }
        // `Harness::ok` also reloads the project to check it; measure apply alone too.
        let with_reload = start.elapsed() / runs;
        let start = Instant::now();
        for _ in 0..runs {
            let mut env = text2create_ir::WriteEnv {
                registry: &h.registry,
                clock: &mut h.clock,
                ids: &mut h.ids,
            };
            h.project
                .apply(&user(), &common::changes(json!(add(1))), &mut env)
                .unwrap();
        }
        let apply_only = start.elapsed() / runs;
        println!(
            "{records:>6} requirements, {:>6} KiB canonical JSON: apply {apply_only:?}, apply + reload check {with_reload:?}",
            size / 1024
        );
    }
}
