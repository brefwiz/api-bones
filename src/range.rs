//! `Range` request and `Content-Range` response header types (RFC 7233).
//!
//! [`ByteRange`] models a single byte range specifier.
//! [`RangeHeader`] models the `Range` request header, which may contain one or
//! more byte ranges or a suffix range.
//! [`ContentRange`] models the `Content-Range` response header, indicating
//! which portion of a resource's representation is being returned.
//!
//! # Example
//!
//! ```rust
//! use api_bones::range::{ByteRange, ContentRange, RangeHeader};
//!
//! // Parse a Range request header.
//! let range: RangeHeader = "bytes=0-499".parse().unwrap();
//! assert_eq!(range, RangeHeader::Bytes(vec![ByteRange::FromTo(0, 499)]));
//!
//! // Build a Content-Range response header.
//! let cr = ContentRange::bytes(0, 499, Some(1234));
//! assert_eq!(cr.to_string(), "bytes 0-499/1234");
//!
//! // Validate the range against the resource length.
//! assert!(ByteRange::FromTo(0, 499).is_valid(1234));
//! assert!(!ByteRange::FromTo(1234, 9999).is_valid(1234)); // first >= length
//! ```

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::{fmt, str::FromStr};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ByteRange
// ---------------------------------------------------------------------------

/// A single byte range specifier as used in the `Range` header (RFC 7233 §2.1).
///
/// - `FromTo(first, last)` — a closed byte range `first-last` (both inclusive).
/// - `From(first)` — an open-ended range starting at `first`.
/// - `Suffix(n)` — the last `n` bytes of the representation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ByteRange {
    /// A closed byte range: `<first>-<last>` (both inclusive).
    FromTo(u64, u64),
    /// An open-ended byte range: `<first>-` (from `first` to end).
    From(u64),
    /// A suffix byte range: `-<n>` (last `n` bytes).
    Suffix(u64),
}

impl ByteRange {
    /// Validate this range against the total length of the resource.
    ///
    /// Returns `false` if:
    /// - `FromTo(first, last)`: `first > last` or `first >= length`.
    /// - `From(first)`: `first >= length`.
    /// - `Suffix(n)`: `n == 0`.
    ///
    /// ```
    /// use api_bones::range::ByteRange;
    ///
    /// assert!(ByteRange::FromTo(0, 499).is_valid(1000));
    /// assert!(!ByteRange::FromTo(500, 200).is_valid(1000)); // first > last
    /// assert!(!ByteRange::FromTo(1000, 1999).is_valid(1000)); // first >= length
    /// assert!(ByteRange::Suffix(100).is_valid(1000));
    /// assert!(!ByteRange::Suffix(0).is_valid(1000));
    /// ```
    #[must_use]
    pub fn is_valid(&self, length: u64) -> bool {
        match self {
            Self::FromTo(first, last) => first <= last && *first < length,
            Self::From(first) => *first < length,
            Self::Suffix(n) => *n > 0,
        }
    }

    /// Resolve this range to a `(first, last)` byte range against the given
    /// resource `length`. Returns `None` if the range is unsatisfiable.
    ///
    /// ```
    /// use api_bones::range::ByteRange;
    ///
    /// assert_eq!(ByteRange::FromTo(0, 99).resolve(500), Some((0, 99)));
    /// assert_eq!(ByteRange::From(400).resolve(500), Some((400, 499)));
    /// assert_eq!(ByteRange::Suffix(100).resolve(500), Some((400, 499)));
    /// assert_eq!(ByteRange::Suffix(600).resolve(500), Some((0, 499)));
    /// assert_eq!(ByteRange::FromTo(0, 99).resolve(0), None);
    /// ```
    #[must_use]
    pub fn resolve(&self, length: u64) -> Option<(u64, u64)> {
        if length == 0 {
            return None;
        }
        match self {
            Self::FromTo(first, last) => {
                if first > last || *first >= length {
                    None
                } else {
                    Some((*first, (*last).min(length - 1)))
                }
            }
            Self::From(first) => {
                if *first >= length {
                    None
                } else {
                    Some((*first, length - 1))
                }
            }
            Self::Suffix(n) => {
                if *n == 0 {
                    None
                } else {
                    let first = length.saturating_sub(*n);
                    Some((first, length - 1))
                }
            }
        }
    }
}

impl fmt::Display for ByteRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FromTo(first, last) => write!(f, "{first}-{last}"),
            Self::From(first) => write!(f, "{first}-"),
            Self::Suffix(n) => write!(f, "-{n}"),
        }
    }
}

// ---------------------------------------------------------------------------
// RangeHeader
// ---------------------------------------------------------------------------

