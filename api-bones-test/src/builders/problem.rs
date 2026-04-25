use api_bones::error::{ApiError, ErrorCode, ValidationError};

/// Builder for a fake [`ApiError`].
///
/// # Quick start
///
/// ```rust
/// use api_bones::error::ErrorCode;
/// use api_bones_test::builders::FakeProblem;
///
/// let err = FakeProblem::new(ErrorCode::ResourceNotFound)
///     .detail("Widget 42 not found")
///     .build();
/// assert_eq!(err.code, ErrorCode::ResourceNotFound);
/// assert_eq!(err.status, 404);
/// ```
pub struct FakeProblem {
    code: ErrorCode,
    detail: Option<String>,
    title: Option<String>,
    status: Option<u16>,
    request_id: Option<String>,
    fields: Vec<(String, String)>,
}

impl FakeProblem {
    #[must_use]
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            detail: None,
            title: None,
            status: None,
            request_id: None,
            fields: Vec::new(),
        }
    }

    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn field(mut self, name: impl Into<String>, message: impl Into<String>) -> Self {
        self.fields.push((name.into(), message.into()));
        self
    }

    #[must_use]
    pub fn status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    #[must_use]
    pub fn request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    #[must_use]
    pub fn build(self) -> ApiError {
        let detail = self.detail.unwrap_or_else(|| self.code.title().to_string());
        let mut err = ApiError::new(self.code, detail);
        if let Some(title) = self.title {
            err.title = title;
        }
        if let Some(status) = self.status {
            err.status = status;
        }
        if let Some(id_str) = self.request_id {
            if let Ok(uuid) = id_str.parse::<uuid::Uuid>() {
                err = err.with_request_id(uuid);
            }
        }
        if !self.fields.is_empty() {
            let errors: Vec<ValidationError> = self
                .fields
                .into_iter()
                .map(|(field, message)| ValidationError {
                    field,
                    message,
                    rule: None,
                })
                .collect();
            err = err.with_errors(errors);
        }
        err
    }
}
