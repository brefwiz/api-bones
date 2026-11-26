// SPDX-License-Identifier: MIT
//! The JSON codec of the values that cross the wasm boundary.
//!
//! A value crosses as JSON text in both directions. [`Wire`] is implemented
//! once for each type of the allowed set, and composes through `Option`, `Vec`
//! and `BTreeMap`, so a generated record encodes field by field with no
//! per-type glue.
//!
//! | Rust | JSON |
//! |---|---|
//! | `bool`, `i32`, `u32`, `String` | boolean, number, number, string |
//! | `f64` | number, or `"NaN"`, `"Infinity"`, `"-Infinity"` |
//! | `i64`, `u64` | decimal string |
//! | `Vec<u8>` | standard padded base64 string |
//! | `SystemTime` | RFC 3339 UTC `Z` timestamp, up to nanoseconds |
//! | `Duration` | whole milliseconds as a number, at most 2^53-1 |
//! | `Option<T>` | value or `null` |
//! | `Vec<T>`, `BTreeMap<String, T>` | array, object |

use std::collections::BTreeMap;
use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Number, Value};

/// The largest whole number a JavaScript `number` holds exactly.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
/// `0001-01-01T00:00:00Z`, the first instant of the RFC 3339 range.
const MIN_SECONDS: i64 = -62_135_596_800;
/// `9999-12-31T23:59:59Z`, the last whole second of the RFC 3339 range.
const MAX_SECONDS: i64 = 253_402_300_799;
const NANOS_PER_SECOND: u32 = 1_000_000_000;

/// A value that does not fit the wire form it was read as or written to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadValue {
    message: String,
}

impl BadValue {
    /// A refusal that carries `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The text of the refusal.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The same refusal, located at `segment` of an enclosing value.
    #[must_use]
    pub fn within(self, segment: &str) -> Self {
        Self {
            message: format!("{segment}: {}", self.message),
        }
    }
}

impl fmt::Display for BadValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BadValue {}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn expected(what: &str, found: &Value) -> BadValue {
    BadValue::new(format!("expected {what}, found {}", kind_of(found)))
}

/// A value of the allowed set, as the wire carries it.
pub trait Wire: Sized {
    /// The JSON form of the value.
    ///
    /// # Errors
    /// The value is outside what its wire form holds.
    fn encode(&self) -> Result<Value, BadValue>;

    /// The value a JSON form stands for.
    ///
    /// # Errors
    /// `value` is not the wire form of this type.
    fn decode(value: &Value) -> Result<Self, BadValue>;
}

/// The JSON text of `value`.
///
/// # Errors
/// The value is outside what its wire form holds.
pub fn to_json_string<T: Wire>(value: &T) -> Result<String, BadValue> {
    let json = value.encode()?;
    Ok(json.to_string())
}

/// The value JSON text stands for.
///
/// # Errors
/// The text is not JSON, or not the wire form of `T`.
pub fn from_json_str<T: Wire>(text: &str) -> Result<T, BadValue> {
    let json: Value =
        serde_json::from_str(text).map_err(|e| BadValue::new(format!("not JSON: {e}")))?;
    T::decode(&json)
}

/// The members of an arguments object, keyed by parameter name.
///
/// # Errors
/// The text is not a JSON object.
pub fn parse_args(text: &str) -> Result<Map<String, Value>, BadValue> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(members)) => Ok(members),
        Ok(other) => Err(expected("an object of arguments", &other)),
        Err(e) => Err(BadValue::new(format!("not JSON: {e}"))),
    }
}

/// The members of a client options object, keyed by option name.
///
/// An option the client does not take is refused, so a misspelled or stale
/// option fails the constructor instead of being ignored.
///
/// # Errors
/// The text is not a JSON object, or names a member outside `known`.
pub fn parse_options(text: &str, known: &[&str]) -> Result<Map<String, Value>, BadValue> {
    let members = parse_args(text)?;
    match members.keys().find(|key| !known.contains(&key.as_str())) {
        Some(key) => Err(BadValue::new(format!("unknown option `{key}`"))),
        None => Ok(members),
    }
}

