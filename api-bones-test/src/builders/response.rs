use api_bones::links::Links;
use api_bones::response::{ApiResponse, ResponseMeta};
use chrono::Utc;
use uuid::Uuid;

/// Builder for a fake [`ApiResponse<T>`].
///
/// # Quick start
///
/// ```rust
/// use api_bones_test::builders::FakeApiResponse;
///
/// let resp = FakeApiResponse::new("hello").build();
/// assert_eq!(resp.data, "hello");
/// assert!(resp.meta.request_id.is_some());
/// assert!(resp.meta.timestamp.is_some());
/// ```
pub struct FakeApiResponse<T> {
    data: T,
    meta: Option<ResponseMeta>,
    links: Option<Links>,
    request_id: Option<String>,
}

impl<T> FakeApiResponse<T> {
    #[must_use]
    pub fn new(data: T) -> Self {
        Self {
            data,
            meta: None,
            links: None,
            request_id: None,
        }
    }

    #[must_use]
    pub fn with_meta(mut self, meta: ResponseMeta) -> Self {
        self.meta = Some(meta);
        self
    }

    #[must_use]
    pub fn with_links(mut self, links: Links) -> Self {
        self.links = Some(links);
        self
    }

    #[must_use]
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    #[must_use]
    pub fn build(self) -> ApiResponse<T> {
        let request_id = self
            .request_id
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let meta = self.meta.unwrap_or_else(|| {
            ResponseMeta::new()
                .request_id(request_id)
                .timestamp(Utc::now())
        });
        let mut builder = ApiResponse::builder(self.data).meta(meta);
        if let Some(links) = self.links {
            builder = builder.links(links);
        }
        builder.build()
    }
}
