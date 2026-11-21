// SPDX-License-Identifier: MIT
//! The ICU `MessageFormat` subset catalog messages use: parsing and rendering
//! on ICU4X with `jiff` for the time zone.
//!
//! Supported placeholders: `{x}` (string), `{x, number}`,
//! `{x, number, percent}`, `{x, plural, =N {..} zero|one|two|few|many|other {..}}`
//! with `#`, `{x, select, word {..} other {..}}`, `{x, date, short|medium|long|full}`
//! and `{x, time, short|medium}`. Anything else does not parse. Argument
//! values are substituted as values and never parsed as patterns.

use std::fmt;

use fixed_decimal::{Decimal, FloatPrecision, SignedRoundingMode, UnsignedRoundingMode};
use icu_datetime::DateTimeFormatter;
use icu_datetime::fieldsets::{self, T, YMD, YMDE};
use icu_datetime::input::{Date, DateTime, Time};
use icu_datetime::options::TimePrecision;
use icu_decimal::DecimalFormatter;
use icu_decimal::options::DecimalFormatterOptions;
use icu_locale::Locale;
use icu_plurals::{PluralCategory, PluralRules};
use jiff::tz::TimeZone;

use crate::args::{ArgValue, Args, key_ok};

const MAX_DEPTH: usize = 16;
const HALF_EXPAND: SignedRoundingMode =
    SignedRoundingMode::Unsigned(UnsignedRoundingMode::HalfExpand);
const MAX_EXACT: i64 = 9_007_199_254_740_991;

/// Why a pattern did not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseErrorKind {
    /// A `{` or `}` without its partner, or a placeholder cut short.
    UnbalancedBraces,
    /// Syntax the subset does not have where a token was expected.
    Malformed,
    /// A placeholder name outside the argument key format.
    InvalidArgumentName,
    /// A placeholder type outside the subset (`selectordinal`, `spellout`, ...).
    UnsupportedType,
    /// A style outside the subset, a `::` skeleton, or a date/time without a style.
    UnsupportedStyle,
    /// A plural `offset:`.
    PluralOffset,
    /// A selector that is not `=N`, a plural category, or a select word.
    InvalidSelector,
    /// The same selector twice in one plural or select.
    DuplicateSelector,
    /// A plural or select without an `other` branch.
    MissingOther,
    /// Placeholders nested deeper than the parser accepts.
    NestingTooDeep,
}

/// A pattern outside the subset, with the character offset where it was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    /// What was wrong.
    pub kind: ParseErrorKind,
    /// The offset, in characters, into the pattern.
    pub offset: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "message pattern not supported at {}: {:?}",
            self.offset, self.kind
        )
    }
}

impl core::error::Error for ParseError {}

/// The argument kind a placeholder needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected {
    /// A string argument.
    String,
    /// An int64 or double argument.
    Number,
    /// A timestamp argument.
    Timestamp,
}

/// Why a parsed message did not render. Nothing is truncated or guessed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    /// The placeholder's argument is absent.
    MissingArg(String),
    /// The argument is present with a kind the placeholder does not accept.
    WrongKind {
        /// The argument name.
        name: String,
        /// The kind the placeholder accepts.
        expected: Expected,
    },
    /// The locale is not a valid BCP 47 tag.
    Locale(String),
    /// The compiled locale data could not be loaded.
    Data(String),
    /// A number the decimal formatter cannot represent.
    Number,
    /// A timestamp the calendar or the time zone cannot represent.
    Timestamp,
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingArg(name) => write!(f, "missing argument `{name}`"),
            Self::WrongKind { name, expected } => {
                write!(f, "argument `{name}` is not a {expected:?}")
            }
            Self::Locale(tag) => write!(f, "invalid locale `{tag}`"),
            Self::Data(reason) => write!(f, "locale data unavailable: {reason}"),
            Self::Number => f.write_str("number not representable"),
            Self::Timestamp => f.write_str("timestamp not representable"),
        }
    }
}

impl core::error::Error for FormatError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DateStyle {
    Short,
    Medium,
    Long,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeStyle {
    Short,
    Medium,
}

#[derive(Debug, Clone, PartialEq)]
enum Key {
    Exact(i64),
    Category(PluralCategory),
}

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Text(String),
    Arg(String),
    Pound,
    Number {
        arg: String,
        percent: bool,
    },
    Date {
        arg: String,
        style: DateStyle,
    },
    Time {
        arg: String,
        style: TimeStyle,
    },
    Plural {
        arg: String,
        branches: Vec<(Key, Vec<Node>)>,
        other: Vec<Node>,
    },
    Select {
        arg: String,
        branches: Vec<(String, Vec<Node>)>,
        other: Vec<Node>,
    },
}

/// A parsed message, ready to render any number of times.
#[derive(Debug, Clone, PartialEq)]
pub struct Message(Vec<Node>);

impl Message {
    /// Parse `pattern`.
    ///
    /// # Errors
    ///
    /// [`ParseError`] when the pattern uses anything outside the subset.
    pub fn parse(pattern: &str) -> Result<Self, ParseError> {
        let mut parser = Parser {
            chars: pattern.chars().collect(),
            pos: 0,
        };
        let nodes = parser.nodes(0, false, false)?;
        Ok(Self(nodes))
    }

