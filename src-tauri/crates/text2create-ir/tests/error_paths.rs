//! Every rejection reports where the invalid data is, as a JSON Pointer.
//!
//! The targeted cases cover each stage of the loading pipeline and each
//! invariant. The exhaustive tests mutate every location of both fixtures and
//! check that the document is rejected with a path on the mutated branch.

mod common;

use common::{ESP32, EXAMPLE, id, load, read};
use serde_json::{Value, json};
use text2create_ir::{FieldPath, LoadError, Rule};

fn path_of(result: Result<text2create_ir::Project, LoadError>) -> (String, LoadError) {
    let error = result.expect_err("expected the load to fail");
    (error.path().as_str().to_owned(), error)
}

fn mutated(change: impl FnOnce(&mut Value)) -> (String, LoadError) {
    path_of(common::mutate(change))
}

/// Asserts that `error` reports `path` and that its message contains it.
#[track_caller]
fn assert_located(error: &LoadError, path: &str) {
    assert_eq!(error.path().as_str(), path, "{error}");
    let expected = if path.is_empty() {
        "at the document root".to_owned()
    } else {
        format!("at `{path}`")
    };
    assert!(
        error.to_string().contains(&expected),
        "missing `{expected}` in: {error}"
    );
}

#[track_caller]
fn assert_invariant(change: impl FnOnce(&mut Value), rule: Rule, path: &str) {
    let (found, error) = mutated(change);
    match &error {
        LoadError::Invariant(e) => assert_eq!(e.rule, rule, "{error}"),
        other => panic!("expected {rule:?}, got: {other}"),
    }
    assert_eq!(found, path, "{error}");
    assert!(
        error.to_string().contains(path),
        "the message names the path: {error}"
    );
}

// ---------------------------------------------------------------- stages

