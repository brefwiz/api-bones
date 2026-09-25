// SPDX-License-Identifier: MIT
use buffa_types::google::protobuf::Timestamp;
use chrono::{DateTime, Utc};

/// Convert a [`chrono::DateTime<Utc>`] to a [`Timestamp`] (`google.protobuf.Timestamp`).
///
/// # Panics
///
/// Never in practice: `chrono` guarantees `timestamp_subsec_nanos()` is
/// always `< 1_000_000_000`, which fits in an `i32` with room to spare.
///
/// # Example
///
/// ```rust
/// use api_bones::connect::chrono_to_timestamp;
///
/// let ts = chrono_to_timestamp(chrono::Utc::now());
/// assert!(ts.seconds >= 0);
/// ```
#[must_use]
pub fn chrono_to_timestamp(dt: DateTime<Utc>) -> Timestamp {
    let subsec_nanos = i32::try_from(dt.timestamp_subsec_nanos())
        .expect("chrono guarantees subsec_nanos < 1_000_000_000, which fits in i32");
    Timestamp::from_unix(dt.timestamp(), subsec_nanos)
}

/// Convert an `Option<DateTime<Utc>>` to an optional proto Timestamp.
///
/// Returns `None` when the input is `None`, forwarding to [`chrono_to_timestamp`]
/// otherwise.
///
/// # Example
///
/// ```rust
/// use api_bones::connect::chrono_opt_to_timestamp;
///
/// assert!(chrono_opt_to_timestamp(None).is_none());
/// assert!(chrono_opt_to_timestamp(Some(chrono::Utc::now())).is_some());
/// ```
#[must_use]
pub fn chrono_opt_to_timestamp(dt: Option<DateTime<Utc>>) -> Option<Timestamp> {
    dt.map(chrono_to_timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    #[test]
    fn epoch_round_trips() {
        let dt = Utc.timestamp_opt(0, 0).unwrap();
        let ts = chrono_to_timestamp(dt);
        assert_eq!(ts.seconds, 0);
        assert_eq!(ts.nanos, 0);
    }

    #[test]
    fn subsec_nanos_preserved() {
        let dt = Utc.timestamp_opt(1_700_000_000, 123_456_789).unwrap();
        let ts = chrono_to_timestamp(dt);
        assert_eq!(ts.seconds, 1_700_000_000);
        assert_eq!(ts.nanos, 123_456_789);
    }

    #[test]
    fn opt_none_stays_none() {
        assert!(chrono_opt_to_timestamp(None).is_none());
    }

    #[test]
    fn opt_some_converts() {
        let dt = Utc.timestamp_opt(1_000, 0).unwrap();
        assert!(chrono_opt_to_timestamp(Some(dt)).is_some());
    }
}