    /// Render in `locale` (a BCP 47 tag, normally a platform locale) with
    /// dates and times in `time_zone`.
    ///
    /// # Errors
    ///
    /// [`FormatError`] when an argument is missing or of the wrong kind, or
    /// the locale, data or value cannot be formatted.
    pub fn render(
        &self,
        args: &Args,
        locale: &str,
        time_zone: &TimeZone,
    ) -> Result<String, FormatError> {
        let locale: Locale = locale
            .parse()
            .map_err(|_| FormatError::Locale(locale.to_owned()))?;
        let cx = Cx {
            args,
            locale,
            time_zone,
        };
        let mut out = String::new();
        cx.nodes(&self.0, None, &mut out)?;
        Ok(out)
    }
}

/// The non-`other` branches of a plural or select, and its `other` body.
type Branches = (Vec<(String, Vec<Node>)>, Vec<Node>);

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.pos + ahead).copied()
    }

    fn err<T>(&self, kind: ParseErrorKind) -> Result<T, ParseError> {
        Err(ParseError {
            kind,
            offset: self.pos,
        })
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, c: char, kind: ParseErrorKind) -> Result<(), ParseError> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            self.err(kind)
        }
    }

    fn word(&mut self, stop: impl Fn(char) -> bool) -> String {
        let start = self.pos;
        while self.peek().is_some_and(|c| !stop(c)) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    /// Message text and placeholders up to the closing `}` of the enclosing
    /// branch (left unconsumed) or the end of the pattern.
    fn nodes(
        &mut self,
        depth: usize,
        nested: bool,
        in_plural: bool,
    ) -> Result<Vec<Node>, ParseError> {
        if depth > MAX_DEPTH {
            return self.err(ParseErrorKind::NestingTooDeep);
        }
        let mut nodes = Vec::new();
        let mut text = String::new();
        let flush = |text: &mut String, nodes: &mut Vec<Node>| {
            if !text.is_empty() {
                nodes.push(Node::Text(std::mem::take(text)));
            }
        };
        loop {
            match self.peek() {
                None if nested => return self.err(ParseErrorKind::UnbalancedBraces),
                None => break,
                Some('}') if nested => break,
                Some('}') => return self.err(ParseErrorKind::UnbalancedBraces),
                Some('{') => {
                    flush(&mut text, &mut nodes);
                    nodes.push(self.placeholder(depth)?);
                }
                Some('#') if in_plural => {
                    flush(&mut text, &mut nodes);
                    nodes.push(Node::Pound);
                    self.pos += 1;
                }
                Some('\'') => self.quote(in_plural, &mut text),
                Some(c) => {
                    text.push(c);
                    self.pos += 1;
                }
            }
        }
        flush(&mut text, &mut nodes);
        Ok(nodes)
    }

    /// ICU apostrophe quoting: `''` is an apostrophe; an apostrophe before
    /// `{`, `}` (or `#` inside a plural) quotes up to the next single
    /// apostrophe; any other apostrophe is literal.
    fn quote(&mut self, in_plural: bool, text: &mut String) {
        match self.peek_at(1) {
            Some('\'') => {
                text.push('\'');
                self.pos += 2;
            }
            Some('{' | '}') => self.quoted(text),
            Some('#') if in_plural => self.quoted(text),
            _ => {
                text.push('\'');
                self.pos += 1;
            }
        }
    }

    fn quoted(&mut self, text: &mut String) {
        self.pos += 1;
        while let Some(c) = self.peek() {
            self.pos += 1;
            if c != '\'' {
                text.push(c);
            } else if self.peek() == Some('\'') {
                text.push('\'');
                self.pos += 1;
            } else {
                return;
            }
        }
    }

    fn placeholder(&mut self, depth: usize) -> Result<Node, ParseError> {
        self.pos += 1;
        self.skip_ws();
        let arg = self.word(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        if !key_ok(&arg) {
            return self.err(ParseErrorKind::InvalidArgumentName);
        }
        self.skip_ws();
        match self.peek() {
            Some('}') => {
                self.pos += 1;
                return Ok(Node::Arg(arg));
            }
            Some(',') => self.pos += 1,
            Some(_) => return self.err(ParseErrorKind::Malformed),
            None => return self.err(ParseErrorKind::UnbalancedBraces),
        }
        self.skip_ws();
        if self.peek().is_none() {
            return self.err(ParseErrorKind::UnbalancedBraces);
        }
        let kind = self.word(|c| !c.is_ascii_alphabetic());
        self.skip_ws();
        match kind.as_str() {
            "number" => self.number(arg),
            "date" => {
                let style = match self.style()?.as_str() {
                    "short" => DateStyle::Short,
                    "medium" => DateStyle::Medium,
                    "long" => DateStyle::Long,
                    "full" => DateStyle::Full,
                    _ => return self.err(ParseErrorKind::UnsupportedStyle),
                };
                Ok(Node::Date { arg, style })
            }
            "time" => {
                let style = match self.style()?.as_str() {
                    "short" => TimeStyle::Short,
                    "medium" => TimeStyle::Medium,
                    _ => return self.err(ParseErrorKind::UnsupportedStyle),
                };
                Ok(Node::Time { arg, style })
            }
            "plural" => self
                .branches(depth, true)
                .map(|(branches, other)| Node::Plural {
                    arg,
                    branches: branches
                        .into_iter()
                        .map(|(selector, body)| (plural_key(&selector), body))
                        .collect(),
                    other,
                }),
            "select" => self
                .branches(depth, false)
                .map(|(branches, other)| Node::Select {
                    arg,
                    branches,
                    other,
                }),
            _ => self.err(ParseErrorKind::UnsupportedType),
        }
    }

    fn number(&mut self, arg: String) -> Result<Node, ParseError> {
        let percent = match self.peek() {
            Some('}') => false,
            Some(',') => {
                self.pos += 1;
                self.skip_ws();
                if self.word(|c| c.is_whitespace() || c == '}' || c == ',') != "percent" {
                    return self.err(ParseErrorKind::UnsupportedStyle);
                }
                true
            }
            Some(_) => return self.err(ParseErrorKind::Malformed),
            None => return self.err(ParseErrorKind::UnbalancedBraces),
        };
        self.skip_ws();
        self.eat('}', ParseErrorKind::Malformed)?;
        Ok(Node::Number { arg, percent })
    }

    /// The mandatory `, style }` tail of a date or time placeholder.
    fn style(&mut self) -> Result<String, ParseError> {
        match self.peek() {
            Some(',') => self.pos += 1,
            Some('}') => return self.err(ParseErrorKind::UnsupportedStyle),
            Some(_) => return self.err(ParseErrorKind::Malformed),
            None => return self.err(ParseErrorKind::UnbalancedBraces),
        }
        self.skip_ws();
        let style = self.word(|c| c.is_whitespace() || c == '}' || c == ',');
        self.skip_ws();
        self.eat('}', ParseErrorKind::UnsupportedStyle)?;
        Ok(style)
    }

    /// The `, selector {body} ... }` tail of a plural or select.
    fn branches(&mut self, depth: usize, plural: bool) -> Result<Branches, ParseError> {
        self.eat(',', ParseErrorKind::Malformed)?;
        let mut branches: Vec<(String, Vec<Node>)> = Vec::new();
        let mut other = None;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('}') => {
                    self.pos += 1;
                    break;
                }
                None => return self.err(ParseErrorKind::UnbalancedBraces),
                Some(_) => {}
            }
            let selector = self.word(|c| c.is_whitespace() || c == '{' || c == '}');
            if selector.is_empty() {
                return self.err(ParseErrorKind::Malformed);
            }
            if plural {
                if selector == "offset" || selector.starts_with("offset:") {
                    return self.err(ParseErrorKind::PluralOffset);
                }
                if !valid_plural_selector(&selector) {
                    return self.err(ParseErrorKind::InvalidSelector);
                }
            }
            self.skip_ws();
            self.eat('{', ParseErrorKind::Malformed)?;
            let body = self.nodes(depth + 1, true, plural)?;
            self.eat('}', ParseErrorKind::UnbalancedBraces)?;
            let duplicate = branches.iter().any(|(s, _)| *s == selector)
                || (selector == "other" && other.is_some());
            if duplicate {
                return self.err(ParseErrorKind::DuplicateSelector);
            }
            if selector == "other" {
                other = Some(body);
            } else {
                branches.push((selector, body));
            }
        }
        match other {
            Some(other) => Ok((branches, other)),
            None => self.err(ParseErrorKind::MissingOther),
        }
    }
}

