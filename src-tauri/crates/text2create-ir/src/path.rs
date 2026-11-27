//! Machine-readable locations of invalid data.

use std::fmt;

/// The location of a value in a JSON document, as a JSON Pointer (RFC 6901),
/// for example `/requirements/0/status/by`. The document root is the empty
/// pointer.
///
/// Errors report the most precise location known. Inside values whose type
/// is selected by a tag field (such as `actor` or `status`), the location is
/// the tagged value itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldPath(String);

impl FieldPath {
    /// The document root.
    pub fn root() -> Self {
        Self(String::new())
    }

    /// The pointer as a string; empty for the root.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The member `key` of the object at this path.
    pub fn key(&self, key: &str) -> Self {
        let mut path = self.0.clone();
        path.push('/');
        for c in key.chars() {
            match c {
                '~' => path.push_str("~0"),
                '/' => path.push_str("~1"),
                c => path.push(c),
            }
        }
        Self(path)
    }

    /// The element `index` of the array at this path.
    pub fn index(&self, index: usize) -> Self {
        Self(format!("{}/{index}", self.0))
    }

    /// `relative`, interpreted relative to this path.
    pub fn join(&self, relative: &FieldPath) -> Self {
        Self(format!("{}{}", self.0, relative.0))
    }

    /// Whether this path is `other` or a location inside it.
    pub fn starts_with(&self, other: &FieldPath) -> bool {
        self.0 == other.0
            || (self.0.starts_with(&other.0) && self.0.as_bytes().get(other.0.len()) == Some(&b'/'))
    }

    /// Converts a path recorded while deserializing. Segments that cannot be
    /// represented end the path at the last known location.
    pub(crate) fn from_serde(path: &serde_path_to_error::Path) -> Self {
        use serde_path_to_error::Segment;
        let mut result = Self::root();
        for segment in path.iter() {
            result = match segment {
                Segment::Seq { index } => result.index(*index),
                Segment::Map { key } => result.key(key),
                Segment::Enum { variant } => result.key(variant),
                Segment::Unknown => break,
            };
        }
        result
    }
}

impl fmt::Display for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str("(document root)")
        } else {
            f.write_str(&self.0)
        }
    }
}

/// A location built on the stack while checking a document. It allocates
/// nothing and is converted into a [`FieldPath`] only when a check fails.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Loc<'a> {
    Root,
    Key(&'a Loc<'a>, &'a str),
    Index(&'a Loc<'a>, usize),
}

/// The document root.
pub(crate) static ROOT: Loc<'static> = Loc::Root;

impl<'a> Loc<'a> {
    pub(crate) fn key(&'a self, key: &'a str) -> Loc<'a> {
        Loc::Key(self, key)
    }

    pub(crate) fn index(&'a self, index: usize) -> Loc<'a> {
        Loc::Index(self, index)
    }

    pub(crate) fn field_path(&self) -> FieldPath {
        match self {
            Loc::Root => FieldPath::root(),
            Loc::Key(parent, key) => parent.field_path().key(key),
            Loc::Index(parent, index) => parent.field_path().index(*index),
        }
    }
}

// Successful parsing runs without path tracking. Only when it fails is the
// input parsed again with tracking, which fails at the same place and
// reports the location. Parsing is deterministic, so both runs agree.

/// Deserializes `T` from JSON bytes and reports the location of an error.
pub(crate) fn from_slice<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, (FieldPath, serde_json::Error)> {
    serde_json::from_slice(bytes).map_err(|error| {
        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
        match serde_path_to_error::deserialize::<_, T>(&mut deserializer) {
            Err(tracked) => (FieldPath::from_serde(tracked.path()), tracked.into_inner()),
            Ok(_) => (FieldPath::root(), error),
        }
    })
}

/// Deserializes `T` from a JSON value and reports the location of an error.
pub(crate) fn from_value<'de, T: serde::Deserialize<'de>>(
    value: &'de serde_json::Value,
) -> Result<T, (FieldPath, serde_json::Error)> {
    T::deserialize(value).map_err(
        |error| match serde_path_to_error::deserialize::<_, T>(value) {
            Err(tracked) => (FieldPath::from_serde(tracked.path()), tracked.into_inner()),
            Ok(_) => (FieldPath::root(), error),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_escaped_and_paths_compose() {
        let path = FieldPath::root().key("a/b").key("c~d").index(3);
        assert_eq!(path.as_str(), "/a~1b/c~0d/3");
        assert_eq!(
            FieldPath::root().key("x").join(&path).as_str(),
            "/x/a~1b/c~0d/3"
        );
        assert_eq!(FieldPath::root().to_string(), "(document root)");
        let entities = FieldPath::root().key("entities");
        assert!(entities.index(1).starts_with(&entities));
        assert!(entities.starts_with(&FieldPath::root()));
        assert!(!FieldPath::root().key("entitiesX").starts_with(&entities));
    }
}