/// The member `name` of `members` as a `T`. An absent member reads as `null`,
/// so only an `Option` accepts it.
///
/// # Errors
/// The member is not the wire form of `T`.
pub fn field<T: Wire>(members: &Map<String, Value>, name: &str) -> Result<T, BadValue> {
    T::decode(members.get(name).unwrap_or(&Value::Null)).map_err(|e| e.within(name))
}

impl Wire for bool {
    fn encode(&self) -> Result<Value, BadValue> {
        Ok(Value::Bool(*self))
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        value.as_bool().ok_or_else(|| expected("a boolean", value))
    }
}

impl Wire for i32 {
    fn encode(&self) -> Result<Value, BadValue> {
        Ok(Value::from(*self))
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        value
            .as_i64()
            .and_then(|n| i32::try_from(n).ok())
            .ok_or_else(|| expected("a 32-bit integer", value))
    }
}

impl Wire for u32 {
    fn encode(&self) -> Result<Value, BadValue> {
        Ok(Value::from(*self))
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| expected("an unsigned 32-bit integer", value))
    }
}

/// The JSON form of a double: a number, or a word for a value JSON cannot hold.
#[must_use]
pub fn double_value(x: f64) -> Value {
    if x.is_nan() {
        Value::String("NaN".to_owned())
    } else if x.is_infinite() {
        Value::String(if x > 0.0 { "Infinity" } else { "-Infinity" }.to_owned())
    } else {
        Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}

impl Wire for f64 {
    fn encode(&self) -> Result<Value, BadValue> {
        Ok(double_value(*self))
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        let parsed = match value {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => match s.as_str() {
                "NaN" => Some(Self::NAN),
                "Infinity" => Some(Self::INFINITY),
                "-Infinity" => Some(Self::NEG_INFINITY),
                _ => None,
            },
            _ => None,
        };
        parsed.ok_or_else(|| expected("a number", value))
    }
}

impl Wire for String {
    fn encode(&self) -> Result<Value, BadValue> {
        Ok(Value::String(self.clone()))
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| expected("a string", value))
    }
}

/// A 64-bit integer crosses as a decimal string.
macro_rules! decimal_string_wire {
    ($int:ty, $what:literal) => {
        impl Wire for $int {
            fn encode(&self) -> Result<Value, BadValue> {
                Ok(Value::String(self.to_string()))
            }

            fn decode(value: &Value) -> Result<Self, BadValue> {
                value
                    .as_str()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| expected($what, value))
            }
        }
    };
}

decimal_string_wire!(i64, "a decimal string of a 64-bit integer");
decimal_string_wire!(u64, "a decimal string of an unsigned 64-bit integer");

impl Wire for Vec<u8> {
    fn encode(&self) -> Result<Value, BadValue> {
        Ok(Value::String(STANDARD.encode(self)))
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        value
            .as_str()
            .and_then(|s| STANDARD.decode(s).ok())
            .ok_or_else(|| expected("a padded base64 string", value))
    }
}

impl Wire for Duration {
    fn encode(&self) -> Result<Value, BadValue> {
        u64::try_from(self.as_millis())
            .ok()
            .filter(|ms| *ms <= MAX_SAFE_INTEGER)
            .map(Value::from)
            .ok_or_else(|| BadValue::new("a duration above 2^53-1 milliseconds"))
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        let ms = value
            .as_u64()
            .ok_or_else(|| expected("a whole number of milliseconds", value))?;
        if ms > MAX_SAFE_INTEGER {
            return Err(BadValue::new("a duration above 2^53-1 milliseconds"));
        }
        Ok(Self::from_millis(ms))
    }
}

impl<T: Wire> Wire for Option<T> {
    fn encode(&self) -> Result<Value, BadValue> {
        self.as_ref().map_or(Ok(Value::Null), T::encode)
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        if value.is_null() {
            Ok(None)
        } else {
            T::decode(value).map(Some)
        }
    }
}