/// The `Range` request header (RFC 7233 §3.1).
///
/// The only range unit currently defined by the spec is `bytes`.
/// Other range units are represented by the `Other` variant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum RangeHeader {
    /// `bytes=<range-set>` — one or more byte ranges.
    Bytes(Vec<ByteRange>),
    /// An unrecognised range unit, preserved as-is.
    Other(String),
}

impl fmt::Display for RangeHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes(ranges) => {
                f.write_str("bytes=")?;
                for (i, r) in ranges.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    fmt::Display::fmt(r, f)?;
                }
                Ok(())
            }
            Self::Other(s) => f.write_str(s),
        }
    }
}

// ---------------------------------------------------------------------------
// Parse errors
// ---------------------------------------------------------------------------

/// Error returned when parsing a `Range` or `Content-Range` header fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseRangeError {
    /// The input was empty.
    Empty,
    /// The range-unit or format was not recognised.
    Malformed,
}

impl fmt::Display for ParseRangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("range header is empty"),
            Self::Malformed => f.write_str("range header is malformed"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParseRangeError {}

// ---------------------------------------------------------------------------
// FromStr for ByteRange
// ---------------------------------------------------------------------------

impl FromStr for ByteRange {
    type Err = ParseRangeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ParseRangeError::Empty);
        }
        if let Some(n) = s.strip_prefix('-') {
            // suffix range: -N
            let n: u64 = n.parse().map_err(|_| ParseRangeError::Malformed)?;
            return Ok(Self::Suffix(n));
        }
        if let Some(pos) = s.find('-') {
            let first: u64 = s[..pos].parse().map_err(|_| ParseRangeError::Malformed)?;
            let rest = &s[pos + 1..];
            if rest.trim().is_empty() {
                return Ok(Self::From(first));
            }
            let last: u64 = rest.parse().map_err(|_| ParseRangeError::Malformed)?;
            return Ok(Self::FromTo(first, last));
        }
        Err(ParseRangeError::Malformed)
    }
}

// ---------------------------------------------------------------------------
// FromStr for RangeHeader
// ---------------------------------------------------------------------------

impl FromStr for RangeHeader {
    type Err = ParseRangeError;

    /// Parse a `Range` header value such as `bytes=0-499` or `bytes=0-99, 200-299`.
    ///
    /// ```
    /// use api_bones::range::{ByteRange, RangeHeader};
    ///
    /// let h: RangeHeader = "bytes=0-499".parse().unwrap();
    /// assert_eq!(h, RangeHeader::Bytes(vec![ByteRange::FromTo(0, 499)]));
    ///
    /// let h2: RangeHeader = "bytes=0-99, 200-299, -50".parse().unwrap();
    /// assert_eq!(h2, RangeHeader::Bytes(vec![
    ///     ByteRange::FromTo(0, 99),
    ///     ByteRange::FromTo(200, 299),
    ///     ByteRange::Suffix(50),
    /// ]));
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ParseRangeError::Empty);
        }

        if let Some(rest) = s.strip_prefix("bytes=") {
            let ranges: Result<Vec<ByteRange>, _> = rest
                .split(',')
                .filter(|p| !p.trim().is_empty())
                .map(|p| p.trim().parse::<ByteRange>())
                .collect();
            let ranges = ranges?;
            if ranges.is_empty() {
                return Err(ParseRangeError::Malformed);
            }
            return Ok(Self::Bytes(ranges));
        }

        Ok(Self::Other(s.to_string()))
    }
}

// ---------------------------------------------------------------------------
// ContentRange
// ---------------------------------------------------------------------------

/// The `Content-Range` response header (RFC 7233 §4.2).
///
/// Indicates which portion of a resource's representation is included in the
/// response body.
///
/// ```
/// use api_bones::range::ContentRange;
///
/// let cr = ContentRange::bytes(0, 999, Some(5000));
/// assert_eq!(cr.to_string(), "bytes 0-999/5000");
///
/// let cr_unknown = ContentRange::bytes_unknown_length(200, 299);
/// assert_eq!(cr_unknown.to_string(), "bytes 200-299/*");
///
/// let cr_unsatisfied = ContentRange::unsatisfiable(5000);
/// assert_eq!(cr_unsatisfied.to_string(), "bytes */5000");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ContentRange {
    /// A satisfiable byte range: `bytes <first>-<last>/<complete-length or *>`.
    Bytes {
        /// First byte position (inclusive).
        first: u64,
        /// Last byte position (inclusive).
        last: u64,
        /// Total length of the representation, or `None` if unknown (`*`).
        complete_length: Option<u64>,
    },
    /// An unsatisfiable range: `bytes */<complete-length>`.
    Unsatisfiable {
        /// Total length of the representation.
        complete_length: u64,
    },
}

impl ContentRange {
    /// Construct a satisfiable byte range with a known total length.
    #[must_use]
    pub fn bytes(first: u64, last: u64, complete_length: Option<u64>) -> Self {
        Self::Bytes {
            first,
            last,
            complete_length,
        }
    }

