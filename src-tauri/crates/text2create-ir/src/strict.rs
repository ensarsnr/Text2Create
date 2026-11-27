//! Strict JSON parsing for untrusted input.
//!
//! `serde_json` silently keeps the last value when an object repeats a key, so
//! repeated keys must be rejected while parsing, before a [`Value`] exists. This
//! parser also rejects every number that is not an integer in the 64-bit range,
//! because the Project IR contains no floating-point numbers.

use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::fmt;

/// Maximum accepted document size in bytes (16 MiB).
pub const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;

/// Maximum nesting depth of arrays and objects. It matches `serde_json`'s
/// recursion limit, so typed parsing of the same bytes cannot fail on depth,
/// and it still applies if a dependency enables `serde_json/unbounded_depth`.
pub const MAX_NESTING_DEPTH: usize = 127;

/// Errors produced by [`parse`].
#[derive(Debug, thiserror::Error)]
pub enum StrictJsonError {
    /// The input exceeds [`MAX_DOCUMENT_BYTES`].
    #[error("document is {size} bytes; the limit is {limit} bytes")]
    TooLarge { size: usize, limit: usize },
    /// Malformed or disallowed JSON. The message includes the line and column.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Parses untrusted JSON bytes into a [`Value`].
///
/// Rejects:
/// - input larger than [`MAX_DOCUMENT_BYTES`];
/// - repeated object keys at any depth;
/// - non-integer numbers (fractions or exponents) and integers outside the
///   `i64`/`u64` range;
/// - arrays and objects nested deeper than [`MAX_NESTING_DEPTH`];
/// - trailing data after the value, and anything `serde_json` rejects
///   (for example a byte order mark or an unpaired surrogate escape).
pub fn parse(bytes: &[u8]) -> Result<Value, StrictJsonError> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(StrictJsonError::TooLarge {
            size: bytes.len(),
            limit: MAX_DOCUMENT_BYTES,
        });
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = Seed { depth: 0 }.deserialize(&mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

/// Deserializes one value nested inside `depth` arrays or objects.
#[derive(Clone, Copy)]
struct Seed {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for Seed {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(StrictVisitor { depth: self.depth })
    }
}

struct StrictVisitor {
    depth: usize,
}

impl StrictVisitor {
    /// The seed for the children of an array or object at this depth.
    fn child<E: de::Error>(&self) -> Result<Seed, E> {
        let depth = self.depth + 1;
        if depth > MAX_NESTING_DEPTH {
            return Err(E::custom(format_args!(
                "arrays and objects are nested deeper than {MAX_NESTING_DEPTH} levels"
            )));
        }
        Ok(Seed { depth })
    }
}

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value without repeated keys or non-integer numbers")
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    // Fractions, exponents, `-0`, and integers outside the i64/u64 range all
    // arrive here.
    fn visit_f64<E: de::Error>(self, _v: f64) -> Result<Value, E> {
        Err(E::custom(
            "non-integer or out-of-range number; the Project IR only allows 64-bit integers",
        ))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let seed = self.child()?;
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(seed)? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let seed = self.child()?;
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom(format_args!("repeated key `{key}`")));
            }
            let value = map.next_value_seed(seed)?;
            object.insert(key, value);
        }
        Ok(Value::Object(object))
    }
}