impl<T: Wire> Wire for Vec<T> {
    fn encode(&self) -> Result<Value, BadValue> {
        self.iter()
            .enumerate()
            .map(|(i, item)| item.encode().map_err(|e| e.within(&format!("[{i}]"))))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array)
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        value
            .as_array()
            .ok_or_else(|| expected("an array", value))?
            .iter()
            .enumerate()
            .map(|(i, item)| T::decode(item).map_err(|e| e.within(&format!("[{i}]"))))
            .collect()
    }
}

impl<T: Wire> Wire for BTreeMap<String, T> {
    fn encode(&self) -> Result<Value, BadValue> {
        self.iter()
            .map(|(key, item)| {
                item.encode()
                    .map(|json| (key.clone(), json))
                    .map_err(|e| e.within(key))
            })
            .collect::<Result<Map<_, _>, _>>()
            .map(Value::Object)
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        value
            .as_object()
            .ok_or_else(|| expected("an object", value))?
            .iter()
            .map(|(key, item)| {
                T::decode(item)
                    .map(|decoded| (key.clone(), decoded))
                    .map_err(|e| e.within(key))
            })
            .collect()
    }
}

impl Wire for SystemTime {
    fn encode(&self) -> Result<Value, BadValue> {
        let (seconds, nanos) = system_time_parts(*self);
        format_rfc3339(seconds, nanos).map(Value::String)
    }

    fn decode(value: &Value) -> Result<Self, BadValue> {
        let text = value
            .as_str()
            .ok_or_else(|| expected("an RFC 3339 UTC timestamp", value))?;
        let (seconds, nanos) = parse_rfc3339(text)?;
        Ok(system_time_from_parts(seconds, nanos))
    }
}

/// An instant as whole seconds from the Unix epoch, negative before it, and
/// the nanoseconds past that second. A distance beyond 63 bits of seconds
/// saturates, which [`format_rfc3339`] refuses.
#[must_use]
pub fn system_time_parts(time: SystemTime) -> (i64, u32) {
    let seconds_of = |d: Duration| i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
    match time.duration_since(UNIX_EPOCH) {
        Ok(after) => (seconds_of(after), after.subsec_nanos()),
        Err(before) => {
            let before = before.duration();
            match before.subsec_nanos() {
                0 => (-seconds_of(before), 0),
                nanos => (-seconds_of(before) - 1, NANOS_PER_SECOND - nanos),
            }
        }
    }
}

/// The instant `seconds` and `nanos` past the Unix epoch. `seconds` lies in
/// the RFC 3339 range that [`parse_rfc3339`] and [`format_rfc3339`] hold to,
/// where the arithmetic cannot overflow.
#[must_use]
pub fn system_time_from_parts(seconds: i64, nanos: u32) -> SystemTime {
    let nanos = Duration::new(0, nanos);
    if seconds >= 0 {
        UNIX_EPOCH + Duration::from_secs(seconds.unsigned_abs()) + nanos
    } else {
        UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs()) + nanos
    }
}

/// The days from the Unix epoch of a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_of_year = (month + 9) % 12;
    let day_of_year = (153 * month_of_year + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The proleptic Gregorian date of a day count from the Unix epoch.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_of_year = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_of_year + 2) / 5 + 1;
    let month = if month_of_year < 10 {
        month_of_year + 3
    } else {
        month_of_year - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The RFC 3339 UTC text of an instant: `Z`, and a fraction of up to nine
/// digits only when the instant has one.
///
/// # Errors
/// The instant is outside `0001-01-01T00:00:00Z` to `9999-12-31T23:59:59Z`,
/// or `nanos` is a second or more.
pub fn format_rfc3339(seconds: i64, nanos: u32) -> Result<String, BadValue> {
    if !(MIN_SECONDS..=MAX_SECONDS).contains(&seconds) || nanos >= NANOS_PER_SECOND {
        return Err(BadValue::new("a timestamp outside the RFC 3339 range"));
    }
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let time_of_day = seconds.rem_euclid(86_400);
    let mut text = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        time_of_day / 3600,
        time_of_day % 3600 / 60,
        time_of_day % 60
    );
    if nanos != 0 {
        text.push('.');
        text.push_str(format!("{nanos:09}").trim_end_matches('0'));
    }
    text.push('Z');
    Ok(text)
}

