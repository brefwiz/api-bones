// SPDX-License-Identifier: MIT
//! Typed, bounded message arguments: one definition shared with the
//! TypeScript lane.

use std::collections::BTreeMap;

/// The most arguments one error carries.
pub const MAX_ARGS: usize = 32;
/// The longest argument key.
pub const MAX_KEY_LEN: usize = 64;
/// The longest string argument, in UTF-8 bytes.
pub const MAX_STRING_BYTES: usize = 1024;
const MIN_SECONDS: i64 = -62_135_596_800;
const MAX_SECONDS: i64 = 253_402_300_799;

/// One typed argument value.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgValue {
    /// Text, at most [`MAX_STRING_BYTES`] bytes.
    String(String),
    /// A whole number.
    Int64(i64),
    /// A finite real number.
    Double(f64),
    /// An instant as `google.protobuf.Timestamp` carries it.
    Timestamp {
        /// Seconds since the Unix epoch.
        seconds: i64,
        /// Nanoseconds within the second.
        nanos: i32,
    },
}

/// The bound an argument broke. [`ArgRule::as_str`] is the Gherkin rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgRule {
    /// More than [`MAX_ARGS`] arguments.
    TooManyArgs,
    /// A key outside `^[a-z][a-z0-9_]{0,63}$`.
    ArgKeyFormat,
    /// A string longer than [`MAX_STRING_BYTES`].
    StringTooLong,
    /// A control or bidirectional-formatting code point in a string.
    StringForbiddenChar,
    /// A NaN or infinite double.
    NonFiniteDouble,
    /// A timestamp outside `0001-01-01..=9999-12-31` or with nanos outside `0..=999_999_999`.
    TimestampRange,
    /// Arguments that do not decode at all: a shape the wire mapping does not
    /// define. Never raised by construction.
    Malformed,
}

impl ArgRule {
    /// The rule name the shared contract uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TooManyArgs => "too_many_args",
            Self::ArgKeyFormat => "arg_key_format",
            Self::StringTooLong => "string_too_long",
            Self::StringForbiddenChar => "string_forbidden_char",
            Self::NonFiniteDouble => "non_finite_double",
            Self::TimestampRange => "timestamp_range",
            Self::Malformed => "malformed_args",
        }
    }
}

/// An argument, or argument set, outside the bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgError {
    /// The first rule that failed.
    pub rule: ArgRule,
}

impl core::fmt::Display for ArgError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "argument out of bounds: {}", self.rule.as_str())
    }
}

impl core::error::Error for ArgError {}

/// A checked argument map in ascending key order. Nothing is truncated.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Args(BTreeMap<String, ArgValue>);

impl Args {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one argument, replacing one of the same key.
    ///
    /// # Errors
    ///
    /// [`ArgError`] with the first rule broken: the count, then the key, then
    /// the value.
    pub fn insert(&mut self, key: &str, value: ArgValue) -> Result<(), ArgError> {
        if !self.0.contains_key(key) && self.0.len() >= MAX_ARGS {
            return Err(ArgError {
                rule: ArgRule::TooManyArgs,
            });
        }
        check(key, &value)?;
        self.0.insert(key.to_owned(), value);
        Ok(())
    }

    /// Build a set from decoded entries, applying the bounds in contract order.
    ///
    /// # Errors
    ///
    /// [`ArgError`] with the first rule broken: the count, then each argument
    /// in ascending key byte order.
    pub fn from_entries(
        entries: impl IntoIterator<Item = (String, ArgValue)>,
    ) -> Result<Self, ArgError> {
        let map: BTreeMap<String, ArgValue> = entries.into_iter().collect();
        if map.len() > MAX_ARGS {
            return Err(ArgError {
                rule: ArgRule::TooManyArgs,
            });
        }
        map.iter().try_for_each(|(key, value)| check(key, value))?;
        Ok(Self(map))
    }

    /// The argument named `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&ArgValue> {
        self.0.get(key)
    }

    /// The number of arguments.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no arguments.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Build the arguments of one error from `(key, value)` pairs, checking each