#[test]
fn strict_parsing_errors_have_paths() {
    let text = String::from_utf8(read(EXAMPLE)).unwrap();
    let repeated = text.replacen(r#""size": 12,"#, r#""size": 12, "size": 13,"#, 1);
    let (path, error) = path_of(load(repeated.as_bytes()));
    assert!(matches!(error, LoadError::Json(_)));
    assert_eq!(path, "/entities/0/data");
    let fraction = text.replacen(r#""size": 12,"#, r#""size": 12.5,"#, 1);
    assert_eq!(
        path_of(load(fraction.as_bytes())).0,
        "/entities/0/data/size"
    );
    assert_eq!(path_of(load(b"{} []")).0, "");
}

#[test]
fn version_errors_have_paths() {
    assert_eq!(
        mutated(|v| v["format"]["schema"] = json!("x")).0,
        "/format/schema"
    );
    assert_eq!(
        mutated(|v| v["format"]["version"] = json!(9)).0,
        "/format/version"
    );
    assert_eq!(
        mutated(|v| v["format"]["version"] = json!(0)).0,
        "/format/version"
    );
    assert_eq!(
        mutated(|v| v["domains"][0]["version"] = json!(9)).0,
        "/domains/0/version"
    );
    assert_eq!(
        mutated(|v| v["domains"][0]["id"] = json!("robotics")).0,
        "/domains/0/id"
    );
}

#[test]
fn schema_errors_point_at_the_field() {
    for (change, path) in [
        (
            Box::new(|v: &mut Value| v["requirements"][1]["tags"] = json!([]))
                as Box<dyn FnOnce(&mut Value)>,
            "/requirements/1/tags",
        ),
        (
            Box::new(|v| v["project"]["name"] = json!(" padded")),
            "/project/name",
        ),
        (
            Box::new(|v| v["requirements"][2]["priority"] = json!("urgent")),
            "/requirements/2/priority",
        ),
        (
            Box::new(|v| v["artifacts"][1]["path"] = json!("../x")),
            "/artifacts/1/path",
        ),
        (
            Box::new(|v| v["plans"][0]["tasks"][1]["depends_on"][0] = json!(7)),
            "/plans/0/tasks/1/depends_on/0",
        ),
        (
            Box::new(|v| {
                v["requirements"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("statement");
            }),
            "/requirements/0",
        ),
        // Inside a tagged value, the location is the tagged value.
        (
            Box::new(|v| v["provenance"][0]["actor"]["extra"] = json!(1)),
            "/provenance/0/actor",
        ),
    ] {
        let (found, error) = mutated(change);
        assert!(matches!(error, LoadError::Schema { .. }), "{error}");
        assert_eq!(found, path, "{error}");
    }
}

#[test]
fn payload_errors_point_into_the_payload() {
    for (change, path) in [
        (
            Box::new(|v: &mut Value| v["entities"][0]["data"]["color"] = json!("red"))
                as Box<dyn FnOnce(&mut Value)>,
            "/entities/0/data/color",
        ),
        (
            Box::new(|v| v["entities"][1]["data"]["size"] = json!("12")),
            "/entities/1/data/size",
        ),
        (
            Box::new(|v| v["entities"][2]["data"]["parts"] = json!([])),
            "/entities/2/data/parts",
        ),
        (
            Box::new(|v| v["entities"][0]["kind"] = json!("example.gadget")),
            "/entities/0/kind",
        ),
    ] {
        let (found, error) = mutated(change);
        assert_eq!(found, path, "{error}");
        assert!(
            error.to_string().contains(path),
            "the message names the path: {error}"
        );
    }
}

// ------------------------------------------------------------ invariants

#[test]
fn each_invariant_reports_its_location() {
    assert_invariant(
        |v| v["requirements"].as_array_mut().unwrap().swap(0, 1),
        Rule::SortedUnique,
        "/requirements/1",
    );
    assert_invariant(
        |v| v["plans"][0]["tasks"].as_array_mut().unwrap().swap(1, 2),
        Rule::SortedUnique,
        "/plans/0/tasks/2",
    );
    assert_invariant(
        |v| {
            v["domains"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id": "example", "version": 1}))
        },
        Rule::Domains,
        "/domains/1",
    );
    assert_invariant(
        |v| v["plans"][0]["tasks"][0]["inputs"][0] = json!(id("ent", 99)),
        Rule::ReferenceResolves,
        "/plans/0/tasks/0/inputs/0",
    );
    assert_invariant(
        |v| v["requirements"][2]["status"]["replacement"] = json!(id("req", 99)),
        Rule::ReferenceResolves,
        "/requirements/2/status/replacement",
    );
    assert_invariant(
        |v| v["entities"][2]["data"]["parts"][0] = json!(id("ent", 10)),
        Rule::ReferenceResolves,
        "/entities/2/data",
    );
    assert_invariant(
        |v| v["validations"][0]["check"] = json!("example.build"),
        Rule::KindRegistered,
        "/validations/0/check",
    );
    assert_invariant(
        |v| v["links"][0]["to"] = json!(id("ent", 22)),
        Rule::LinkEndpoints,
        "/links/0/to",
    );
    assert_invariant(
        |v| v["plans"][0]["tasks"][0]["depends_on"] = json!([id("tsk", 64)]),
        Rule::TaskDependencies,
        "/plans/0/tasks/0/depends_on/0",
    );
    assert_invariant(
        |v| v["plans"][0]["tasks"][0]["addresses"] = json!([id("req", 13)]),
        Rule::AddressesConfirmed,
        "/plans/0/tasks/0/addresses/0",
    );
    assert_invariant(
        |v| v["entities"][0]["status"]["by"] = json!(id("prv", 2)),
        Rule::UserDecision,
        "/entities/0/status/by",
    );
    assert_invariant(
        |v| v["validations"][0]["provenance"] = json!(id("prv", 3)),
        Rule::ValidationActor,
        "/validations/0/provenance",
    );
    assert_invariant(
        |v| v["artifacts"][0]["content"]["provenance"] = json!(id("prv", 99)),
        Rule::ProvenanceResolves,
        "/artifacts/0/content/provenance",
    );
    assert_invariant(
        |v| v["links"][1]["provenance"]["rev"] = json!(0),
        Rule::Revision,
        "/links/1/provenance/rev",
    );
    // A missing optional field decodes as `None` but re-encodes as `null`.
    assert_invariant(
        |v| {
            v["entities"][1]["data"]
                .as_object_mut()
                .unwrap()
                .remove("tolerance");
        },
        Rule::CanonicalPayload,
        "/entities/1/data",
    );
}

#[test]
fn a_dependency_cycle_points_at_a_task_in_the_cycle() {
    let (path, error) =
        mutated(|v| v["plans"][0]["tasks"][0]["depends_on"] = json!([id("tsk", 63)]));
    assert!(
        matches!(&error, LoadError::Invariant(e) if e.rule == Rule::TaskDependencies),
        "{error}"
    );
    assert!(
        path.starts_with("/plans/0/tasks/") && path.ends_with("/depends_on"),
        "{path}"
    );
}

// ------------------------------------------------------------ exhaustive

/// The path of every value in `value`, with the value.
fn locations(value: &Value, path: FieldPath, out: &mut Vec<(FieldPath, Value)>) {
    out.push((path.clone(), value.clone()));
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                locations(child, path.key(key), out);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                locations(child, path.index(index), out);
            }
        }
        _ => {}
    }
}

/// The value at `path`, as a mutable reference.
fn at<'v>(value: &'v mut Value, path: &FieldPath) -> &'v mut Value {
    value
        .pointer_mut(path.as_str())
        .expect("path from locations()")
}

