// SPDX-License-Identifier: Apache-2.0
//! Closed UTC time interval `[from, to]`.
//!
//! Requires the `chrono` feature.
//!
//! # Example
//!
//! ```rust
//! use api_bones::time_window::TimeWindow;
//! use chrono::Utc;
//!
//! let now = Utc::now();
//! let later = now + chrono::Duration::hours(1);
//! let window = TimeWindow::new(now, later).unwrap();
//! assert!(window.contains(now));
//! ```

use crate::common::Timestamp;
use core::fmt;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// TimeWindowError
// ---------------------------------------------------------------------------

/// Error returned when constructing an invalid [`TimeWindow`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TimeWindowError {
    /// The `to` timestamp precedes `from`.
    #[error("window `to` ({to}) must be >= `from` ({from})")]
    Inverted {
        /// The start of the window.
        from: Timestamp,
        /// The (invalid) end of the window.
        to: Timestamp,
    },
}

// ---------------------------------------------------------------------------
// TimeWindow
// ---------------------------------------------------------------------------

/// A closed UTC time interval `[from, to]`.
///
/// Both endpoints are inclusive. Use [`TimeWindow::new`] to construct a
/// validated instance.
///
/// # Example
///
/// ```rust
/// use api_bones::time_window::TimeWindow;
/// use chrono::Utc;
///
/// let now = Utc::now();
/// let later = now + chrono::Duration::hours(1);
/// let window = TimeWindow::new(now, later).unwrap();
/// assert_eq!(window.duration(), chrono::Duration::hours(1));
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TimeWindow {
    /// Start of the window (inclusive).
    #[cfg_attr(feature = "utoipa", schema(value_type = String, format = DateTime))]
    pub from: Timestamp,
    /// End of the window (inclusive).
    #[cfg_attr(feature = "utoipa", schema(value_type = String, format = DateTime))]
    pub to: Timestamp,
}

impl TimeWindow {
    /// Construct a new `TimeWindow`, validating that `from <= to`.
    ///
    /// # Errors
    ///
    /// Returns [`TimeWindowError::Inverted`] if `to < from`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::time_window::TimeWindow;
    /// use chrono::Utc;
    ///
    /// let now = Utc::now();
    /// let later = now + chrono::Duration::seconds(60);
    /// assert!(TimeWindow::new(now, later).is_ok());
    /// assert!(TimeWindow::new(later, now).is_err());
    /// ```
    pub fn new(from: Timestamp, to: Timestamp) -> Result<Self, TimeWindowError> {
        if from > to {
            return Err(TimeWindowError::Inverted { from, to });
        }
        Ok(Self { from, to })
    }

    /// Return the duration of this window (`to - from`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::time_window::TimeWindow;
    /// use chrono::Utc;
    ///
    /// let now = Utc::now();
    /// let later = now + chrono::Duration::hours(1);
    /// let window = TimeWindow::new(now, later).unwrap();
    /// assert_eq!(window.duration(), chrono::Duration::hours(1));
    /// ```
    #[must_use]
    pub fn duration(&self) -> chrono::Duration {
        self.to - self.from
    }

    /// Return `true` if `ts` falls within `[from, to]` (inclusive).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::time_window::TimeWindow;
    /// use chrono::Utc;
    ///
    /// let now = Utc::now();
    /// let later = now + chrono::Duration::hours(1);
    /// let window = TimeWindow::new(now, later).unwrap();
    /// assert!(window.contains(now));
    /// assert!(window.contains(later));
    /// assert!(!window.contains(now - chrono::Duration::seconds(1)));
    /// ```
    #[must_use]
    pub fn contains(&self, ts: Timestamp) -> bool {
        ts >= self.from && ts <= self.to
    }

    /// Return `true` if this window overlaps with `other`.
    ///
    /// Two windows overlap when `self.from <= other.to && self.to >= other.from`.
    /// Touching boundaries (one window's end equals the other's start) count as
    /// an overlap.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::time_window::TimeWindow;
    /// use chrono::Utc;
    ///
    /// let t0 = Utc::now();
    /// let t1 = t0 + chrono::Duration::hours(1);
    /// let t2 = t0 + chrono::Duration::hours(2);
    /// let t3 = t0 + chrono::Duration::hours(3);
    ///
    /// let a = TimeWindow::new(t0, t2).unwrap();
    /// let b = TimeWindow::new(t1, t3).unwrap();
    /// assert!(a.overlaps(&b));
    /// ```
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        self.from <= other.to && self.to >= other.from
    }

    /// Return the intersection of two windows, or `None` if they do not overlap.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use api_bones::time_window::TimeWindow;
    /// use chrono::Utc;
    ///
    /// let t0 = Utc::now();
    /// let t1 = t0 + chrono::Duration::hours(1);
    /// let t2 = t0 + chrono::Duration::hours(2);
    /// let t3 = t0 + chrono::Duration::hours(3);
    ///
    /// let a = TimeWindow::new(t0, t2).unwrap();
    /// let b = TimeWindow::new(t1, t3).unwrap();
    /// let inter = a.intersect(&b).unwrap();
    /// assert_eq!(inter.from, t1);
    /// assert_eq!(inter.to, t2);
    /// ```
    #[must_use]
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        if self.overlaps(other) {
            let from = self.from.max(other.from);
            let to = self.to.min(other.to);
            // overlaps() guarantees from <= to, so direct construction is valid
            Some(Self { from, to })
        } else {
            None
        }
    }
}

