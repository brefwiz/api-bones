//! Query parameter types for list endpoints.
//!
//! Provides reusable structs for sorting, filtering, and full-text search
//! so consumers don't reinvent query parameter handling for every collection endpoint.
//!
//! # Overview
//!
//! - [`SortDirection`] — ascending or descending order
//! - [`SortParams`] — field name + direction
//! - [`FilterParams`] — field/operator/value triples for structured filtering
//! - [`SearchParams`] — full-text query with optional field scoping

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "validator")]
use validator::Validate;

// ---------------------------------------------------------------------------
// SortDirection
// ---------------------------------------------------------------------------

/// Sort order for list endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(
    feature = "serde",
    derive(Serialize, Deserialize),
    serde(rename_all = "lowercase")
)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub enum SortDirection {
    /// Ascending order (A → Z, 0 → 9).
    #[default]
    Asc,
    /// Descending order (Z → A, 9 → 0).
    Desc,
}

// ---------------------------------------------------------------------------
// SortParams
// ---------------------------------------------------------------------------

#[cfg(feature = "serde")]
fn default_sort_direction() -> SortDirection {
    SortDirection::Asc
}

/// Query parameters for sorting a collection endpoint.
///
/// ```json
/// {"sort_by": "created_at", "direction": "desc"}
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema, utoipa::IntoParams))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct SortParams {
    /// The field name to sort by (e.g. `"created_at"`, `"name"`).
    pub sort_by: String,
    /// Sort direction. Defaults to [`SortDirection::Asc`].
    #[cfg_attr(feature = "serde", serde(default = "default_sort_direction"))]
    pub direction: SortDirection,
}

impl SortParams {
    /// Create sort params with the given field and direction.
    #[must_use]
    pub fn new(sort_by: impl Into<String>, direction: SortDirection) -> Self {
        Self {
            sort_by: sort_by.into(),
            direction,
        }
    }

    /// Create sort params with ascending direction.
    #[must_use]
    pub fn asc(sort_by: impl Into<String>) -> Self {
        Self::new(sort_by, SortDirection::Asc)
    }

    /// Create sort params with descending direction.
    #[must_use]
    pub fn desc(sort_by: impl Into<String>) -> Self {
        Self::new(sort_by, SortDirection::Desc)
    }
}

// ---------------------------------------------------------------------------
// FilterParams
// ---------------------------------------------------------------------------

/// A single filter triple: `field`, `operator`, `value`.
///
/// ```json
/// {"field": "status", "operator": "eq", "value": "active"}
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct FilterEntry {
    /// The field name to filter on.
    pub field: String,
    /// The comparison operator (e.g. `"eq"`, `"neq"`, `"gt"`, `"lt"`, `"contains"`).
    pub operator: String,
    /// The value to compare against.
    pub value: String,
}

impl FilterEntry {
    /// Create a filter entry.
    #[must_use]
    pub fn new(
        field: impl Into<String>,
        operator: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            operator: operator.into(),
            value: value.into(),
        }
    }
}

/// Query parameters for structured filtering on a collection endpoint.
///
/// Each entry is a field/operator/value triple. Multiple entries are
/// AND-combined by convention; consumers may choose different semantics.
///
/// ```json
/// {"filters": [{"field": "status", "operator": "eq", "value": "active"}]}
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema, utoipa::IntoParams))]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct FilterParams {
    /// The list of filter entries.
    #[cfg_attr(feature = "serde", serde(default))]
    pub filters: Vec<FilterEntry>,
}

impl FilterParams {
    /// Create filter params from an iterator of entries.
    #[must_use]
    pub fn new(filters: impl IntoIterator<Item = FilterEntry>) -> Self {
        Self {
            filters: filters.into_iter().collect(),
        }
    }

    /// Returns `true` if no filters are set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }
}

// ---------------------------------------------------------------------------
// SearchParams
// ---------------------------------------------------------------------------

/// Query parameters for full-text search on a collection endpoint.
///
/// `query` is the search string. `fields` optionally scopes the search
/// to specific fields; when omitted the backend decides which fields to search.
///
/// ```json
/// {"query": "annual report", "fields": ["title", "description"]}
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema, utoipa::IntoParams))]
#[cfg_attr(feature = "validator", derive(Validate))]
#[cfg_attr(feature = "proptest", derive(proptest_derive::Arbitrary))]
pub struct SearchParams {
    /// The search string. Must not exceed 500 characters.
    #[cfg_attr(
        feature = "validator",
        validate(length(
            min = 1,
            max = 500,
            message = "query must be between 1 and 500 characters"
        ))
    )]
    #[cfg_attr(feature = "proptest", proptest(strategy = "search_query_strategy()"))]
    pub query: String,
    /// Optional list of field names to scope the search to.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub fields: Vec<String>,
}