/// A value of a different JSON type that no field of the original type
/// accepts.
fn type_breaking(value: &Value) -> Value {
    match value {
        Value::Object(_) => json!("not an object"),
        Value::Array(_) => json!({}),
        Value::String(_) => json!([]),
        Value::Number(_) => json!("1"),
        Value::Bool(_) => json!("true"),
        Value::Null => json!({"unexpected": 1}),
    }
}

/// Whether one path is the other or contains it.
fn same_branch(a: &FieldPath, b: &FieldPath) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

#[test]
fn every_type_change_and_unknown_field_is_rejected_on_its_branch() {
    let mut checked = 0;
    for fixture in [ESP32, EXAMPLE] {
        let original: Value = serde_json::from_slice(&read(fixture)).unwrap();
        let mut all = Vec::new();
        locations(&original, FieldPath::root(), &mut all);
        for (path, value) in all {
            let mut mutations = Vec::new();
            if !path.as_str().is_empty() {
                let mut changed = original.clone();
                *at(&mut changed, &path) = type_breaking(&value);
                mutations.push((changed, path.clone()));
            }
            if value.is_object() {
                let mut changed = original.clone();
                at(&mut changed, &path)
                    .as_object_mut()
                    .unwrap()
                    .insert("zz_unknown".to_owned(), json!(1));
                mutations.push((changed, path.key("zz_unknown")));
            }
            for (changed, mutation) in mutations {
                let bytes = serde_json::to_vec_pretty(&changed).unwrap();
                let error = match load(&bytes) {
                    Ok(_) => panic!("{fixture}: mutation at `{mutation}` was accepted"),
                    Err(error) => error,
                };
                let found = error.path();
                assert!(
                    same_branch(&found, &mutation),
                    "{fixture}: mutation at `{mutation}` reported at `{found}`: {error}"
                );
                assert_located(&error, found.as_str());
                checked += 1;
            }
        }
    }
    assert!(checked > 500, "only {checked} mutations");
}