impl fmt::Display for TimeWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}, {}]", self.from, self.to)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn ts(offset_secs: i64) -> Timestamp {
        // Use a fixed epoch so tests are deterministic
        chrono::DateTime::from_timestamp(1_700_000_000 + offset_secs, 0).unwrap()
    }

    #[test]
    fn new_valid_range() {
        let w = TimeWindow::new(ts(0), ts(3600)).unwrap();
        assert_eq!(w.from, ts(0));
        assert_eq!(w.to, ts(3600));
    }

    #[test]
    fn new_equal_from_to() {
        let w = TimeWindow::new(ts(0), ts(0)).unwrap();
        assert_eq!(w.duration(), chrono::Duration::zero());
    }

    #[test]
    fn new_inverted_rejects() {
        let result = TimeWindow::new(ts(3600), ts(0));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TimeWindowError::Inverted { .. }
        ));
    }

    #[test]
    fn duration_one_hour() {
        let w = TimeWindow::new(ts(0), ts(3600)).unwrap();
        assert_eq!(w.duration(), chrono::Duration::hours(1));
    }

    #[test]
    fn contains_inside() {
        let w = TimeWindow::new(ts(0), ts(3600)).unwrap();
        assert!(w.contains(ts(1800)));
    }

    #[test]
    fn contains_on_boundary_from() {
        let w = TimeWindow::new(ts(0), ts(3600)).unwrap();
        assert!(w.contains(ts(0)));
    }

    #[test]
    fn contains_on_boundary_to() {
        let w = TimeWindow::new(ts(0), ts(3600)).unwrap();
        assert!(w.contains(ts(3600)));
    }

    #[test]
    fn contains_outside_before() {
        let w = TimeWindow::new(ts(0), ts(3600)).unwrap();
        assert!(!w.contains(ts(-1)));
    }

    #[test]
    fn contains_outside_after() {
        let w = TimeWindow::new(ts(0), ts(3600)).unwrap();
        assert!(!w.contains(ts(3601)));
    }

    #[test]
    fn overlaps_yes() {
        let a = TimeWindow::new(ts(0), ts(3600)).unwrap();
        let b = TimeWindow::new(ts(1800), ts(7200)).unwrap();
        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
    }

    #[test]
    fn overlaps_touching_boundary() {
        let a = TimeWindow::new(ts(0), ts(3600)).unwrap();
        let b = TimeWindow::new(ts(3600), ts(7200)).unwrap();
        assert!(a.overlaps(&b));
    }

    #[test]
    fn overlaps_no() {
        let a = TimeWindow::new(ts(0), ts(3600)).unwrap();
        let b = TimeWindow::new(ts(3601), ts(7200)).unwrap();
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn intersect_overlapping_returns_some() {
        let a = TimeWindow::new(ts(0), ts(3600)).unwrap();
        let b = TimeWindow::new(ts(1800), ts(7200)).unwrap();
        let inter = a.intersect(&b).unwrap();
        assert_eq!(inter.from, ts(1800));
        assert_eq!(inter.to, ts(3600));
    }

    #[test]
    fn intersect_touching_boundary_returns_some_zero_duration() {
        let a = TimeWindow::new(ts(0), ts(3600)).unwrap();
        let b = TimeWindow::new(ts(3600), ts(7200)).unwrap();
        let inter = a.intersect(&b).unwrap();
        assert_eq!(inter.from, ts(3600));
        assert_eq!(inter.to, ts(3600));
        assert_eq!(inter.duration(), chrono::Duration::zero());
    }

    #[test]
    fn intersect_no_overlap_returns_none() {
        let a = TimeWindow::new(ts(0), ts(3600)).unwrap();
        let b = TimeWindow::new(ts(3601), ts(7200)).unwrap();
        assert!(a.intersect(&b).is_none());
    }

    #[test]
    fn error_display_message() {
        let from = ts(3600);
        let to = ts(0);
        let err = TimeWindowError::Inverted { from, to };
        let msg = err.to_string();
        assert!(msg.contains("must be >="));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_roundtrip() {
        let w = TimeWindow::new(ts(0), ts(3600)).unwrap();
        let json = serde_json::to_string(&w).unwrap();
        let back: TimeWindow = serde_json::from_str(&json).unwrap();
        assert_eq!(back, w);
    }

    // Ensure Utc::now() usage compiles (chrono is required for this module).
    #[test]
    fn utc_now_is_usable() {
        let now = Utc::now();
        let later = now + chrono::Duration::seconds(1);
        let w = TimeWindow::new(now, later).unwrap();
        assert!(w.contains(now));
    }

    #[test]
    fn display_format() {
        let w = TimeWindow::new(ts(0), ts(3600)).unwrap();
        let s = w.to_string();
        assert!(s.starts_with('['));
        assert!(s.contains(','));
        assert!(s.ends_with(']'));
    }
}
