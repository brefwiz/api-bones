use api_bones::pagination::PaginatedResponse;

/// Builder for a fake [`PaginatedResponse<T>`].
///
/// # Quick start
///
/// ```rust
/// use api_bones_test::builders::FakePaginated;
///
/// let resp = FakePaginated::new(vec![1u32, 2, 3]).build();
/// assert_eq!(resp.items.len(), 3);
/// assert_eq!(resp.total_count, 3);
/// assert_eq!(resp.limit, 3);
/// assert_eq!(resp.offset, 0);
/// assert!(!resp.has_more);
/// ```
pub struct FakePaginated<T> {
    items: Vec<T>,
    total: Option<u64>,
    limit: Option<u64>,
    offset: u64,
    has_more: Option<bool>,
}

impl<T> FakePaginated<T> {
    #[must_use]
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            total: None,
            limit: None,
            offset: 0,
            has_more: None,
        }
    }

    #[must_use]
    pub fn total(mut self, total: u64) -> Self {
        self.total = Some(total);
        self
    }

    #[must_use]
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    #[must_use]
    pub fn offset(mut self, offset: u64) -> Self {
        self.offset = offset;
        self
    }

    #[must_use]
    pub fn has_more(mut self, has_more: bool) -> Self {
        self.has_more = Some(has_more);
        self
    }

    #[must_use]
    pub fn build(self) -> PaginatedResponse<T> {
        let count = self.items.len() as u64;
        let total = self.total.unwrap_or(count);
        let limit = self.limit.unwrap_or(count);
        let has_more = self.has_more.unwrap_or_else(|| self.offset + count < total);
        PaginatedResponse {
            items: self.items,
            total_count: total,
            has_more,
            limit,
            offset: self.offset,
        }
    }
}
