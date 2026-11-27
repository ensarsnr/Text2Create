//! Strict JSON parsing: repeated keys, numbers, limits, and error locations.

mod common;

use common::{EXAMPLE, load, read};
use serde_json::{Value, json};
use text2create_ir::LoadError;
use text2create_ir::strict::{self, MAX_DOCUMENT_BYTES, MAX_NESTING_DEPTH, StrictJsonError};

fn rejected(input: &str) -> String {
    strict::parse(input.as_bytes())
        .expect_err(&format!("accepted {input:?}"))
        .to_string()
}

#[test]
fn plain_serde_json_silently_keeps_the_last_repeated_key() {
    // The reason for the strict parser.
    let value: Value = serde_json::from_str(r#"{"a":1,"a":2}"#).unwrap();
    assert_eq!(value, json!({"a": 2}));
}

#[test]
fn repeated_keys_are_rejected_at_any_depth() {
    for input in [
        r#"{"a":1,"a":2}"#,
        r#"{"a":{"b":{"c":1,"c":1}}}"#,
        r#"[{"ok":1},{"b":1,"b":2}]"#,
        r#"{"x":[[{"y":null,"y":null}]]}"#,
    ] {
        let error = rejected(input);
        assert!(error.contains("repeated key"), "{input}: {error}");
    }
}

#[test]
fn repeated_key_inside_a_document_and_payload_is_rejected_with_its_location() {
    let text = String::from_utf8(read(EXAMPLE)).unwrap();
    let field = r#""label": "Left bracket","#;
    assert_eq!(text.matches(field).count(), 1);
    let line = text[..text.find(field).unwrap()].lines().count();
    let input = text.replacen(field, &format!("{field} \"label\": \"Other\","), 1);
    match load(input.as_bytes()) {
        Err(LoadError::Json(StrictJsonError::Json(error))) => {
            assert!(
                error.to_string().contains("repeated key `label`"),
                "{error}"
            );
            assert_eq!(error.line(), line);
        }
        other => panic!("unexpected result: {:?}", other.map(|_| ())),
    }
}

#[test]
fn only_64_bit_integers_are_accepted() {
    for input in [
        "0",
        "-1",
        "18446744073709551615",
        "-9223372036854775808",
        "[1,2]",
    ] {
        assert!(strict::parse(input.as_bytes()).is_ok(), "{input}");
    }
    for input in [
        "1.5",
        "1.0",
        "0.0",
        "1e3",
        "1E3",
        "-2.5e-3",
        "-0",
        "18446744073709551616",
        "-9223372036854775809",
        "123456789012345678901234567890",
        r#"{"a":[{"b":0.5}]}"#,
    ] {
        let error = rejected(input);
        assert!(error.contains("64-bit integers"), "{input}: {error}");
    }
}

#[test]
fn non_integer_number_in_a_document_is_rejected() {
    let input =
        String::from_utf8(read(EXAMPLE))
            .unwrap()
            .replacen(r#""size": 12,"#, r#""size": 12.0,"#, 1);
    assert!(common::error_text(load(input.as_bytes())).contains("64-bit integers"));
}

#[test]
fn nesting_is_limited() {
    let nested = |depth: usize| "[".repeat(depth) + &"]".repeat(depth);
    assert!(strict::parse(nested(MAX_NESTING_DEPTH).as_bytes()).is_ok());
    assert!(strict::parse(nested(MAX_NESTING_DEPTH + 1).as_bytes()).is_err());
    let objects = |depth: usize| r#"{"a":"#.repeat(depth) + "1" + &"}".repeat(depth);
    assert!(strict::parse(objects(MAX_NESTING_DEPTH).as_bytes()).is_ok());
    assert!(strict::parse(objects(MAX_NESTING_DEPTH + 1).as_bytes()).is_err());
    assert!(strict::parse(nested(10_000).as_bytes()).is_err());
}

#[test]
fn document_size_is_limited() {
    // A JSON string of exactly the maximum size is accepted.
    let at_limit = format!("\"{}\"", "a".repeat(MAX_DOCUMENT_BYTES - 2));
    assert!(strict::parse(at_limit.as_bytes()).is_ok());
    let over = format!("\"{}\"", "a".repeat(MAX_DOCUMENT_BYTES - 1));
    match strict::parse(over.as_bytes()) {
        Err(StrictJsonError::TooLarge { size, limit }) => {
            assert_eq!((size, limit), (MAX_DOCUMENT_BYTES + 1, MAX_DOCUMENT_BYTES));
        }
        other => panic!("unexpected result: {other:?}"),
    }
    assert!(matches!(
        load(over.as_bytes()),
        Err(LoadError::Json(StrictJsonError::TooLarge { .. }))
    ));
}

#[test]
fn malformed_input_is_rejected() {
    for input in [
        &b"{\"a\":1} {\"b\":2}"[..], // trailing data
        b"{\"a\":1},",               // trailing comma
        b"\xEF\xBB\xBF{}",           // byte order mark
        b"\"\\ud800\"",              // unpaired surrogate escape
        b"\"\xFF\"",                 // invalid UTF-8
        b"{'a':1}",                  // single quotes
        b"{\"a\":1,}",               // trailing comma in object
        b"",                         // empty input
        b"NaN",                      // not JSON
    ] {
        assert!(
            strict::parse(input).is_err(),
            "{:?}",
            String::from_utf8_lossy(input)
        );
    }
}

#[test]
fn errors_report_line_and_column() {
    let error = match strict::parse(b"{\n  \"a\": 1,\n  \"a\": 2\n}") {
        Err(StrictJsonError::Json(error)) => error,
        other => panic!("unexpected result: {other:?}"),
    };
    assert_eq!(error.line(), 3);
    assert!(error.column() > 0);
    assert!(error.to_string().contains("line 3"), "{error}");
}
