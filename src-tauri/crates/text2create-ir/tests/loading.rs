//! Loading pipeline: fixtures, canonical serialization, versions, typed parsing.

mod common;

use common::{ESP32, EXAMPLE, error_text, example_value, id, load, mutate, read};
use serde_json::{Value, json};
use text2create_ir::LoadError;
use text2create_ir::Project;
use text2create_ir::domain::DomainRegistry;
use text2create_ir::model::ProjectDocument;

fn text(path: &str) -> String {
    String::from_utf8(read(path)).unwrap()
}

#[test]
fn esp32_fixture_is_core_only_and_loads_without_any_domain() {
    let project = Project::load(&read(ESP32), &DomainRegistry::new()).unwrap();
    let document = project.document();
    assert!(document.domains.is_empty());
    assert!(document.entities.is_empty());
    assert_eq!(document.requirements.len(), 4);
}

#[test]
fn canonical_serialization_reproduces_the_committed_fixtures_byte_for_byte() {
    for path in [ESP32, EXAMPLE] {
        let committed = text(path);
        let first = load(committed.as_bytes()).unwrap().to_canonical_json();
        assert_eq!(
            first, committed,
            "{path}: canonical output differs from the fixture"
        );
        let second = load(first.as_bytes()).unwrap().to_canonical_json();
        assert_eq!(second, first, "{path}: load and save is not stable");
    }
}

#[test]
fn any_formatting_and_field_order_load_to_the_same_canonical_bytes() {
    // Compact output, with object keys in reverse alphabetical order.
    fn reorder(value: &Value) -> String {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort_by(|a, b| b.cmp(a));
                let fields: Vec<String> = keys
                    .into_iter()
                    .map(|k| format!("{}:{}", Value::String(k.clone()), reorder(&map[k])))
                    .collect();
                format!("{{{}}}", fields.join(","))
            }
            Value::Array(items) => {
                format!(
                    "[{}]",
                    items.iter().map(reorder).collect::<Vec<_>>().join(",")
                )
            }
            other => other.to_string(),
        }
    }
    for path in [ESP32, EXAMPLE] {
        let value: Value = serde_json::from_slice(&read(path)).unwrap();
        let shuffled = reorder(&value);
        assert_ne!(shuffled, text(path));
        let project = load(shuffled.as_bytes()).unwrap();
        assert_eq!(project.to_canonical_json(), text(path), "{path}");
    }
}

#[test]
fn every_record_type_round_trips() {
    let project = load(&read(EXAMPLE)).unwrap();
    let document = project.document();
    // The fixture contains every record type.
    assert!(!document.provenance.is_empty());
    assert!(!document.requirements.is_empty());
    assert!(!document.entities.is_empty());
    assert!(!document.artifacts.is_empty());
    assert!(!document.links.is_empty());
    assert!(document.plans.iter().any(|plan| !plan.tasks.is_empty()));
    assert!(!document.validations.is_empty());
    // Typed value -> JSON -> typed value is the identity.
    let json = serde_json::to_value(document).unwrap();
    let back: ProjectDocument = serde_json::from_value(json).unwrap();
    assert_eq!(&back, document);
    assert_eq!(
        load(project.to_canonical_json().as_bytes()).unwrap(),
        project
    );
}