/// in input order: the first pair that breaks a rule names the rule, so a
/// caller reports what it wrote first. A repeated key replaces the earlier
/// value and is not a new argument.
///
/// # Errors
///
/// [`ArgError`] with the rule the first offending pair broke: the count, then
/// the key, then the value.
pub fn error_args<'a, I>(entries: I) -> Result<Args, ArgError>
where
    I: IntoIterator<Item = (&'a str, ArgValue)>,
{
    let mut args = Args::new();
    for (key, value) in entries {
        args.insert(key, value)?;
    }
    Ok(args)
}

pub(crate) fn key_ok(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= MAX_KEY_LEN
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn forbidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn check(key: &str, value: &ArgValue) -> Result<(), ArgError> {
    let fail = |rule| Err(ArgError { rule });
    if !key_ok(key) {
        return fail(ArgRule::ArgKeyFormat);
    }
    match value {
        ArgValue::String(s) if s.len() > MAX_STRING_BYTES => fail(ArgRule::StringTooLong),
        ArgValue::String(s) if s.chars().any(forbidden) => fail(ArgRule::StringForbiddenChar),
        ArgValue::Double(d) if !d.is_finite() => fail(ArgRule::NonFiniteDouble),
        ArgValue::Timestamp { seconds, nanos }
            if !(MIN_SECONDS..=MAX_SECONDS).contains(seconds)
                || !(0..=999_999_999).contains(nanos) =>
        {
            fail(ArgRule::TimestampRange)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(key: &str, value: ArgValue) -> Option<&'static str> {
        Args::new()
            .insert(key, value)
            .err()
            .map(|e| e.rule.as_str())
    }

    #[test]
    fn error_args_reports_the_first_offending_pair_in_input_order() {
        let bad_double = ("ratio", ArgValue::Double(f64::NAN));
        let bad_string = ("name", ArgValue::String("\u{7}".into()));
        let first = |entries: Vec<(&str, ArgValue)>| error_args(entries).unwrap_err().rule.as_str();
        assert_eq!(
            first(vec![bad_double.clone(), bad_string.clone()]),
            "non_finite_double"
        );
        assert_eq!(first(vec![bad_string, bad_double]), "string_forbidden_char");
        assert_eq!(
            first(vec![("Bad", ArgValue::Double(f64::NAN))]),
            "arg_key_format",
            "the key is checked before the value"
        );
    }

    #[test]
    fn error_args_bounds_the_count_at_32() {
        let keys: Vec<String> = (1..=33).map(|n| format!("k{n}")).collect();
        let take = |n: usize| {
            error_args(
                keys.iter()
                    .take(n)
                    .map(|k| (k.as_str(), ArgValue::Int64(1))),
            )
        };
        assert_eq!(take(32).unwrap().len(), 32);
        assert_eq!(take(33).unwrap_err().rule.as_str(), "too_many_args");
        let replaced = error_args(
            keys.iter()
                .take(32)
                .chain(keys.iter().take(1))
                .map(|k| (k.as_str(), ArgValue::Int64(2))),
        );
        assert_eq!(replaced.unwrap().len(), 32, "a repeated key is not new");
    }

    #[test]
    fn error_args_equals_folding_insert() {
        let entries = vec![
            ("a", ArgValue::Int64(1)),
            ("b", ArgValue::String("x".into())),
            ("a", ArgValue::Double(2.5)),
        ];
        let mut folded = Args::new();
        for (k, v) in entries.clone() {
            folded.insert(k, v).unwrap();
        }
        assert_eq!(error_args(entries).unwrap(), folded);
        assert!(error_args(Vec::new()).unwrap().is_empty());
    }

    #[test]
    fn accepts_inclusive_bounds() {
        let mut args = Args::new();
        for n in 1..=32 {
            args.insert(&format!("k{n}"), ArgValue::Int64(n)).unwrap();
        }
        assert_eq!(args.len(), 32);
        assert!(
            args.insert("k1", ArgValue::Int64(9)).is_ok(),
            "replacing is not a new key"
        );
        let long_key = format!("a{}", "b".repeat(63));
        assert_eq!(rule(&long_key, ArgValue::Int64(1)), None);
        assert_eq!(rule("n", ArgValue::String("é".repeat(512))), None);
        assert_eq!(rule("n", ArgValue::Double(-0.0)), None);
        let edge = ArgValue::Timestamp {
            seconds: 253_402_300_799,
            nanos: 999_999_999,
        };
        assert_eq!(rule("at", edge), None);
        let low = ArgValue::Timestamp {
            seconds: -62_135_596_800,
            nanos: 0,
        };
        assert_eq!(rule("at", low), None);
    }

    #[test]
    fn count_is_checked_before_the_key() {
        let mut args = Args::new();
        for n in 1..=32 {
            args.insert(&format!("k{n}"), ArgValue::Int64(n)).unwrap();
        }
        let err = args.insert("Bad", ArgValue::Int64(1)).unwrap_err();
        assert_eq!(err.rule, ArgRule::TooManyArgs);
    }

    #[test]
    fn refuses_each_rule() {
        for key in [
            "Count",
            "_count",
            "9count",
            "count-x",
            &format!("a{}", "b".repeat(64)),
        ] {
            assert_eq!(
                rule(key, ArgValue::Int64(1)),
                Some("arg_key_format"),
                "{key}"
            );
        }
        assert_eq!(
            rule("n", ArgValue::String("a".repeat(1025))),
            Some("string_too_long")
        );
        assert_eq!(
            rule("n", ArgValue::String("é".repeat(513))),
            Some("string_too_long")
        );
        for c in [
            '\u{7}', '\n', '\u{7f}', '\u{85}', '\u{61c}', '\u{200f}', '\u{202e}', '\u{2066}',
        ] {
            assert_eq!(
                rule("n", ArgValue::String(format!("a{c}b"))),
                Some("string_forbidden_char"),
                "{c:?}"
            );
        }
        for d in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(rule("n", ArgValue::Double(d)), Some("non_finite_double"));
        }
        for (seconds, nanos) in [
            (253_402_300_800, 0),
            (-62_135_596_801, 0),
            (0, 1_000_000_000),
            (0, -1),
        ] {
            assert_eq!(
                rule("at", ArgValue::Timestamp { seconds, nanos }),
                Some("timestamp_range")
            );
        }
    }

    #[test]
    fn from_entries_orders_the_checks() {
        let entries = (1..=33).map(|n| (format!("k{n}"), ArgValue::Int64(n)));
        assert_eq!(
            Args::from_entries(entries).unwrap_err().rule,
            ArgRule::TooManyArgs
        );
        let mixed = [
            ("b".to_owned(), ArgValue::Double(f64::NAN)),
            ("A".to_owned(), ArgValue::Int64(1)),
        ];
        assert_eq!(
            Args::from_entries(mixed).unwrap_err().rule,
            ArgRule::ArgKeyFormat
        );
        let ok = Args::from_entries([("a".to_owned(), ArgValue::Int64(1))]).unwrap();
        assert_eq!(ok.get("a"), Some(&ArgValue::Int64(1)));
        assert!(!ok.is_empty());
    }
}