    /// Construct a satisfiable byte range where the total length is unknown.
    #[must_use]
    pub fn bytes_unknown_length(first: u64, last: u64) -> Self {
        Self::Bytes {
            first,
            last,
            complete_length: None,
        }
    }

    /// Construct an unsatisfiable response (`bytes */<complete-length>`).
    ///
    /// Use this when the `Range` header cannot be satisfied; pair it with a
    /// `416 Range Not Satisfiable` status code.
    #[must_use]
    pub fn unsatisfiable(complete_length: u64) -> Self {
        Self::Unsatisfiable { complete_length }
    }
}

impl fmt::Display for ContentRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes {
                first,
                last,
                complete_length,
            } => {
                write!(f, "bytes {first}-{last}/")?;
                match complete_length {
                    Some(len) => write!(f, "{len}"),
                    None => f.write_str("*"),
                }
            }
            Self::Unsatisfiable { complete_length } => {
                write!(f, "bytes */{complete_length}")
            }
        }
    }
}

impl FromStr for ContentRange {
    type Err = ParseRangeError;

    /// Parse a `Content-Range` header value.
    ///
    /// Accepts `bytes <first>-<last>/<length>`, `bytes <first>-<last>/*`,
    /// and `bytes */<length>`.
    ///
    /// ```
    /// use api_bones::range::ContentRange;
    ///
    /// let cr: ContentRange = "bytes 0-999/5000".parse().unwrap();
    /// assert_eq!(cr, ContentRange::bytes(0, 999, Some(5000)));
    ///
    /// let cr: ContentRange = "bytes */5000".parse().unwrap();
    /// assert_eq!(cr, ContentRange::unsatisfiable(5000));
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let rest = s
            .strip_prefix("bytes ")
            .ok_or(ParseRangeError::Malformed)?;

        if let Some(len_str) = rest.strip_prefix("*/") {
            let complete_length: u64 = len_str.parse().map_err(|_| ParseRangeError::Malformed)?;
            return Ok(Self::Unsatisfiable { complete_length });
        }

        let slash = rest.find('/').ok_or(ParseRangeError::Malformed)?;
        let range_part = &rest[..slash];
        let len_part = &rest[slash + 1..];

        let dash = range_part.find('-').ok_or(ParseRangeError::Malformed)?;
        let first: u64 = range_part[..dash]
            .parse()
            .map_err(|_| ParseRangeError::Malformed)?;
        let last: u64 = range_part[dash + 1..]
            .parse()
            .map_err(|_| ParseRangeError::Malformed)?;

        let complete_length = if len_part == "*" {
            None
        } else {
            Some(
                len_part
                    .parse::<u64>()
                    .map_err(|_| ParseRangeError::Malformed)?,
            )
        };