fn valid_plural_selector(selector: &str) -> bool {
    match selector.strip_prefix('=') {
        Some(digits) => {
            !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit())
                && digits.parse::<i64>().is_ok_and(|n| n <= MAX_EXACT)
        }
        None => matches!(selector, "zero" | "one" | "two" | "few" | "many" | "other"),
    }
}

/// Only called with selectors [`valid_plural_selector`] accepted.
fn plural_key(selector: &str) -> Key {
    if let Some(digits) = selector.strip_prefix('=') {
        return Key::Exact(digits.parse().unwrap_or(0));
    }
    Key::Category(match selector {
        "zero" => PluralCategory::Zero,
        "one" => PluralCategory::One,
        "two" => PluralCategory::Two,
        "few" => PluralCategory::Few,
        "many" => PluralCategory::Many,
        _ => PluralCategory::Other,
    })
}

struct Cx<'a> {
    args: &'a Args,
    locale: Locale,
    time_zone: &'a TimeZone,
}

impl<'a> Cx<'a> {
    fn arg(&self, name: &str) -> Result<&'a ArgValue, FormatError> {
        self.args
            .get(name)
            .ok_or_else(|| FormatError::MissingArg(name.to_owned()))
    }

    fn wrong(name: &str, expected: Expected) -> FormatError {
        FormatError::WrongKind {
            name: name.to_owned(),
            expected,
        }
    }

    /// The argument `name` as a number, with its decimal.
    fn numeric(&self, name: &str) -> Result<(&'a ArgValue, Decimal), FormatError> {
        let value = self.arg(name)?;
        match decimal_of(value) {
            Ok(decimal) => Ok((value, decimal)),
            Err(FormatError::WrongKind { .. }) => Err(Self::wrong(name, Expected::Number)),
            Err(other) => Err(other),
        }
    }

    /// `pound` is the number a `#` renders: the enclosing plural's argument.
    fn nodes(
        &self,
        nodes: &[Node],
        pound: Option<&ArgValue>,
        out: &mut String,
    ) -> Result<(), FormatError> {
        nodes
            .iter()
            .try_for_each(|node| self.node(node, pound, out))
    }

    fn node(
        &self,
        node: &Node,
        pound: Option<&ArgValue>,
        out: &mut String,
    ) -> Result<(), FormatError> {
        match node {
            Node::Text(text) => out.push_str(text),
            Node::Arg(name) => match self.arg(name)? {
                ArgValue::String(s) => out.push_str(s),
                _ => return Err(Self::wrong(name, Expected::String)),
            },
            Node::Pound => match pound {
                Some(value) => out.push_str(&self.number(&decimal_of(value)?, false)?),
                None => out.push('#'),
            },
            Node::Number { arg, percent } => {
                let (_, decimal) = self.numeric(arg)?;
                out.push_str(&self.number(&decimal, *percent)?);
            }
            Node::Plural {
                arg,
                branches,
                other,
            } => {
                let (value, decimal) = self.numeric(arg)?;
                let chosen = self.plural_branch(&decimal, branches, other)?;
                self.nodes(chosen, Some(value), out)?;
            }
            Node::Select {
                arg,
                branches,
                other,
            } => {
                let ArgValue::String(value) = self.arg(arg)? else {
                    return Err(Self::wrong(arg, Expected::String));
                };
                let chosen = branches
                    .iter()
                    .find(|(word, _)| word == value)
                    .map_or(other.as_slice(), |(_, body)| body.as_slice());
                self.nodes(chosen, pound, out)?;
            }
            Node::Date { arg, style } => {
                let civil = self.civil(arg)?;
                out.push_str(&self.date(&civil, *style)?);
            }
            Node::Time { arg, style } => {
                let civil = self.civil(arg)?;
                out.push_str(&self.time(&civil, *style)?);
            }
        }
        Ok(())
    }

    fn plural_branch<'n>(
        &self,
        decimal: &Decimal,
        branches: &'n [(Key, Vec<Node>)],
        other: &'n [Node],
    ) -> Result<&'n [Node], FormatError> {
        let exact = branches
            .iter()
            .find(|(key, _)| matches!(key, Key::Exact(n) if Decimal::from(*n) == *decimal));
        if let Some((_, body)) = exact {
            return Ok(body);
        }
        let rules = PluralRules::try_new_cardinal((&self.locale).into())
            .map_err(|e| FormatError::Data(e.to_string()))?;
        let category = rules.category_for(decimal);
        Ok(branches
            .iter()
            .find(|(key, _)| *key == Key::Category(category))
            .map_or(other, |(_, body)| body.as_slice()))
    }

    /// Grouping on, at most 3 fraction digits, half away from zero; percent
    /// scales by 100 and keeps no fraction digits.
    fn number(&self, decimal: &Decimal, percent: bool) -> Result<String, FormatError> {
        let formatter =
            DecimalFormatter::try_new((&self.locale).into(), DecimalFormatterOptions::default())
                .map_err(|e| FormatError::Data(e.to_string()))?;
        let mut value = decimal.clone();
        if percent {
            value.multiply_pow10(2);
            value.round_with_mode(0, HALF_EXPAND);
        } else {
            value.round_with_mode(-3, HALF_EXPAND);
        }
        value.trim_end();
        value.trim_start();
        let body = formatter.format_to_string(&value);
        if !percent {
            return Ok(body);
        }
        let (prefix, suffix) = percent_affixes(&self.locale);
        Ok(format!("{prefix}{body}{suffix}"))
    }

    fn civil(&self, name: &str) -> Result<DateTime<icu_calendar::Iso>, FormatError> {
        let ArgValue::Timestamp { seconds, nanos } = self.arg(name)? else {
            return Err(Self::wrong(name, Expected::Timestamp));
        };
        // The argument range ends at 9999-12-31T23:59:59Z, a day past the last
        // instant jiff represents. The zone lookup uses the clamped instant and
        // the remainder is added to the civil time, so the whole range renders
        // and only a local time that leaves year 9999 is not representable.
        let clamped = (*seconds).min(jiff::Timestamp::MAX.as_second());
        let timestamp =
            jiff::Timestamp::new(clamped, *nanos).map_err(|_| FormatError::Timestamp)?;
        let local = self
            .time_zone
            .to_datetime(timestamp)
            .checked_add(jiff::SignedDuration::from_secs(*seconds - clamped))
            .map_err(|_| FormatError::Timestamp)?;
        let bad = |_| FormatError::Timestamp;
        let date = Date::try_new_iso(
            i32::from(local.year()),
            u8::try_from(local.month()).map_err(bad)?,
            u8::try_from(local.day()).map_err(bad)?,
        )
        .map_err(|_| FormatError::Timestamp)?;
        let time = Time::try_new(
            u8::try_from(local.hour()).map_err(bad)?,
            u8::try_from(local.minute()).map_err(bad)?,
            u8::try_from(local.second()).map_err(bad)?,
            u32::try_from(local.subsec_nanosecond()).map_err(bad)?,
        )
        .map_err(|_| FormatError::Timestamp)?;
        Ok(DateTime { date, time })
    }

    fn date(
        &self,
        civil: &DateTime<icu_calendar::Iso>,
        style: DateStyle,
    ) -> Result<String, FormatError> {
        let data = |e: icu_datetime::DateTimeFormatterLoadError| FormatError::Data(e.to_string());
        let prefs = (&self.locale).into();
        Ok(match style {
            DateStyle::Short => DateTimeFormatter::try_new(prefs, YMD::short())
                .map_err(data)?
                .format(&civil.date)
                .to_string(),
            DateStyle::Medium => DateTimeFormatter::try_new(prefs, YMD::medium())
                .map_err(data)?
                .format(&civil.date)
                .to_string(),
            DateStyle::Long => DateTimeFormatter::try_new(prefs, YMD::long())
                .map_err(data)?
                .format(&civil.date)
                .to_string(),
            DateStyle::Full => DateTimeFormatter::try_new(prefs, YMDE::long())
                .map_err(data)?
                .format(&civil.date)
                .to_string(),
        })
    }

    fn time(
        &self,
        civil: &DateTime<icu_calendar::Iso>,
        style: TimeStyle,
    ) -> Result<String, FormatError> {
        let (set, precision) = match style {
            TimeStyle::Short => (fieldsets::T::short(), TimePrecision::Minute),
            TimeStyle::Medium => (fieldsets::T::medium(), TimePrecision::Second),
        };
        let formatter: DateTimeFormatter<T> =
            DateTimeFormatter::try_new((&self.locale).into(), set.with_time_precision(precision))
                .map_err(|e| FormatError::Data(e.to_string()))?;
        Ok(formatter.format(&civil.time).to_string())
    }
}