/// The digits of `text` as a number, or `None` when it is empty or holds
/// anything else.
fn digits(text: &str) -> Option<i64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The whole seconds from the Unix epoch and the nanoseconds of an RFC 3339
/// UTC timestamp. The zone is `Z`, the fraction has one to nine digits.
///
/// # Errors
/// The text is not such a timestamp, or names a date that does not exist.
pub fn parse_rfc3339(text: &str) -> Result<(i64, u32), BadValue> {
    let bad = || {
        BadValue::new(format!(
            "expected an RFC 3339 UTC timestamp, found {text:?}"
        ))
    };
    let body = text.strip_suffix('Z').ok_or_else(bad)?;
    let (head, fraction) = match body.split_once('.') {
        Some((head, fraction)) => (head, Some(fraction)),
        None => (body, None),
    };
    let separators = [(4, "-"), (7, "-"), (10, "T"), (13, ":"), (16, ":")];
    if head.len() != 19
        || !separators
            .iter()
            .all(|&(at, sep)| head.get(at..=at) == Some(sep))
    {
        return Err(bad());
    }
    let part = |from: usize, to: usize| head.get(from..to).and_then(digits);
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        part(0, 4),
        part(5, 7),
        part(8, 10),
        part(11, 13),
        part(14, 16),
        part(17, 19),
    ) else {
        return Err(bad());
    };
    let nanos = match fraction {
        None => 0,
        Some(fraction) if fraction.len() <= 9 => digits(fraction)
            .and_then(|_| format!("{fraction:0<9}").parse::<u32>().ok())
            .ok_or_else(bad)?,
        Some(_) => return Err(bad()),
    };
    let valid = (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour < 24
        && minute < 60
        && second < 60;
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    if !valid || !(MIN_SECONDS..=MAX_SECONDS).contains(&seconds) {
        return Err(bad());
    }
    Ok((seconds, nanos))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn round_trip<T: Wire + PartialEq + fmt::Debug>(value: &T, json: &str) {
        assert_eq!(to_json_string(value).unwrap(), json);
        assert_eq!(&from_json_str::<T>(json).unwrap(), value);
    }

    fn refuses<T: Wire + fmt::Debug>(json: &str) -> String {
        from_json_str::<T>(json).unwrap_err().to_string()
    }

    #[test]
    fn bool_crosses_as_a_boolean() {
        round_trip(&true, "true");
        round_trip(&false, "false");
        assert_eq!(refuses::<bool>("1"), "expected a boolean, found a number");
        assert_eq!(refuses::<bool>("null"), "expected a boolean, found null");
    }

    #[test]
    fn i32_and_u32_cross_as_numbers_within_their_range() {
        round_trip(&-7_i32, "-7");
        round_trip(&i32::MAX, "2147483647");
        round_trip(&7_u32, "7");
        round_trip(&u32::MAX, "4294967295");
        assert!(refuses::<i32>("2147483648").contains("32-bit"));
        assert!(refuses::<i32>("\"1\"").contains("found a string"));
        assert!(refuses::<i32>("1.5").contains("32-bit"));
        assert!(refuses::<u32>("-1").contains("unsigned 32-bit"));
        assert!(refuses::<u32>("4294967296").contains("unsigned 32-bit"));
    }

    #[test]
    fn f64_crosses_as_a_number_or_a_word_for_a_non_finite_value() {
        round_trip(&1.5_f64, "1.5");
        round_trip(&-0.25_f64, "-0.25");
        assert_eq!(to_json_string(&f64::INFINITY).unwrap(), "\"Infinity\"");
        assert_eq!(to_json_string(&f64::NEG_INFINITY).unwrap(), "\"-Infinity\"");
        assert_eq!(to_json_string(&f64::NAN).unwrap(), "\"NaN\"");
        assert_eq!(from_json_str::<f64>("\"Infinity\"").unwrap(), f64::INFINITY);
        assert_eq!(
            from_json_str::<f64>("\"-Infinity\"").unwrap(),
            f64::NEG_INFINITY
        );
        assert!(from_json_str::<f64>("\"NaN\"").unwrap().is_nan());
        assert!(refuses::<f64>("\"1.5\"").contains("expected a number"));
        assert!(refuses::<f64>("true").contains("expected a number"));
    }

    #[test]
    fn string_crosses_as_a_string() {
        round_trip(&"héllo \"x\"".to_owned(), "\"héllo \\\"x\\\"\"");
        assert!(refuses::<String>("3").contains("expected a string"));
    }

    #[test]
    fn i64_and_u64_cross_as_decimal_strings() {
        round_trip(&i64::MIN, "\"-9223372036854775808\"");
        round_trip(&i64::MAX, "\"9223372036854775807\"");
        round_trip(&u64::MAX, "\"18446744073709551615\"");
        assert!(refuses::<i64>("5").contains("decimal string"));
        assert!(refuses::<i64>("\"9223372036854775808\"").contains("decimal string"));
        assert!(refuses::<i64>("\"x\"").contains("decimal string"));
        assert!(refuses::<u64>("5").contains("decimal string"));
        assert!(refuses::<u64>("\"-1\"").contains("decimal string"));
    }

    #[test]
    fn bytes_cross_as_padded_standard_base64() {
        round_trip(&vec![0xfb_u8, 0xff, 0x00], "\"+/8A\"");
        round_trip(&vec![1_u8], "\"AQ==\"");
        round_trip(&Vec::<u8>::new(), "\"\"");
        assert!(refuses::<Vec<u8>>("\"AQ\"").contains("padded base64"));
        assert!(refuses::<Vec<u8>>("\"-_8A\"").contains("padded base64"));
        assert!(refuses::<Vec<u8>>("[1]").contains("padded base64"));
    }

    #[test]
    fn duration_crosses_as_whole_milliseconds() {
        round_trip(&Duration::from_millis(1500), "1500");
        round_trip(&Duration::ZERO, "0");
        round_trip(&Duration::from_millis(MAX_SAFE_INTEGER), "9007199254740991");
        assert_eq!(
            to_json_string(&Duration::new(2, 999_999)).unwrap(),
            "2000",
            "a sub-millisecond remainder truncates toward zero"
        );
    }

    #[test]
    fn duration_above_the_exact_range_is_refused_both_ways() {
        assert!(refuses::<Duration>("9007199254740992").contains("above 2^53-1"));
        assert!(refuses::<Duration>("-1").contains("whole number of milliseconds"));
        assert!(refuses::<Duration>("1.5").contains("whole number of milliseconds"));
        assert!(refuses::<Duration>("\"5\"").contains("whole number of milliseconds"));
        let too_long = Duration::from_millis(MAX_SAFE_INTEGER + 1);
        assert!(
            to_json_string(&too_long)
                .unwrap_err()
                .message()
                .contains("above 2^53-1")
        );
        assert!(
            Duration::MAX
                .encode()
                .unwrap_err()
                .message()
                .contains("above 2^53-1")
        );
    }

    #[test]
    fn option_crosses_as_the_value_or_null() {
        round_trip(&Some(3_i32), "3");
        round_trip(&None::<i32>, "null");
        round_trip(&Some(Some("a".to_owned())), "\"a\"");
        assert!(refuses::<Option<i32>>("\"3\"").contains("32-bit"));
    }

    #[test]
    fn vec_and_map_compose_with_their_elements() {
        round_trip(&vec![1_i32, 2, 3], "[1,2,3]");
        round_trip(&vec![Some(1_i64), None], "[\"1\",null]");
        round_trip(&Some(vec![Some(vec![true])]), "[[true]]");
        let mut map = BTreeMap::new();
        map.insert("a".to_owned(), 1_u32);
        map.insert("b".to_owned(), 2);
        round_trip(&map, "{\"a\":1,\"b\":2}");
        assert!(refuses::<Vec<i32>>("{}").contains("expected an array"));
        assert!(refuses::<BTreeMap<String, i32>>("[]").contains("expected an object"));
    }

    #[test]
    fn a_nested_refusal_names_where_it_happened() {
        assert_eq!(
            refuses::<Vec<bool>>("[true, 1]"),
            "[1]: expected a boolean, found a number"
        );
        assert_eq!(
            refuses::<BTreeMap<String, Vec<u32>>>("{\"k\": [1, -1]}"),
            "k: [1]: expected an unsigned 32-bit integer, found a number"
        );
        let mut map = BTreeMap::new();
        map.insert("k".to_owned(), vec![Duration::MAX]);
        assert!(
            to_json_string(&map)
                .unwrap_err()
                .to_string()
                .starts_with("k: [0]: ")
        );
        assert!(
            to_json_string(&vec![Duration::MAX])
                .unwrap_err()
                .to_string()
                .starts_with("[0]: ")
        );
    }

    #[test]
    fn text_that_is_not_json_is_refused() {
        assert!(refuses::<bool>("{").starts_with("not JSON: "));
        assert!(
            parse_args("{")
                .unwrap_err()
                .to_string()
                .starts_with("not JSON: ")
        );
    }

    #[test]
    fn arguments_are_an_object_read_by_member_name() {
        let args = parse_args("{\"id\":\"7\",\"note\":null,\"tags\":[\"a\"]}").unwrap();
        assert_eq!(field::<i64>(&args, "id").unwrap(), 7);
        assert_eq!(field::<Option<String>>(&args, "note").unwrap(), None);
        assert_eq!(field::<Option<String>>(&args, "absent").unwrap(), None);
        assert_eq!(
            field::<Vec<String>>(&args, "tags").unwrap(),
            vec!["a".to_owned()]
        );
        assert_eq!(
            field::<String>(&args, "absent").unwrap_err().to_string(),
            "absent: expected a string, found null"
        );
        assert_eq!(
            field::<u32>(&args, "id").unwrap_err().to_string(),
            "id: expected an unsigned 32-bit integer, found a string"
        );
        assert_eq!(
            parse_args("[]").unwrap_err().to_string(),
            "expected an object of arguments, found an array"
        );
    }

    #[test]
    fn options_are_an_object_of_known_members() {
        let known = ["defaultPageSize", "transport"];
        let options = parse_options("{\"defaultPageSize\":7}", &known).unwrap();
        assert_eq!(
            field::<Option<u32>>(&options, "defaultPageSize").unwrap(),
            Some(7)
        );
        assert_eq!(field::<Option<u32>>(&options, "transport").unwrap(), None);
        assert!(parse_options("{}", &known).unwrap().is_empty());
        assert!(parse_options("{}", &[]).unwrap().is_empty());
    }

    #[test]
    fn an_option_outside_the_known_set_or_a_non_object_is_refused() {
        assert_eq!(
            parse_options("{\"pageSize\":7}", &["defaultPageSize"])
                .unwrap_err()
                .to_string(),
            "unknown option `pageSize`"
        );
        assert_eq!(
            parse_options("{\"x\":1}", &[]).unwrap_err().to_string(),
            "unknown option `x`"
        );
        assert_eq!(
            parse_options("[]", &[]).unwrap_err().to_string(),
            "expected an object of arguments, found an array"
        );
        assert!(
            parse_options("", &[])
                .unwrap_err()
                .to_string()
                .starts_with("not JSON: ")
        );
    }

    #[test]
    fn the_message_of_a_refusal_is_its_display() {
        let bad = BadValue::new("x");
        assert_eq!(bad.message(), "x");
        assert_eq!(bad.to_string(), "x");
        let as_error: &dyn std::error::Error = &bad;
        assert!(as_error.source().is_none());
    }

    fn time(seconds: i64, nanos: u32) -> SystemTime {
        system_time_from_parts(seconds, nanos)
    }

    #[test]
    fn timestamp_crosses_as_rfc_3339_utc() {
        round_trip(&UNIX_EPOCH, "\"1970-01-01T00:00:00Z\"");
        round_trip(&time(1_000_000_000, 0), "\"2001-09-09T01:46:40Z\"");
        round_trip(&time(1_709_164_800, 0), "\"2024-02-29T00:00:00Z\"");
        round_trip(
            &time(1_709_164_800 + 45_296, 123_456_789),
            "\"2024-02-29T12:34:56.123456789Z\"",
        );
        round_trip(&time(0, 500_000_000), "\"1970-01-01T00:00:00.5Z\"");
        round_trip(&time(0, 1), "\"1970-01-01T00:00:00.000000001Z\"");
        round_trip(&time(-1, 500_000_000), "\"1969-12-31T23:59:59.5Z\"");
        round_trip(&time(-86_400, 0), "\"1969-12-31T00:00:00Z\"");
        round_trip(&time(MIN_SECONDS, 0), "\"0001-01-01T00:00:00Z\"");
        round_trip(
            &time(MAX_SECONDS, 999_999_999),
            "\"9999-12-31T23:59:59.999999999Z\"",
        );
    }

    #[test]
    fn a_leap_day_exists_only_in_a_leap_year() {
        assert!(parse_rfc3339("2000-02-29T00:00:00Z").is_ok());
        assert!(parse_rfc3339("2024-02-29T00:00:00Z").is_ok());
        assert!(parse_rfc3339("1900-02-29T00:00:00Z").is_err());
        assert!(parse_rfc3339("2023-02-29T00:00:00Z").is_err());
        assert!(parse_rfc3339("2023-02-28T00:00:00Z").is_ok());
        assert!(parse_rfc3339("2023-04-30T00:00:00Z").is_ok());
        assert!(parse_rfc3339("2023-04-31T00:00:00Z").is_err());
        assert!(parse_rfc3339("2023-01-31T00:00:00Z").is_ok());
    }

    #[test]
    fn every_calendar_day_round_trips_through_the_day_count() {
        for days in (-800_000..2_900_000).step_by(997) {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
        }
    }

    #[test]
    fn a_timestamp_that_is_not_utc_rfc_3339_is_refused() {
        let refused = [
            "",
            "2024-02-29",
            "2024-02-29T00:00:00",
            "2024-02-29T00:00:00+00:00",
            "2024-02-29t00:00:00Z",
            "2024-02-29 00:00:00Z",
            "2024-02-29T00:00:00z",
            "2024-02-29T00:00:00.Z",
            "2024-02-29T00:00:00.1234567890Z",
            "2024-02-29T00:00:00.1a3Z",
            "2024-02-29T00:00Z",
            "2024-13-01T00:00:00Z",
            "2024-00-01T00:00:00Z",
            "2024-01-00T00:00:00Z",
            "2024-01-01T24:00:00Z",
            "2024-01-01T00:60:00Z",
            "2024-01-01T00:00:60Z",
            "0000-12-31T23:59:59Z",
            "20x4-01-01T00:00:00Z",
            "2024-01-01T00:00:+1Z",
            "2024-01-0éT00:00:00Z",
        ];
        for text in refused {
            let error = parse_rfc3339(text).unwrap_err();
            assert!(
                error
                    .message()
                    .starts_with("expected an RFC 3339 UTC timestamp"),
                "{text}: {error}"
            );
        }
        assert!(refuses::<SystemTime>("7").contains("an RFC 3339 UTC timestamp"));
        assert!(refuses::<SystemTime>("\"yesterday\"").contains("an RFC 3339 UTC timestamp"));
    }

    #[test]
    fn a_timestamp_outside_the_range_is_refused_on_encode() {
        assert!(format_rfc3339(MAX_SECONDS + 1, 0).is_err());
        assert!(format_rfc3339(MIN_SECONDS - 1, 0).is_err());
        assert!(format_rfc3339(0, NANOS_PER_SECOND).is_err());
        let far = UNIX_EPOCH + Duration::from_secs(300_000_000_000);
        assert!(
            far.encode()
                .unwrap_err()
                .message()
                .contains("RFC 3339 range")
        );
    }

    #[test]
    fn instants_split_into_seconds_and_nanoseconds_around_the_epoch() {
        assert_eq!(system_time_parts(UNIX_EPOCH), (0, 0));
        assert_eq!(system_time_parts(time(5, 6)), (5, 6));
        assert_eq!(system_time_parts(time(-5, 0)), (-5, 0));
        assert_eq!(system_time_parts(time(-5, 250)), (-5, 250));
    }
}