#[test]
fn staleness_is_computed_from_digests() {
    let project = load(&read(EXAMPLE)).unwrap();
    let validations = &project.document().validations;
    assert!(!project.is_stale(&validations[0]));
    let changed = mutate(|v| {
        v["artifacts"][1]["content"]["digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    })
    .unwrap();
    assert!(changed.is_stale(&changed.document().validations[0]));
    let unproduced = mutate(|v| v["artifacts"][1]["content"] = Value::Null).unwrap();
    assert!(unproduced.is_stale(&unproduced.document().validations[0]));
}

// ------------------------------------------------------------------ versions

#[test]
fn newer_format_versions_are_rejected_without_modifying_the_input() {
    for version in [json!(2), json!(u64::MAX)] {
        let mut value = example_value();
        value["format"]["version"] = version.clone();
        let input = serde_json::to_vec_pretty(&value).unwrap();
        let before = input.clone();
        let result = load(&input);
        assert!(
            matches!(result, Err(LoadError::NewerFormat { found, supported: 1 }) if json!(found) == version),
            "{version}"
        );
        assert_eq!(input, before);
    }
}

#[test]
fn older_or_invalid_format_versions_are_rejected() {
    assert!(matches!(
        mutate(|v| v["format"]["version"] = json!(0)),
        Err(LoadError::OlderFormat {
            found: 0,
            supported: 1
        })
    ));
    for change in [
        |v: &mut Value| v["format"]["version"] = json!(-1),
        |v: &mut Value| v["format"]["version"] = json!("1"),
        |v: &mut Value| v["format"]["schema"] = json!("other"),
        |v: &mut Value| {
            v.as_object_mut().unwrap().remove("format");
        },
    ] {
        assert!(matches!(mutate(change), Err(LoadError::NotProjectIr(_))));
    }
    assert!(matches!(load(b"[]"), Err(LoadError::NotProjectIr(_))));
}

#[test]
fn domain_versions_must_match_this_build() {
    assert!(matches!(
        mutate(|v| v["domains"][0]["version"] = json!(2)),
        Err(LoadError::NewerDomain {
            found: 2,
            supported: 1,
            ..
        })
    ));
    assert!(matches!(
        mutate(|v| v["domains"][0]["version"] = json!(0)),
        Err(LoadError::OlderDomain {
            found: 0,
            supported: 1,
            ..
        })
    ));
    assert!(matches!(
        mutate(|v| v["domains"][0]["id"] = json!("robotics")),
        Err(LoadError::UnknownDomain(id)) if id == "robotics"
    ));
    // A build without the domain rejects the project.
    assert!(matches!(
        Project::load(&read(EXAMPLE), &DomainRegistry::new()),
        Err(LoadError::UnknownDomain(id)) if id == "example"
    ));
}

// -------------------------------------------------------------- typed parsing

#[test]
fn unknown_fields_are_rejected_with_their_location() {
    let input = text(EXAMPLE).replacen(
        r#""kind": "functional","#,
        r#""kind": "functional", "tags": [],"#,
        1,
    );
    match load(input.as_bytes()) {
        Err(LoadError::Schema(error)) => {
            assert!(
                error.to_string().contains("unknown field `tags`"),
                "{error}"
            );
            assert!(error.line() > 1);
        }
        other => panic!("unexpected result: {:?}", other.map(|_| ())),
    }
    for change in [
        |v: &mut Value| v["extra"] = json!(true),
        |v: &mut Value| v["format"]["extra"] = json!(1),
        |v: &mut Value| v["project"]["provenance"]["extra"] = json!(1),
        |v: &mut Value| v["provenance"][0]["actor"]["name"] = json!("someone"),
        |v: &mut Value| v["provenance"][0]["activity"]["note"] = json!("x"),
        |v: &mut Value| v["requirements"][3]["status"]["by"] = json!(id("prv", 3)),
        |v: &mut Value| v["artifacts"][0]["content"]["mime"] = json!("text/markdown"),
        |v: &mut Value| v["plans"][0]["tasks"][0]["status"] = json!("done"),
        |v: &mut Value| v["validations"][0]["diagnostics"][0]["code"] = json!("E1"),
    ] {
        let error = error_text(mutate(change));
        assert!(error.contains("unknown field"), "{error}");
    }
}

#[test]
fn unknown_enum_values_are_rejected() {
    for change in [
        |v: &mut Value| v["requirements"][0]["kind"] = json!("legal"),
        |v: &mut Value| v["requirements"][0]["priority"] = json!("wont"),
        |v: &mut Value| v["requirements"][0]["status"]["state"] = json!("done"),
        |v: &mut Value| v["requirements"][0]["origin"]["type"] = json!("guess"),
        |v: &mut Value| v["provenance"][0]["actor"]["type"] = json!("robot"),
        |v: &mut Value| v["provenance"][0]["activity"]["type"] = json!("deploy"),
        |v: &mut Value| v["entities"][0]["status"]["state"] = json!("validated"),
        |v: &mut Value| v["artifacts"][0]["ownership"] = json!("shared"),
        |v: &mut Value| v["plans"][0]["status"]["state"] = json!("running"),
        |v: &mut Value| v["plans"][0]["tasks"][0]["disposition"]["state"] = json!("failed"),
        |v: &mut Value| v["validations"][0]["outcome"] = json!("flaky"),
        |v: &mut Value| v["validations"][0]["diagnostics"][0]["severity"] = json!("fatal"),
    ] {
        let error = error_text(mutate(change));
        assert!(error.contains("unknown variant"), "{error}");
    }
}

#[test]
fn every_field_must_be_present() {
    for change in [
        |v: &mut Value| {
            v["project"].as_object_mut().unwrap().remove("description");
        },
        |v: &mut Value| {
            v["provenance"][0].as_object_mut().unwrap().remove("model");
        },
        |v: &mut Value| {
            v["artifacts"][2].as_object_mut().unwrap().remove("content");
        },
        |v: &mut Value| {
            v.as_object_mut().unwrap().remove("validations");
        },
    ] {
        let error = error_text(mutate(change));
        assert!(error.contains("missing field"), "{error}");
    }
}

#[test]
fn invalid_identifiers_and_scalars_are_rejected() {
    for change in [
        |v: &mut Value| v["project"]["id"] = json!("prj_01A21C90-0000-7000-8000-000000000001"),
        |v: &mut Value| v["requirements"][0]["id"] = json!(id("ent", 11)),
        |v: &mut Value| v["links"][0]["from"] = json!(id("prv", 1)),
        |v: &mut Value| v["artifacts"][0]["path"] = json!("../outside.md"),
        |v: &mut Value| v["artifacts"][0]["path"] = json!("/etc/passwd"),
        |v: &mut Value| v["artifacts"][0]["content"]["digest"] = json!("sha256:abc"),
        |v: &mut Value| v["provenance"][0]["at"] = json!("2026-11-27T10:00:00Z"),
        |v: &mut Value| v["project"]["name"] = json!(" padded"),
        |v: &mut Value| v["project"]["name"] = json!("x".repeat(121)),
        |v: &mut Value| v["entities"][0]["kind"] = json!("Example.widget"),
        |v: &mut Value| v["requirements"][0]["provenance"]["rev"] = json!(-1),
        |v: &mut Value| v["requirements"][0]["provenance"]["rev"] = json!(u64::from(u32::MAX) + 1),
    ] {
        assert!(matches!(mutate(change), Err(LoadError::Schema(_))));
    }
}
