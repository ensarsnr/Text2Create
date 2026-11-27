//! Domain codec behavior during loading.

mod common;

use common::{assert_rule, error_text, id, mutate};
use serde_json::{Value, json};
use text2create_ir::domain::DomainError;
use text2create_ir::{FieldPath, LoadError, Rule};

fn payload_error(result: Result<text2create_ir::Project, LoadError>) -> DomainError {
    match result {
        Err(LoadError::Payload { error, .. }) => error,
        other => panic!("expected a payload error, got {:?}", other.map(|_| ())),
    }
}

#[test]
fn payloads_are_decoded_into_their_typed_structure() {
    let error = payload_error(mutate(|v| v["entities"][0]["data"]["color"] = json!("red")));
    assert!(
        error.to_string().contains("unknown field `color`"),
        "{error}"
    );
    let error = payload_error(mutate(|v| v["entities"][0]["data"]["size"] = json!("12")));
    assert!(error.to_string().contains("invalid type"), "{error}");
    let error = payload_error(mutate(|v| {
        v["entities"][0]["data"]["tolerance"] = json!("0.050")
    }));
    assert!(error.to_string().contains("decimal"), "{error}");
    let error = payload_error(mutate(|v| v["entities"][0]["data"]["size"] = json!(-1)));
    assert!(error.to_string().contains("invalid value"), "{error}");
}

#[test]
fn payload_validation_runs_after_decoding() {
    let error = payload_error(mutate(|v| v["entities"][2]["data"]["parts"] = json!([])));
    assert_eq!(
        error,
        DomainError::invalid_at(FieldPath::root().key("parts"), "an assembly needs parts")
    );
    let error = payload_error(mutate(|v| {
        v["entities"][2]["data"]["parts"] = json!([id("ent", 22), id("ent", 21)]);
    }));
    assert_eq!(
        error,
        DomainError::invalid_at(
            FieldPath::root().key("parts"),
            "parts must be sorted and unique"
        )
    );
}

#[test]
fn payloads_must_be_in_canonical_form() {
    // A missing optional field decodes as `None` but re-encodes as `null`.
    assert_rule(
        mutate(|v| {
            v["entities"][1]["data"]
                .as_object_mut()
                .unwrap()
                .remove("tolerance");
        }),
        Rule::CanonicalPayload,
    );
}

#[test]
fn references_inside_payloads_must_resolve() {
    assert_rule(
        mutate(|v| v["entities"][2]["data"]["parts"][1] = json!(id("ent", 29))),
        Rule::ReferenceResolves,
    );
}

#[test]
fn entity_kinds_must_be_registered_by_a_listed_domain() {
    // `inspect` is an action kind of the domain, not an entity kind.
    assert_rule(
        mutate(|v| v["entities"][0]["kind"] = json!("example.inspect")),
        Rule::KindRegistered,
    );
    assert_rule(
        mutate(|v| v["entities"][0]["kind"] = json!("example.gadget")),
        Rule::KindRegistered,
    );
    // The core domain defines no entity kinds in v1.
    assert_rule(
        mutate(|v| v["entities"][0]["kind"] = json!("core.document")),
        Rule::KindRegistered,
    );
    // A domain that is available in this build but not listed in `domains`.
    let error = error_text(mutate(|v| v["domains"] = json!([])));
    assert!(error.contains("not listed in `domains`"), "{error}");
}

#[test]
fn payloads_are_opaque_to_the_core_until_the_codec_accepts_them() {
    // Any JSON shape reaches the codec; the codec decides.
    for data in [json!(null), json!([]), json!("text"), json!(1)] {
        let data: Value = data;
        let result = mutate(move |v| v["entities"][0]["data"] = data);
        assert!(matches!(result, Err(LoadError::Payload { .. })));
    }
}