fn decimal_of(value: &ArgValue) -> Result<Decimal, FormatError> {
    match value {
        ArgValue::Int64(v) => Ok(Decimal::from(*v)),
        ArgValue::Double(d) => {
            let mut decimal = Decimal::try_from_f64(*d, FloatPrecision::RoundTrip)
                .map_err(|_| FormatError::Number)?;
            decimal.trim_end();
            Ok(decimal)
        }
        _ => Err(FormatError::WrongKind {
            name: String::new(),
            expected: Expected::Number,
        }),
    }
}

/// The CLDR percent pattern's literal text around the number.
fn percent_affixes(locale: &Locale) -> (&'static str, &'static str) {
    match locale.id.language.as_str() {
        "tr" => ("%", ""),
        "ar" => ("", "\u{200e}%\u{200e}"),
        "fr" | "de" | "es" | "sv" | "da" | "fi" | "nb" | "cs" | "sk" | "sl" | "hr" | "ro"
        | "lt" => ("", "\u{a0}%"),
        _ => ("", "%"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc() -> TimeZone {
        TimeZone::get("UTC").unwrap()
    }

    fn args(entries: &[(&str, ArgValue)]) -> Args {
        Args::from_entries(entries.iter().map(|(k, v)| ((*k).to_owned(), v.clone()))).unwrap()
    }

    fn s(text: &str) -> ArgValue {
        ArgValue::String(text.to_owned())
    }

    /// Every Unicode `White_Space` character becomes U+0020, as the contract compares.
    fn norm(text: &str) -> String {
        text.chars()
            .map(|c| if c.is_whitespace() { ' ' } else { c })
            .collect()
    }

    fn render_in(pattern: &str, locale: &str, entries: &[(&str, ArgValue)]) -> String {
        render_tz(pattern, locale, "UTC", entries)
    }

    fn render_tz(pattern: &str, locale: &str, zone: &str, entries: &[(&str, ArgValue)]) -> String {
        let message = Message::parse(pattern).unwrap();
        let zone = TimeZone::get(zone).unwrap();
        norm(&message.render(&args(entries), locale, &zone).unwrap())
    }

    fn at(iso: &str) -> ArgValue {
        let ts: jiff::Timestamp = iso.parse().unwrap();
        ArgValue::Timestamp {
            seconds: ts.as_second(),
            nanos: ts.subsec_nanosecond(),
        }
    }

    fn parse_kind(pattern: &str) -> ParseErrorKind {
        Message::parse(pattern).unwrap_err().kind
    }

    fn render_err(pattern: &str, entries: &[(&str, ArgValue)]) -> FormatError {
        Message::parse(pattern)
            .unwrap()
            .render(&args(entries), "en", &utc())
            .unwrap_err()
    }

    #[test]
    fn text_and_simple_arguments() {
        assert_eq!(render_in("plain", "en", &[]), "plain");
        assert_eq!(render_in("", "en", &[]), "");
        assert_eq!(
            render_in("Hello {name}!", "fr", &[("name", s("Zoé"))]),
            "Hello Zoé!"
        );
        assert_eq!(render_in("{ name }", "en", &[("name", s("x"))]), "x");
        assert_eq!(
            render_in("<b>{a}</b>", "en", &[("a", s("x"))]),
            "<b>x</b>",
            "tags are text"
        );
        assert_eq!(
            render_in("{a}", "en", &[("a", s("{a}, {b, number}"))]),
            "{a}, {b, number}"
        );
        assert_eq!(render_in("{a}", "en", &[("a", s("#"))]), "#");
    }

    #[test]
    fn apostrophe_quoting() {
        let name = [("n", s("x"))];
        assert_eq!(render_in("It''s {n}", "en", &name), "It's x");
        assert_eq!(render_in("don't {n}", "en", &name), "don't x");
        assert_eq!(render_in("'{n}'", "en", &name), "{n}");
        assert_eq!(render_in("a '{' b '}' c", "en", &name), "a { b } c");
        assert_eq!(render_in("'{n''s}'", "en", &name), "{n's}");
        assert_eq!(render_in("'{n} open", "en", &name), "{n} open");
        assert_eq!(render_in("#", "en", &[]), "#", "# outside a plural is text");
        let count = [("n", ArgValue::Int64(2))];
        assert_eq!(
            render_in("{n, plural, other {'#' is # }}", "en", &count),
            "# is 2 "
        );
    }

    #[test]
    fn numbers_in_english_and_french() {
        let cases: [(ArgValue, &str, &str); 7] = [
            (ArgValue::Int64(1_234_567), "1,234,567", "1 234 567"),
            (ArgValue::Int64(-42), "-42", "-42"),
            (ArgValue::Double(1234.5), "1,234.5", "1 234,5"),
            (ArgValue::Double(1_234.567_8), "1,234.568", "1 234,568"),
            (ArgValue::Double(2.0), "2", "2"),
            (ArgValue::Double(2.000_5), "2.001", "2,001"),
            (ArgValue::Double(-2.000_5), "-2.001", "-2,001"),
        ];
        for (value, en, fr) in cases {
            let entries = [("n", value)];
            assert_eq!(render_in("{n, number}", "en", &entries), en);
            assert_eq!(render_in("{n,number}", "fr", &entries), fr);
        }
        assert_eq!(
            render_in("{n, number}", "ar", &[("n", ArgValue::Int64(1234))]),
            "1,234"
        );
    }

    #[test]
    fn percent_scales_and_rounds_half_away_from_zero() {
        let half = [("n", ArgValue::Double(0.125))];
        assert_eq!(render_in("{n, number, percent}", "en", &half), "13%");
        assert_eq!(render_in("{n, number, percent}", "fr", &half), "13 %");
        assert_eq!(render_in("{n, number, percent}", "tr", &half), "%13");
        assert_eq!(
            render_in("{n, number, percent}", "ar", &half),
            "13\u{200e}%\u{200e}"
        );
        let big = [("n", ArgValue::Int64(12))];
        assert_eq!(render_in("{n, number, percent}", "en", &big), "1,200%");
        let neg = [("n", ArgValue::Double(-0.125))];
        assert_eq!(render_in("{n, number , percent }", "en", &neg), "-13%");
    }

    #[test]
    fn percent_keeps_no_leading_zero() {
        let quarter = [("n", ArgValue::Double(0.25))];
        assert_eq!(render_in("{n, number, percent}", "en", &quarter), "25%");
        let ratio = [("n", ArgValue::Double(0.05))];
        assert_eq!(render_in("{n, number, percent}", "en", &ratio), "5%");
    }

    #[test]
    fn the_other_selector_parses_in_a_plural() {
        let one = [("n", ArgValue::Int64(1))];
        let many = [("n", ArgValue::Int64(5))];
        let pattern = "{n, plural, one {# seat} other {# seats}}";
        assert_eq!(render_in(pattern, "en", &one), "1 seat");
        assert_eq!(render_in(pattern, "en", &many), "5 seats");
    }

    #[test]
    fn extreme_doubles_render_or_fail_typed() {
        for d in [f64::MAX, f64::MIN, f64::MIN_POSITIVE, 5e-324] {
            let result = Message::parse("{n, number}").unwrap().render(
                &args(&[("n", ArgValue::Double(d))]),
                "en",
                &utc(),
            );
            assert!(matches!(result, Ok(_) | Err(FormatError::Number)), "{d}");
        }
        let tiny = render_in("{n, number}", "en", &[("n", ArgValue::Double(5e-324))]);
        assert_eq!(tiny, "0");
    }

    #[test]
    fn plural_in_english() {
        let p = "{n, plural, =0 {none} =7 {lucky seven} one {# item} other {# items}}";
        let cases: [(ArgValue, &str); 7] = [
            (ArgValue::Int64(0), "none"),
            (ArgValue::Int64(1), "1 item"),
            (ArgValue::Int64(2), "2 items"),
            (ArgValue::Int64(7), "lucky seven"),
            (ArgValue::Int64(1234), "1,234 items"),
            (ArgValue::Double(1.5), "1.5 items"),
            (ArgValue::Double(7.0), "lucky seven"),
        ];
        for (value, want) in cases {
            assert_eq!(
                render_in(p, "en", &[("n", value.clone())]),
                want,
                "{value:?}"
            );
        }
        let nested = "{n, plural, one {{g, select, f {She has # file} other {They have # file}}} other {{g, select, f {She has # files} other {They have # files}}}}";
        let entries = [("n", ArgValue::Int64(1)), ("g", s("f"))];
        // `#` is the number only directly inside a plural branch; a nested select
        // keeps it literal, as intl-messageformat does.
        assert_eq!(render_in(nested, "en", &entries), "She has # file");
        let entries = [("n", ArgValue::Int64(3)), ("g", s("x"))];
        assert_eq!(render_in(nested, "en", &entries), "They have # files");
    }

    #[test]
    fn plural_categories_follow_the_locale() {
        let p = "{n, plural, zero {z} one {one} two {two} few {few} many {many} other {other}}";
        let french = |value| render_in(p, "fr", &[("n", value)]);
        assert_eq!(french(ArgValue::Int64(0)), "one");
        assert_eq!(french(ArgValue::Int64(1)), "one");
        assert_eq!(french(ArgValue::Double(1.5)), "one");
        assert_eq!(french(ArgValue::Int64(2)), "other");
        assert_eq!(french(ArgValue::Int64(1_000_000)), "many");
        let arabic = |value| render_in(p, "ar", &[("n", value)]);
        assert_eq!(arabic(ArgValue::Int64(0)), "z");
        assert_eq!(arabic(ArgValue::Int64(2)), "two");
        assert_eq!(arabic(ArgValue::Int64(5)), "few");
        assert_eq!(arabic(ArgValue::Int64(11)), "many");
        assert_eq!(arabic(ArgValue::Int64(100)), "other");
        let english = |value| {
            render_in(
                "{n, plural, one {one} other {other}}",
                "en",
                &[("n", value)],
            )
        };
        assert_eq!(english(ArgValue::Int64(1)), "one");
        assert_eq!(english(ArgValue::Int64(0)), "other");
    }

    #[test]
    fn a_missing_category_falls_to_other() {
        let p = "{n, plural, few {few} other {other}}";
        assert_eq!(render_in(p, "en", &[("n", ArgValue::Int64(1))]), "other");
    }

    #[test]
    fn select_matches_bytes_exactly() {
        let p = "{g, select, female {She} male {He} other {They}}";
        let pick = |value: &str| render_in(p, "en", &[("g", s(value))]);
        assert_eq!(pick("female"), "She");
        assert_eq!(pick("male"), "He");
        assert_eq!(pick("Female"), "They");
        assert_eq!(pick(" male"), "They");
        assert_eq!(pick(""), "They");
        assert_eq!(pick("other"), "They");
        assert_eq!(pick("é"), "They");
        let accented = render_in("{g, select, é {yes} other {no}}", "en", &[("g", s("é"))]);
        assert_eq!(accented, "yes");
    }

    #[test]
    fn dates_in_english_and_french() {
        let when = [("at", at("2026-11-20T13:05:09Z"))];
        let styles = [
            ("short", "11/20/26", "20/11/2026"),
            ("medium", "Nov 20, 2026", "20 nov. 2026"),
            ("long", "November 20, 2026", "20 novembre 2026"),
            (
                "full",
                "Friday, November 20, 2026",
                "vendredi 20 novembre 2026",
            ),
        ];
        for (style, en, fr) in styles {
            let pattern = format!("{{at, date, {style}}}");
            assert_eq!(render_in(&pattern, "en", &when), en, "{style}");
            assert_eq!(render_in(&pattern, "fr", &when), fr, "{style}");
        }
    }

    #[test]
    fn times_in_english_and_french() {
        let when = [("at", at("2026-11-20T13:05:09Z"))];
        assert_eq!(render_in("{at, time, short}", "en", &when), "1:05 PM");
        assert_eq!(render_in("{at, time, medium}", "en", &when), "1:05:09 PM");
        assert_eq!(render_in("{at, time, short}", "fr", &when), "13:05");
        assert_eq!(render_in("{at, time, medium}", "fr", &when), "13:05:09");
    }

    #[test]
    fn the_time_zone_moves_the_date_and_the_time() {
        let when = [("at", at("2026-11-20T23:30:00Z"))];
        let both = "{at, date, medium} {at, time, short}";
        assert_eq!(render_tz(both, "en", "UTC", &when), "Nov 20, 2026 11:30 PM");
        assert_eq!(
            render_tz(both, "en", "America/New_York", &when),
            "Nov 20, 2026 6:30 PM"
        );
        assert_eq!(
            render_tz(both, "en", "Asia/Tokyo", &when),
            "Nov 21, 2026 8:30 AM"
        );
        assert_eq!(
            render_tz(both, "fr", "Pacific/Kiritimati", &when),
            "21 nov. 2026 13:30"
        );
    }

    #[test]
    fn epoch_and_range_edges_render() {
        let epoch = [(
            "at",
            ArgValue::Timestamp {
                seconds: 0,
                nanos: 0,
            },
        )];
        assert_eq!(render_in("{at, date, medium}", "en", &epoch), "Jan 1, 1970");
        let first = [(
            "at",
            ArgValue::Timestamp {
                seconds: -62_135_596_800,
                nanos: 0,
            },
        )];
        assert!(
            Message::parse("{at, date, short}")
                .unwrap()
                .render(&args(&first), "en", &utc())
                .is_ok()
        );
        let last = [(
            "at",
            ArgValue::Timestamp {
                seconds: 253_402_300_799,
                nanos: 999_999_999,
            },
        )];
        assert_eq!(
            render_in("{at, date, short}{at, time, medium}", "en", &last),
            "12/31/9999".to_owned() + "11:59:59 PM"
        );
        let east = TimeZone::get("Pacific/Kiritimati").unwrap();
        let message = Message::parse("{at, date, short}").unwrap();
        assert_eq!(
            message.render(&args(&last), "en", &east),
            Err(FormatError::Timestamp)
        );
    }

    #[test]
    fn formatting_errors_are_typed() {
        let missing = FormatError::MissingArg("n".to_owned());
        assert_eq!(render_err("{n}", &[]), missing);
        assert_eq!(render_err("{n, number}", &[]), missing);
        assert_eq!(render_err("{n, plural, other {x}}", &[]), missing);
        assert_eq!(render_err("{n, select, other {x}}", &[]), missing);
        assert_eq!(render_err("{n, date, short}", &[]), missing);
        let wrong = |expected| FormatError::WrongKind {
            name: "n".to_owned(),
            expected,
        };
        let int = [("n", ArgValue::Int64(1))];
        let text = [("n", s("1"))];
        let stamp = [(
            "n",
            ArgValue::Timestamp {
                seconds: 0,
                nanos: 0,
            },
        )];
        assert_eq!(render_err("{n}", &int), wrong(Expected::String));
        assert_eq!(render_err("{n}", &stamp), wrong(Expected::String));
        assert_eq!(render_err("{n, number}", &text), wrong(Expected::Number));
        assert_eq!(
            render_err("{n, number, percent}", &stamp),
            wrong(Expected::Number)
        );
        assert_eq!(
            render_err("{n, plural, other {x}}", &text),
            wrong(Expected::Number)
        );
        assert_eq!(
            render_err("{n, select, other {x}}", &int),
            wrong(Expected::String)
        );
        assert_eq!(
            render_err("{n, date, short}", &int),
            wrong(Expected::Timestamp)
        );
        assert_eq!(
            render_err("{n, time, short}", &text),
            wrong(Expected::Timestamp)
        );
        let past_the_calendar = [(
            "n",
            ArgValue::Timestamp {
                seconds: 253_402_300_799,
                nanos: 0,
            },
        )];
        let east = TimeZone::get("Pacific/Kiritimati").unwrap();
        let message = Message::parse("{n, date, short}").unwrap();
        assert_eq!(
            message.render(&args(&past_the_calendar), "en", &east),
            Err(FormatError::Timestamp)
        );
        let message = Message::parse("x").unwrap();
        let err = message
            .render(&Args::new(), "not a locale", &utc())
            .unwrap_err();
        assert_eq!(err, FormatError::Locale("not a locale".to_owned()));
        assert_eq!(err.to_string(), "invalid locale `not a locale`");
    }

    #[test]
    fn a_failing_branch_fails_the_whole_message() {
        let message = Message::parse("start {n, plural, other {{missing} end}}").unwrap();
        let err = message
            .render(&args(&[("n", ArgValue::Int64(1))]), "en", &utc())
            .unwrap_err();
        assert_eq!(err, FormatError::MissingArg("missing".to_owned()));
    }

    #[test]
    fn parses_only_the_subset() {
        let cases = [
            (
                "{n, selectordinal, one {#st} other {#th}}",
                ParseErrorKind::UnsupportedType,
            ),
            ("{n, spellout}", ParseErrorKind::UnsupportedType),
            ("{n, duration}", ParseErrorKind::UnsupportedType),
            ("{n, bogus}", ParseErrorKind::UnsupportedType),
            (
                "{n, number, ::currency/EUR}",
                ParseErrorKind::UnsupportedStyle,
            ),
            ("{n, number, integer}", ParseErrorKind::UnsupportedStyle),
            ("{n, date}", ParseErrorKind::UnsupportedStyle),
            ("{n, date, ::yMMMd}", ParseErrorKind::UnsupportedStyle),
            ("{n, date, tiny}", ParseErrorKind::UnsupportedStyle),
            ("{n, time}", ParseErrorKind::UnsupportedStyle),
            ("{n, time, long}", ParseErrorKind::UnsupportedStyle),
            ("{n, time, full}", ParseErrorKind::UnsupportedStyle),
            (
                "{n, plural, offset:1 one {a} other {b}}",
                ParseErrorKind::PluralOffset,
            ),
            ("{n, plural, one {a}}", ParseErrorKind::MissingOther),
            ("{n, select, a {a}}", ParseErrorKind::MissingOther),
            (
                "{n, plural, other {a} other {b}}",
                ParseErrorKind::DuplicateSelector,
            ),
            (
                "{n, plural, one {a} one {b} other {c}}",
                ParseErrorKind::DuplicateSelector,
            ),
            (
                "{n, select, a {a} a {b} other {c}}",
                ParseErrorKind::DuplicateSelector,
            ),
            (
                "{n, plural, =1 {a} =1 {b} other {c}}",
                ParseErrorKind::DuplicateSelector,
            ),
            (
                "{n, plural, several {a} other {b}}",
                ParseErrorKind::InvalidSelector,
            ),
            (
                "{n, plural, = {a} other {b}}",
                ParseErrorKind::InvalidSelector,
            ),
            (
                "{n, plural, =x {a} other {b}}",
                ParseErrorKind::InvalidSelector,
            ),
            (
                "{n, plural, =9007199254740992 {a} other {b}}",
                ParseErrorKind::InvalidSelector,
            ),
            (
                "{n, plural, =-1 {a} other {b}}",
                ParseErrorKind::InvalidSelector,
            ),
            ("{N}", ParseErrorKind::InvalidArgumentName),
            ("{0}", ParseErrorKind::InvalidArgumentName),
            ("{}", ParseErrorKind::InvalidArgumentName),
            ("{my-arg}", ParseErrorKind::Malformed),
            ("{n", ParseErrorKind::UnbalancedBraces),
            ("{n,", ParseErrorKind::UnbalancedBraces),
            ("{n, number", ParseErrorKind::UnbalancedBraces),
            ("{n, plural, one {a", ParseErrorKind::UnbalancedBraces),
            ("{n, plural, one", ParseErrorKind::Malformed),
            ("}", ParseErrorKind::UnbalancedBraces),
            ("a } b", ParseErrorKind::UnbalancedBraces),
            (
                "{n, select, a {x} other {y}",
                ParseErrorKind::UnbalancedBraces,
            ),
            ("{n x}", ParseErrorKind::Malformed),
            ("{n, plural one {a} other {b}}", ParseErrorKind::Malformed),
            ("{n, plural, one a other {b}}", ParseErrorKind::Malformed),
        ];
        for (pattern, kind) in cases {
            assert_eq!(parse_kind(pattern), kind, "{pattern}");
        }
        assert!(Message::parse("{n, plural, =9007199254740991 {a} other {b}}").is_ok());
    }

    #[test]
    fn nesting_is_bounded() {
        let nest = |levels: usize| {
            let mut p = "x".to_owned();
            for _ in 0..levels {
                p = format!("{{a, select, k {{{p}}} other {{y}}}}");
            }
            p
        };
        assert!(Message::parse(&nest(8)).is_ok());
        assert_eq!(parse_kind(&nest(100_000)), ParseErrorKind::NestingTooDeep);
    }

    #[test]
    fn parse_errors_report_the_offset() {
        let err = Message::parse("ab {n, bogus}").unwrap_err();
        assert_eq!(err.kind, ParseErrorKind::UnsupportedType);
        assert!(err.offset >= 3 && err.offset <= 13, "{}", err.offset);
        assert!(err.to_string().contains("UnsupportedType"));
    }

    #[test]
    fn a_parsed_message_renders_repeatedly() {
        let message = Message::parse("{n, plural, one {# row} other {# rows}}").unwrap();
        for (n, want) in [(1, "1 row"), (2, "2 rows"), (1, "1 row")] {
            let out = message
                .render(&args(&[("n", ArgValue::Int64(n))]), "en", &utc())
                .unwrap();
            assert_eq!(out, want);
        }
    }
}
