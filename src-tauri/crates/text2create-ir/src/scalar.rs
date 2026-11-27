//! Validated scalar types.
//!
//! Every type validates by rejecting invalid input and never normalizes it, so
//! loading and saving are exact. Text is not Unicode-normalized.

use serde::{Deserialize, Serialize};

/// A scalar value failed validation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {what}: {reason}")]
pub struct ScalarError {
    pub what: &'static str,
    pub reason: &'static str,
}

fn error(what: &'static str, reason: &'static str) -> ScalarError {
    ScalarError { what, reason }
}

/// Implements `TryFrom<String>`, `From<Self> for String`, and `as_str` for a
/// string newtype whose validation is `$check`.
macro_rules! string_scalar {
    ($name:ident, $check:expr) => {
        impl TryFrom<String> for $name {
            type Error = ScalarError;
            fn try_from(value: String) -> Result<Self, ScalarError> {
                let check: fn(&str) -> Result<(), ScalarError> = $check;
                check(&value)?;
                Ok(Self(value))
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl $name {
            /// The validated string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

/// Non-empty text of at most `MAX` characters, without leading or trailing
/// whitespace and without control characters other than line feed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Text<const MAX: usize>(String);

impl<const MAX: usize> TryFrom<String> for Text<MAX> {
    type Error = ScalarError;
    fn try_from(value: String) -> Result<Self, ScalarError> {
        if value.is_empty() {
            return Err(error("text", "empty"));
        }
        if value.trim() != value {
            return Err(error("text", "leading or trailing whitespace"));
        }
        if value.chars().count() > MAX {
            return Err(error("text", "too long"));
        }
        if value.chars().any(|c| c.is_control() && c != '\n') {
            return Err(error("text", "control character"));
        }
        Ok(Self(value))
    }
}

impl<const MAX: usize> From<Text<MAX>> for String {
    fn from(value: Text<MAX>) -> Self {
        value.0
    }
}

impl<const MAX: usize> Text<MAX> {
    /// The validated text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// `[a-z][a-z0-9_]*`, the building block of domain identifiers and kinds.
fn is_segment(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some('a'..='z'))
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
}

/// Maximum length of a [`DomainId`].
pub const MAX_DOMAIN_ID_LEN: usize = 32;
/// Maximum length of a [`Kind`].
pub const MAX_KIND_LEN: usize = 128;

pub(crate) fn check_domain_id(s: &str) -> Result<(), ScalarError> {
    if is_segment(s) && s.len() <= MAX_DOMAIN_ID_LEN {
        Ok(())
    } else {
        Err(error("domain id", "expected [a-z][a-z0-9_]{0,31}"))
    }
}

pub(crate) fn check_kind_name(s: &str) -> Result<(), ScalarError> {
    if !s.is_empty() && s.split('.').all(is_segment) {
        Ok(())
    } else {
        Err(error(
            "kind name",
            "expected dot-separated [a-z][a-z0-9_]* segments",
        ))
    }
}

/// Domain identifier, for example `electronics`. `core` is the built-in domain
/// and cannot be declared by a project or registered by a codec.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DomainId(String);
string_scalar!(DomainId, check_domain_id);

/// Namespaced kind: `<domain>.<name>`, where the name has one or more segments,
/// for example `embedded.arduino_cli.compile`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Kind(String);
string_scalar!(Kind, |s| {
    let (domain, name) = s
        .split_once('.')
        .ok_or(error("kind", "expected <domain>.<name>"))?;
    if s.len() > MAX_KIND_LEN {
        return Err(error("kind", "too long"));
    }
    check_domain_id(domain).map_err(|_| error("kind", "invalid domain part"))?;
    check_kind_name(name).map_err(|_| error("kind", "invalid name part"))
});

impl Kind {
    /// The domain part, for example `embedded`.
    pub fn domain(&self) -> &str {
        self.0.split_once('.').map_or("", |(domain, _)| domain)
    }

    /// The name part without the domain, for example `arduino_cli.compile`.
    pub fn name(&self) -> &str {
        self.0.split_once('.').map_or("", |(_, name)| name)
    }
}

/// Maximum length of a [`ProjectPath`] in bytes.
pub const MAX_PATH_LEN: usize = 1024;
/// Maximum length of a single [`ProjectPath`] component in bytes.
pub const MAX_PATH_COMPONENT_LEN: usize = 255;

/// Project-relative path with `/` separators. Checks are lexical only;
/// filesystem checks (symbolic links, canonicalization) belong to #13.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectPath(String);
string_scalar!(ProjectPath, |s| {
    if s.is_empty() || s.len() > MAX_PATH_LEN {
        return Err(error("path", "empty or too long"));
    }
    if s.contains('\\') {
        return Err(error("path", "backslash"));
    }
    if s.starts_with('/') {
        return Err(error("path", "absolute path"));
    }
    for component in s.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(error("path", "empty, '.', or '..' component"));
        }
        if component.len() > MAX_PATH_COMPONENT_LEN {
            return Err(error("path", "component too long"));
        }
        // Covers NUL and every other control character.
        if component.chars().any(char::is_control) {
            return Err(error("path", "control character"));
        }
    }
    Ok(())
});

/// Content digest: `sha256:` followed by 64 lowercase hexadecimal digits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Digest(String);
string_scalar!(Digest, |s| match s.strip_prefix("sha256:") {
    Some(hex) if hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) => {
        Ok(())
    }
    _ => Err(error("digest", "expected sha256:<64 lowercase hex digits>")),
});

/// UTC timestamp with exactly millisecond precision: `YYYY-MM-DDTHH:MM:SS.mmmZ`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Timestamp(String);
string_scalar!(Timestamp, |s| {
    let b = s.as_bytes();
    let shape = b.len() == 24 && b[10] == b'T' && b[19] == b'.' && b[23] == b'Z';
    let valid =
        time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).is_ok();
    if shape && valid {
        Ok(())
    } else {
        Err(error("timestamp", "expected YYYY-MM-DDTHH:MM:SS.mmmZ"))
    }
});

/// Exact decimal in canonical form, for example `28` or `-0.5`.
///
/// Rejects leading zeros, trailing fractional zeros, `-0`, signs other than a
/// leading `-`, and exponents. At most 12 integer and 9 fractional digits.
/// A shared library type for domain payloads; core records do not use it in v1.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Decimal(String);
string_scalar!(Decimal, |s| {
    let body = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match body.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (body, None),
    };
    let int_ok = !int.is_empty()
        && int.len() <= 12
        && int.bytes().all(|b| b.is_ascii_digit())
        && (int == "0" || !int.starts_with('0'));
    let frac_ok = frac.is_none_or(|f| {
        !f.is_empty() && f.len() <= 9 && f.bytes().all(|b| b.is_ascii_digit()) && !f.ends_with('0')
    });
    if int_ok && frac_ok && s != "-0" {
        Ok(())
    } else {
        Err(error("decimal", "not a canonical decimal"))
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    fn ok<T: TryFrom<String>>(s: &str) -> bool {
        T::try_from(s.to_owned()).is_ok()
    }

    #[test]
    fn text_rejects_instead_of_trimming() {
        assert!(ok::<Text<5>>("abc"));
        assert!(ok::<Text<5>>("a\nb"));
        assert!(ok::<Text<2>>("°C"), "limit counts characters, not bytes");
        for bad in ["", " a", "a ", "\ta", "a\tb", "a\u{0}b", "abcdef"] {
            assert!(!ok::<Text<5>>(bad), "{bad:?}");
        }
    }

    #[test]
    fn text_is_not_unicode_normalized() {
        let composed = Text::<10>::try_from("\u{e9}".to_owned()).unwrap();
        let decomposed = Text::<10>::try_from("e\u{301}".to_owned()).unwrap();
        assert_ne!(composed, decomposed);
        assert_eq!(decomposed.as_str(), "e\u{301}");
    }

    #[test]
    fn domain_id() {
        for good in ["core", "electronics", "a", "a1_b", &"a".repeat(32)] {
            assert!(ok::<DomainId>(good), "{good}");
        }
        for bad in ["", "1a", "_a", "A", "a-b", "a.b", &"a".repeat(33)] {
            assert!(!ok::<DomainId>(bad), "{bad}");
        }
    }

    #[test]
    fn kind() {
        let k = Kind::try_from("embedded.arduino_cli.compile".to_owned()).unwrap();
        assert_eq!(k.domain(), "embedded");
        assert_eq!(k.name(), "arduino_cli.compile");
        for bad in [
            "core",
            ".a",
            "a.",
            "a..b",
            "A.b",
            "a.B",
            "a.1b",
            "a-b.c",
            &format!("a.{}", "b".repeat(127)),
        ] {
            assert!(!ok::<Kind>(bad), "{bad}");
        }
    }

    #[test]
    fn project_path() {
        for good in [
            "a",
            "docs/wiring.md",
            "a/b/c.ino",
            ".hidden/file",
            "a b/ü.txt",
        ] {
            assert!(ok::<ProjectPath>(good), "{good}");
        }
        for bad in [
            "",
            "/abs",
            "a/../b",
            "..",
            "./a",
            "a/./b",
            "a//b",
            "a/",
            "a\\b",
            "a\u{0}b",
            "a\nb",
            &"a".repeat(256),
            &"a/".repeat(600),
        ] {
            assert!(!ok::<ProjectPath>(bad), "{bad:?}");
        }
    }

    #[test]
    fn digest() {
        assert!(ok::<Digest>(&format!("sha256:{}", "0a".repeat(32))));
        for bad in [
            format!("sha256:{}", "0A".repeat(32)),
            format!("sha256:{}", "0".repeat(63)),
            format!("sha512:{}", "0".repeat(64)),
            "0".repeat(64),
        ] {
            assert!(!ok::<Digest>(&bad), "{bad}");
        }
    }

    #[test]
    fn timestamp() {
        assert!(ok::<Timestamp>("2026-11-27T10:00:00.000Z"));
        for bad in [
            "2026-11-27T10:00:00Z",
            "2026-11-27T10:00:00.0000Z",
            "2026-11-27T10:00:00.000+00:00",
            "2026-11-27 10:00:00.000Z",
            "2026-02-30T10:00:00.000Z",
            "2026-11-27T24:00:00.000Z",
        ] {
            assert!(!ok::<Timestamp>(bad), "{bad}");
        }
    }

    #[test]
    fn decimal_is_canonical() {
        for good in ["0", "28", "-0.5", "2.25", "999999999999.999999999"] {
            assert!(ok::<Decimal>(good), "{good}");
        }
        for bad in [
            "",
            "-",
            "028",
            "28.0",
            "-0",
            "-0.0",
            "1e3",
            ".5",
            "5.",
            "+1",
            "1,5",
            " 1",
            "1000000000000",
            "0.1234567891",
        ] {
            assert!(!ok::<Decimal>(bad), "{bad}");
        }
    }

    #[test]
    fn serde_uses_validation() {
        assert!(serde_json::from_str::<Kind>("\"core.document\"").is_ok());
        let error = serde_json::from_str::<Kind>("\"Core.document\"").unwrap_err();
        assert!(error.to_string().contains("invalid kind"), "{error}");
    }
}
