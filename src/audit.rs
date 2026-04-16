//! Audit metadata for API resources.
//!
//! Provides [`AuditInfo`], an embeddable struct that tracks when a resource
//! was created and last updated, and optionally by whom.
//!
//! # Standards
//! - Timestamps: [RFC 3339](https://www.rfc-editor.org/rfc/rfc3339)

use crate::common::Timestamp;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// AuditInfo
// ---------------------------------------------------------------------------

/// Audit metadata embedded in API resource structs.
///
/// Tracks creation and last-update times (RFC 3339) and optional actor
/// references for `created_by` and `updated_by`.
///
/// # Example
///
/// ```rust
/// # #[cfg(feature = "chrono")] {
/// use shared_types::AuditInfo;
///
/// let mut info = AuditInfo::now(Some("alice".to_string()));
/// info.touch(Some("bob".to_string()));
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
pub struct AuditInfo {
    /// When the resource was created (RFC 3339).
    #[cfg_attr(
        feature = "utoipa",
        schema(value_type = String, format = DateTime)
    )]
    pub created_at: Timestamp,
    /// When the resource was last updated (RFC 3339).
    #[cfg_attr(
        feature = "utoipa",
        schema(value_type = String, format = DateTime)
    )]
    pub updated_at: Timestamp,
    /// Identity of the actor who created the resource, if known.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub created_by: Option<String>,
    /// Identity of the actor who last updated the resource, if known.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub updated_by: Option<String>,
}

impl AuditInfo {
    /// Construct an `AuditInfo` with explicit timestamps.
    #[must_use]
    pub fn new(
        created_at: Timestamp,
        updated_at: Timestamp,
        created_by: Option<String>,
        updated_by: Option<String>,
    ) -> Self {
        Self {
            created_at,
            updated_at,
            created_by,
            updated_by,
        }
    }

    /// Construct an `AuditInfo` with `created_at` and `updated_at` set to the
    /// current UTC time.
    ///
    /// Requires the `chrono` feature.
    #[cfg(feature = "chrono")]
    #[must_use]
    pub fn now(created_by: Option<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            created_at: now,
            updated_at: now,
            created_by,
            updated_by: None,
        }
    }

    /// Update `updated_at` to the current UTC time and set `updated_by`.
    ///
    /// Requires the `chrono` feature.
    #[cfg(feature = "chrono")]
    pub fn touch(&mut self, updated_by: Option<String>) {
        self.updated_at = chrono::Utc::now();
        self.updated_by = updated_by;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "chrono")]
    #[test]
    fn now_sets_created_at_and_updated_at() {
        let before = chrono::Utc::now();
        let info = AuditInfo::now(Some("alice".to_string()));
        let after = chrono::Utc::now();

        assert!(info.created_at >= before && info.created_at <= after);
        assert!(info.updated_at >= before && info.updated_at <= after);
        assert_eq!(info.created_by.as_deref(), Some("alice"));
        assert!(info.updated_by.is_none());
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn now_without_actor() {
        let info = AuditInfo::now(None);
        assert!(info.created_by.is_none());
        assert!(info.updated_by.is_none());
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn touch_updates_updated_at_and_updated_by() {
        let mut info = AuditInfo::now(Some("alice".to_string()));
        let before_touch = chrono::Utc::now();
        info.touch(Some("bob".to_string()));
        let after_touch = chrono::Utc::now();

        assert!(info.updated_at >= before_touch && info.updated_at <= after_touch);
        assert_eq!(info.updated_by.as_deref(), Some("bob"));
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn touch_without_actor_clears_updated_by() {
        let mut info = AuditInfo::now(Some("alice".to_string()));
        info.touch(Some("bob".to_string()));
        info.touch(None);
        assert!(info.updated_by.is_none());
    }

    #[cfg(all(feature = "chrono", feature = "serde"))]
    #[test]
    fn serde_round_trip_with_actors() {
        let info = AuditInfo::now(Some("alice".to_string()));
        let json = serde_json::to_value(&info).unwrap();
        let back: AuditInfo = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }

    #[cfg(all(feature = "chrono", feature = "serde"))]
    #[test]
    fn serde_omits_none_optional_fields() {
        let info = AuditInfo::now(None);
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("created_by").is_none());
        assert!(json.get("updated_by").is_none());
    }

    #[cfg(all(feature = "chrono", feature = "serde"))]
    #[test]
    fn serde_round_trip_without_actors() {
        let info = AuditInfo::now(None);
        let json = serde_json::to_value(&info).unwrap();
        let back: AuditInfo = serde_json::from_value(json).unwrap();
        assert_eq!(back, info);
    }

    #[cfg(feature = "chrono")]
    #[test]
    fn new_constructor() {
        let now = chrono::Utc::now();
        let info = AuditInfo::new(now, now, Some("alice".to_string()), None);
        assert_eq!(info.created_at, now);
        assert_eq!(info.updated_at, now);
        assert_eq!(info.created_by.as_deref(), Some("alice"));
        assert!(info.updated_by.is_none());
    }
}
