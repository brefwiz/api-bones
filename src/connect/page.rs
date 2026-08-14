// SPDX-License-Identifier: MIT
//! Offset pagination builder for Connect proto adapters (ADR-0096).

/// Brefwiz standard default page size.
pub const DEFAULT_LIMIT: u64 = 20;
/// Brefwiz standard maximum page size.
pub const MAX_LIMIT: u64 = 200;

/// Normalized offset-page result.
///
/// Constructed only via [`build_page`] or [`build_offset_page`] — the inner
/// fields are private to prevent hand-rolled construction. Call
/// [`OffsetPage::into_parts`] to destructure into the four values needed to
/// populate a proto response message.
///
/// # Why a newtype?
///
/// The private constructor is the enforcement mechanism (ADR-0096 §Enforcement
/// Level 1): call sites that expect `OffsetPage` cannot be satisfied with an
/// inline-built response struct, making the canonical builder mandatory at
/// compile time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetPage {
    pub(super) total_count: u64,
    pub(super) has_more: bool,
    pub(super) limit: u64,
    pub(super) offset: u64,
}

impl OffsetPage {
    /// Destructure into `(total_count, has_more, limit, offset)`.
    ///
    /// Use these values to populate the `page` field of a proto response:
    ///
    /// ```rust,ignore
    /// let page = build_page(req.page.limit, req.page.offset, items.len(), total);
    /// let (total_count, has_more, limit, offset) = page.into_parts();
    /// response.page = SomeOffsetPageResponse { total_count, has_more, limit, offset, .. };
    /// ```
    #[must_use]
    pub fn into_parts(self) -> (u64, bool, u64, u64) {
        (self.total_count, self.has_more, self.limit, self.offset)
    }

    /// Total number of items across all pages.
    #[must_use]
    pub fn total_count(&self) -> u64 {
        self.total_count
    }

    /// Whether more items exist beyond this page.
    #[must_use]
    pub fn has_more(&self) -> bool {
        self.has_more
    }

    /// The effective limit used for this page (after clamping).
    #[must_use]
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// The offset used for this page.
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.offset
    }
}

/// Normalize inputs and build an [`OffsetPage`].
///
/// - `limit_in == 0` → `default_limit`
/// - `limit_in > max_limit` → clamped to `max_limit`
/// - `has_more` is derived from `offset + item_count < total`
///
/// # Example
///
/// ```rust
/// use api_bones::connect::{build_offset_page, DEFAULT_LIMIT, MAX_LIMIT};
///
/// let page = build_offset_page(0, 0, 5, 100, DEFAULT_LIMIT, MAX_LIMIT);
/// assert_eq!(page.limit(), DEFAULT_LIMIT);
/// assert!(page.has_more());
/// ```
#[must_use]
pub fn build_offset_page(
    limit_in: u64,
    offset: u64,
    item_count: usize,
    total: u64,
    default_limit: u64,
    max_limit: u64,
) -> OffsetPage {
    let limit = if limit_in == 0 {
        default_limit
    } else {
        limit_in.min(max_limit)
    };
    let has_more = offset + (item_count as u64) < total;
    OffsetPage {
        total_count: total,
        has_more,
        limit,
        offset,
    }
}

/// [`build_offset_page`] with brefwiz standard defaults (default=20, max=200).
///
/// # Example
///
/// ```rust
/// use api_bones::connect::build_page;
///
/// let page = build_page(0, 0, 20, 42);
/// assert_eq!(page.limit(), 20);
/// assert_eq!(page.total_count(), 42);
/// assert!(page.has_more());
/// ```
#[must_use]
pub fn build_page(limit_in: u64, offset: u64, item_count: usize, total: u64) -> OffsetPage {
    build_offset_page(
        limit_in,
        offset,
        item_count,
        total,
        DEFAULT_LIMIT,
        MAX_LIMIT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_limit_uses_default() {
        let p = build_page(0, 0, 5, 100);
        assert_eq!(p.limit(), DEFAULT_LIMIT);
    }

    #[test]
    fn oversized_limit_clamped() {
        let p = build_page(9999, 0, 5, 100);
        assert_eq!(p.limit(), MAX_LIMIT);
    }

    #[test]
    fn explicit_limit_preserved() {
        let p = build_page(50, 0, 5, 100);
        assert_eq!(p.limit(), 50);
    }

    #[test]
    fn has_more_true_when_items_remain() {
        let p = build_page(20, 0, 20, 100);
        assert!(p.has_more());
    }

    #[test]
    fn has_more_false_on_last_page() {
        let p = build_page(20, 80, 20, 100);
        assert!(!p.has_more());
    }

    #[test]
    fn has_more_false_on_exact_fit() {
        let p = build_page(20, 0, 100, 100);
        assert!(!p.has_more());
    }

    #[test]
    fn empty_result_no_more() {
        let p = build_page(20, 0, 0, 0);
        assert!(!p.has_more());
        assert_eq!(p.total_count(), 0);
    }

    #[test]
    fn into_parts_destructures() {
        let p = build_page(10, 5, 10, 50);
        let (total, has_more, limit, offset) = p.into_parts();
        assert_eq!(total, 50);
        assert!(has_more);
        assert_eq!(limit, 10);
        assert_eq!(offset, 5);
    }
}