        Ok(Self::Bytes {
            first,
            last,
            complete_length,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ByteRange
    // -----------------------------------------------------------------------

    #[test]
    fn byte_range_from_to_display() {
        assert_eq!(ByteRange::FromTo(0, 499).to_string(), "0-499");
    }

    #[test]
    fn byte_range_from_display() {
        assert_eq!(ByteRange::From(100).to_string(), "100-");
    }

    #[test]
    fn byte_range_suffix_display() {
        assert_eq!(ByteRange::Suffix(50).to_string(), "-50");
    }

    #[test]
    fn byte_range_parse_from_to() {
        let r: ByteRange = "0-499".parse().unwrap();
        assert_eq!(r, ByteRange::FromTo(0, 499));
    }

    #[test]
    fn byte_range_parse_from() {
        let r: ByteRange = "100-".parse().unwrap();
        assert_eq!(r, ByteRange::From(100));
    }

    #[test]
    fn byte_range_parse_suffix() {
        let r: ByteRange = "-50".parse().unwrap();
        assert_eq!(r, ByteRange::Suffix(50));
    }

    #[test]
    fn byte_range_roundtrip() {
        let ranges = [
            ByteRange::FromTo(0, 99),
            ByteRange::From(500),
            ByteRange::Suffix(200),
        ];
        for r in &ranges {
            let s = r.to_string();
            let parsed: ByteRange = s.parse().unwrap();
            assert_eq!(&parsed, r);
        }
    }

    #[test]
    fn byte_range_is_valid() {
        assert!(ByteRange::FromTo(0, 499).is_valid(1000));
        assert!(!ByteRange::FromTo(500, 200).is_valid(1000));
        assert!(!ByteRange::FromTo(1000, 1999).is_valid(1000));
        assert!(ByteRange::From(0).is_valid(1));
        assert!(!ByteRange::From(1000).is_valid(1000));
        assert!(ByteRange::Suffix(1).is_valid(1));
        assert!(!ByteRange::Suffix(0).is_valid(1000));
    }

    #[test]
    fn byte_range_resolve_from_to() {
        assert_eq!(ByteRange::FromTo(0, 99).resolve(500), Some((0, 99)));
        // clamp last to length-1
        assert_eq!(ByteRange::FromTo(0, 999).resolve(500), Some((0, 499)));
        // unsatisfiable
        assert_eq!(ByteRange::FromTo(500, 999).resolve(500), None);
        assert_eq!(ByteRange::FromTo(0, 99).resolve(0), None);
    }

    #[test]
    fn byte_range_resolve_from() {
        assert_eq!(ByteRange::From(400).resolve(500), Some((400, 499)));
        assert_eq!(ByteRange::From(500).resolve(500), None);
    }

    #[test]
    fn byte_range_resolve_suffix() {
        assert_eq!(ByteRange::Suffix(100).resolve(500), Some((400, 499)));
        // larger than length → starts at 0
        assert_eq!(ByteRange::Suffix(600).resolve(500), Some((0, 499)));
        assert_eq!(ByteRange::Suffix(0).resolve(500), None);
    }

    // -----------------------------------------------------------------------
    // RangeHeader
    // -----------------------------------------------------------------------

    #[test]
    fn range_header_parse_single() {
        let h: RangeHeader = "bytes=0-499".parse().unwrap();
        assert_eq!(h, RangeHeader::Bytes(vec![ByteRange::FromTo(0, 499)]));
    }

    #[test]
    fn range_header_parse_multi() {
        let h: RangeHeader = "bytes=0-99, 200-299".parse().unwrap();
        assert_eq!(
            h,
            RangeHeader::Bytes(vec![ByteRange::FromTo(0, 99), ByteRange::FromTo(200, 299)])
        );
    }

    #[test]
    fn range_header_parse_suffix() {
        let h: RangeHeader = "bytes=-500".parse().unwrap();
        assert_eq!(h, RangeHeader::Bytes(vec![ByteRange::Suffix(500)]));
    }

    #[test]
    fn range_header_parse_open() {
        let h: RangeHeader = "bytes=9500-".parse().unwrap();
        assert_eq!(h, RangeHeader::Bytes(vec![ByteRange::From(9500)]));
    }

    #[test]
    fn range_header_roundtrip() {
        let h = RangeHeader::Bytes(vec![ByteRange::FromTo(0, 499), ByteRange::Suffix(50)]);
        let s = h.to_string();
        let parsed: RangeHeader = s.parse().unwrap();
        assert_eq!(parsed, h);
    }

    #[test]
    fn range_header_other() {
        let h: RangeHeader = "items=0-9".parse().unwrap();
        assert_eq!(h, RangeHeader::Other("items=0-9".to_string()));
    }

    #[test]
    fn range_header_empty_errors() {
        assert_eq!("".parse::<RangeHeader>(), Err(ParseRangeError::Empty));
    }

    // -----------------------------------------------------------------------
    // ContentRange
    // -----------------------------------------------------------------------

    #[test]
    fn content_range_bytes_display() {
        let cr = ContentRange::bytes(0, 999, Some(5000));
        assert_eq!(cr.to_string(), "bytes 0-999/5000");
    }

    #[test]
    fn content_range_bytes_unknown_length_display() {
        let cr = ContentRange::bytes_unknown_length(200, 299);
        assert_eq!(cr.to_string(), "bytes 200-299/*");
    }

    #[test]
    fn content_range_unsatisfiable_display() {
        let cr = ContentRange::unsatisfiable(5000);
        assert_eq!(cr.to_string(), "bytes */5000");
    }

    #[test]
    fn content_range_parse_known_length() {
        let cr: ContentRange = "bytes 0-999/5000".parse().unwrap();
        assert_eq!(cr, ContentRange::bytes(0, 999, Some(5000)));
    }

    #[test]
    fn content_range_parse_unknown_length() {
        let cr: ContentRange = "bytes 0-999/*".parse().unwrap();
        assert_eq!(cr, ContentRange::bytes_unknown_length(0, 999));
    }

    #[test]
    fn content_range_parse_unsatisfiable() {
        let cr: ContentRange = "bytes */5000".parse().unwrap();
        assert_eq!(cr, ContentRange::unsatisfiable(5000));
    }

    #[test]
    fn content_range_roundtrip() {
        let cases = [
            ContentRange::bytes(0, 499, Some(1000)),
            ContentRange::bytes_unknown_length(100, 199),
            ContentRange::unsatisfiable(9999),
        ];
        for cr in &cases {
            let s = cr.to_string();
            let parsed: ContentRange = s.parse().unwrap();
            assert_eq!(&parsed, cr);
        }
    }
}