#[test]
fn every_repeated_key_is_rejected_at_its_object() {
    // Repeat the first member of every object in the text of the fixture.
    let text = String::from_utf8(read(EXAMPLE)).unwrap();
    let original: Value = serde_json::from_str(&text).unwrap();
    let mut all = Vec::new();
    locations(&original, FieldPath::root(), &mut all);
    let objects: Vec<_> = all
        .into_iter()
        .filter(|(_, v)| v.as_object().is_some_and(|m| !m.is_empty()))
        .collect();
    for (path, value) in &objects {
        // Serialize with a marker member, then duplicate it in the text.
        let mut changed = original.clone();
        at(&mut changed, path)
            .as_object_mut()
            .unwrap()
            .insert("zz_marker".to_owned(), json!(0));
        let json = serde_json::to_string(&changed).unwrap();
        assert_eq!(json.matches("\"zz_marker\":0").count(), 1);
        let repeated = json.replace("\"zz_marker\":0", "\"zz_marker\":0,\"zz_marker\":0");
        let error = load(repeated.as_bytes()).expect_err("repeated key accepted");
        assert!(matches!(error, LoadError::Json(_)), "{error}");
        assert_eq!(&error.path(), path, "{error}");
        assert_located(&error, path.as_str());
        let _ = value;
    }
    assert!(objects.len() > 50);
}

#[test]
fn every_load_error_names_its_location_in_its_message() {
    use text2create_ir::domain::DomainRegistry;
    use text2create_ir::strict::{MAX_DOCUMENT_BYTES, StrictJsonError};
    let too_large = vec![b' '; MAX_DOCUMENT_BYTES + 1];
    let cases: Vec<(LoadError, &str)> = vec![
        (load(&too_large).unwrap_err(), ""),
        (load(b"{\"a\": 1, \"a\": 2}").unwrap_err(), ""),
        (
            mutated(|v| v["entities"][0]["data"]["size"] = json!(1.5)).1,
            "/entities/0/data/size",
        ),
        (load(b"[]").unwrap_err(), "/format"),
        (
            mutated(|v| v["format"]["schema"] = json!("other")).1,
            "/format/schema",
        ),
        (
            mutated(|v| v["format"]["version"] = json!(2)).1,
            "/format/version",
        ),
        (
            mutated(|v| v["format"]["version"] = json!(0)).1,
            "/format/version",
        ),
        (
            mutated(|v| v["domains"][0]["id"] = json!("robotics")).1,
            "/domains/0/id",
        ),
        (
            mutated(|v| v["domains"][0]["version"] = json!(2)).1,
            "/domains/0/version",
        ),
        (
            mutated(|v| v["domains"][0]["version"] = json!(0)).1,
            "/domains/0/version",
        ),
        (
            mutated(|v| v["project"]["name"] = json!(" padded")).1,
            "/project/name",
        ),
        (
            mutated(|v| v["entities"][1]["data"]["size"] = json!("x")).1,
            "/entities/1/data/size",
        ),
        (
            mutated(|v| v["links"][1]["provenance"]["rev"] = json!(0)).1,
            "/links/1/provenance/rev",
        ),
        (
            text2create_ir::Project::load(&read(EXAMPLE), &DomainRegistry::new()).unwrap_err(),
            "/domains/0/id",
        ),
    ];
    let mut kinds = std::collections::BTreeSet::new();
    for (error, path) in &cases {
        assert_located(error, path);
        kinds.insert(match error {
            LoadError::Json(StrictJsonError::TooLarge { .. }) => "too large",
            LoadError::Json(StrictJsonError::Json { .. }) => "json",
            LoadError::NotProjectIr { .. } => "not ir",
            LoadError::NewerFormat { .. } => "newer format",
            LoadError::OlderFormat { .. } => "older format",
            LoadError::UnknownDomain { .. } => "unknown domain",
            LoadError::NewerDomain { .. } => "newer domain",
            LoadError::OlderDomain { .. } => "older domain",
            LoadError::Schema { .. } => "schema",
            LoadError::Payload { .. } => "payload",
            LoadError::Invariant(_) => "invariant",
        });
    }
    assert_eq!(kinds.len(), 11, "every variant is covered: {kinds:?}");
}
