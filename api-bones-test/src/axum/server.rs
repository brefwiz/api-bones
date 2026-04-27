use api_bones::error::{ApiError, ErrorCode};
use api_bones::pagination::PaginatedResponse;
use axum::Router;
use axum_test::TestServer as AxumTestServer;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::axum::assertions::{assert_envelope, assert_paginated, assert_problem_json};

/// Thin wrapper over [`axum_test::TestServer`] with pre-installed brefwiz middleware.
///
/// # Quick start
///
/// ```rust,no_run
/// use axum::Router;
/// use api_bones_test::axum::TestServer;
///
/// # async fn run() {
/// let app = Router::new(); // your router here
/// let server = TestServer::new(app);
/// # }
/// ```
pub struct TestServer {
    inner: AxumTestServer,
}

impl TestServer {
    #[must_use]
    pub fn new(router: Router) -> Self {
        Self {
            inner: AxumTestServer::new(router),
        }
    }

    #[must_use]
    pub fn inner(&self) -> &AxumTestServer {
        &self.inner
    }

    /// GET `path` and assert an envelope response; return the unwrapped payload.
    pub async fn get_envelope<T: DeserializeOwned>(&self, path: &str) -> T {
        let resp = self.inner.get(path).await;
        assert_envelope::<T>(resp).await
    }

    /// GET `path` and assert a problem-json response with `expected_code`.
    pub async fn get_problem(&self, path: &str, expected_code: ErrorCode) -> ApiError {
        let resp = self.inner.get(path).await;
        assert_problem_json(resp, expected_code).await
    }

    /// GET `path` and assert a paginated envelope response.
    pub async fn get_paginated<T: DeserializeOwned>(&self, path: &str) -> PaginatedResponse<T> {
        let resp = self.inner.get(path).await;
        assert_paginated::<T>(resp).await
    }

    /// POST `path` with a JSON body and assert an envelope response.
    pub async fn post_envelope<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> T {
        let resp = self.inner.post(path).json(body).await;
        assert_envelope::<T>(resp).await
    }
}