impl SearchParams {
    /// Create search params targeting all fields.
    #[must_use]
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            fields: Vec::new(),
        }
    }

    /// Create search params scoped to specific fields.
    #[must_use]
    pub fn with_fields(
        query: impl Into<String>,
        fields: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            query: query.into(),
            fields: fields.into_iter().map(Into::into).collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// proptest strategy helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "proptest")]
fn search_query_strategy() -> impl proptest::strategy::Strategy<Value = String> {
    proptest::string::string_regex("[a-zA-Z0-9 ]{1,500}").expect("valid regex")
}

// ---------------------------------------------------------------------------
// arbitrary::Arbitrary manual impl — constrained SearchParams
// ---------------------------------------------------------------------------

#[cfg(feature = "arbitrary")]
impl<'a> arbitrary::Arbitrary<'a> for SearchParams {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        // Generate a query length between 1 and 500, then fill with arbitrary bytes
        // mapped to printable ASCII (32–126) to satisfy the validator constraint.
        let len = u.int_in_range(1usize..=500)?;
        let query: String = (0..len)
            .map(|_| -> arbitrary::Result<char> {
                let byte = u.int_in_range(32u8..=126)?;
                Ok(char::from(byte))
            })
            .collect::<arbitrary::Result<_>>()?;
        let fields = <Vec<String> as arbitrary::Arbitrary>::arbitrary(u)?;
        Ok(Self { query, fields })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- SortDirection ---

    #[test]
    fn sort_direction_default_is_asc() {
        assert_eq!(SortDirection::default(), SortDirection::Asc);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn sort_direction_serde_lowercase() {
        let asc = serde_json::to_value(SortDirection::Asc).unwrap();
        assert_eq!(asc, serde_json::json!("asc"));
        let desc = serde_json::to_value(SortDirection::Desc).unwrap();
        assert_eq!(desc, serde_json::json!("desc"));

        let back: SortDirection = serde_json::from_value(asc).unwrap();
        assert_eq!(back, SortDirection::Asc);
    }

    // --- SortParams ---

    #[test]
    fn sort_params_asc_helper() {
        let p = SortParams::asc("created_at");
        assert_eq!(p.sort_by, "created_at");
        assert_eq!(p.direction, SortDirection::Asc);
    }

    #[test]
    fn sort_params_desc_helper() {
        let p = SortParams::desc("name");
        assert_eq!(p.sort_by, "name");
        assert_eq!(p.direction, SortDirection::Desc);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn sort_params_serde_round_trip() {
        let p = SortParams::desc("created_at");
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["sort_by"], "created_at");
        assert_eq!(json["direction"], "desc");
        let back: SortParams = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn sort_params_serde_default_direction() {
        let json = serde_json::json!({"sort_by": "name"});
        let p: SortParams = serde_json::from_value(json).unwrap();
        assert_eq!(p.direction, SortDirection::Asc);
    }

    // --- FilterParams ---

    #[test]
    fn filter_params_default_is_empty() {
        let f = FilterParams::default();
        assert!(f.is_empty());
    }

    #[test]
    fn filter_params_new() {
        let f = FilterParams::new([FilterEntry::new("status", "eq", "active")]);
        assert!(!f.is_empty());
        assert_eq!(f.filters.len(), 1);
        assert_eq!(f.filters[0].field, "status");
        assert_eq!(f.filters[0].operator, "eq");
        assert_eq!(f.filters[0].value, "active");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn filter_params_serde_round_trip() {
        let f = FilterParams::new([FilterEntry::new("age", "gt", "18")]);
        let json = serde_json::to_value(&f).unwrap();
        let back: FilterParams = serde_json::from_value(json).unwrap();
        assert_eq!(back, f);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn filter_params_serde_empty_filters_default() {
        let json = serde_json::json!({});
        let f: FilterParams = serde_json::from_value(json).unwrap();
        assert!(f.is_empty());
    }

    // --- SearchParams ---

    #[test]
    fn search_params_new() {
        let s = SearchParams::new("annual report");
        assert_eq!(s.query, "annual report");
        assert!(s.fields.is_empty());
    }

    #[test]
    fn search_params_with_fields() {
        let s = SearchParams::with_fields("report", ["title", "description"]);
        assert_eq!(s.query, "report");
        assert_eq!(s.fields, vec!["title", "description"]);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn search_params_serde_round_trip() {
        let s = SearchParams::with_fields("hello", ["name"]);
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["query"], "hello");
        assert_eq!(json["fields"], serde_json::json!(["name"]));
        let back: SearchParams = serde_json::from_value(json).unwrap();
        assert_eq!(back, s);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn search_params_serde_omits_empty_fields() {
        let s = SearchParams::new("test");
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("fields").is_none());
    }

    #[cfg(feature = "validator")]
    #[test]
    fn search_params_validate_empty_query_fails() {
        use validator::Validate;
        let s = SearchParams::new("");
        assert!(s.validate().is_err());
    }

    #[cfg(feature = "validator")]
    #[test]
    fn search_params_validate_too_long_fails() {
        use validator::Validate;
        let s = SearchParams::new("a".repeat(501));
        assert!(s.validate().is_err());
    }

    #[cfg(feature = "validator")]
    #[test]
    fn search_params_validate_boundary_max() {
        use validator::Validate;
        let s = SearchParams::new("a".repeat(500));
        assert!(s.validate().is_ok());
    }

    #[cfg(feature = "validator")]
    #[test]
    fn search_params_validate_ok() {
        use validator::Validate;
        let s = SearchParams::new("valid query");
        assert!(s.validate().is_ok());
    }
}
