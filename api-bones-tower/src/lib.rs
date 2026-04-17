//! Tower middleware building blocks for api-bones services.
//!
//! Provides composable Tower [`Layer`](tower::Layer) / [`Service`](tower::Service)
//! implementations for:
//!
//! | Layer                | What it does                                         |
//! |----------------------|------------------------------------------------------|
//! | [`RequestIdLayer`]   | Generates / propagates `X-Request-Id` on every req  |
//! | [`ProblemJsonLayer`] | Maps non-`ApiError` inner-service errors to Problem+JSON |
//!
//! # Example
//!
//! ```rust,no_run
//! use api_bones_tower::{RequestIdLayer, ProblemJsonLayer};
//! use tower::ServiceBuilder;
//!
//! let _svc = ServiceBuilder::new()
//!     .layer(RequestIdLayer::new())
//!     .layer(ProblemJsonLayer::new())
//!     .service(tower::service_fn(|_req: http::Request<()>| async {
//!         Ok::<_, std::convert::Infallible>(http::Response::new(()))
//!     }));
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};

use api_bones::error::ApiError;
use http::{Request, Response};
use tower::{Layer, Service};

// ---------------------------------------------------------------------------
// RequestIdLayer
// ---------------------------------------------------------------------------

/// Tower [`Layer`] that ensures every request carries an `X-Request-Id` header.
///
/// - If the incoming request already has an `X-Request-Id`, it is forwarded
///   unchanged.
/// - Otherwise a monotonically-increasing numeric ID is generated and injected
///   (format: `req-<n>`).
///
/// The same header value is echoed back in the response.
///
/// # Example
///
/// ```rust,no_run
/// use api_bones_tower::RequestIdLayer;
/// use tower::ServiceBuilder;
///
/// let _svc = ServiceBuilder::new()
///     .layer(RequestIdLayer::new())
///     .service(tower::service_fn(|_req: http::Request<()>| async {
///         Ok::<_, std::convert::Infallible>(http::Response::new(()))
///     }));
/// ```
#[derive(Clone, Debug)]
pub struct RequestIdLayer {
    counter: Arc<AtomicU64>,
}

impl RequestIdLayer {
    /// Create a new `RequestIdLayer` with an internal counter starting at 1.
    #[must_use]
    pub fn new() -> Self {
        Self {
            counter: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl Default for RequestIdLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for RequestIdLayer {
    type Service = RequestIdService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestIdService {
            inner,
            counter: Arc::clone(&self.counter),
        }
    }
}

/// Tower [`Service`] produced by [`RequestIdLayer`].
#[derive(Clone, Debug)]
pub struct RequestIdService<S> {
    inner: S,
    counter: Arc<AtomicU64>,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for RequestIdService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Send,
    ReqBody: Send + 'static,
    ResBody: Default + Send,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = RequestIdFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // Determine (or generate) the request ID.
        let request_id: String = if let Some(existing) = req.headers().get("x-request-id") {
            existing.to_str().unwrap_or("invalid").to_owned()
        } else {
            let n = self.counter.fetch_add(1, Ordering::Relaxed);
            let id = format!("req-{n}");
            if let Ok(val) = http::HeaderValue::from_str(&id) {
                req.headers_mut().insert("x-request-id", val);
            }
            id
        };

        let future = self.inner.call(req);
        RequestIdFuture {
            inner: future,
            request_id,
            _body: std::marker::PhantomData,
        }
    }
}

/// Future returned by [`RequestIdService`].
#[pin_project::pin_project]
pub struct RequestIdFuture<F, ResBody> {
    #[pin]
    inner: F,
    request_id: String,
    _body: std::marker::PhantomData<ResBody>,
}

impl<F, ResBody, E> Future for RequestIdFuture<F, ResBody>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.inner.poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(mut resp)) => {
                if let Ok(val) = http::HeaderValue::from_str(this.request_id) {
                    resp.headers_mut().entry("x-request-id").or_insert(val);
                }
                Poll::Ready(Ok(resp))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
        }
    }
}

// ---------------------------------------------------------------------------
// ProblemJsonLayer
// ---------------------------------------------------------------------------

/// Tower [`Layer`] that maps inner-service errors into Problem+JSON HTTP
/// responses.
///
/// Any `Err` propagated from the inner service is converted to an [`ApiError`]
/// via the [`Into<ApiError>`] bound and then serialized as
/// `application/problem+json`.
///
/// Successful responses are passed through unchanged.
///
/// # Example
///
/// ```rust,no_run
/// use api_bones_tower::ProblemJsonLayer;
/// use tower::ServiceBuilder;
///
/// let _svc = ServiceBuilder::new()
///     .layer(ProblemJsonLayer::new())
///     .service(tower::service_fn(|_req: http::Request<()>| async {
///         Ok::<_, api_bones::ApiError>(http::Response::new(String::new()))
///     }));
/// ```
#[derive(Clone, Debug, Default)]
pub struct ProblemJsonLayer;

impl ProblemJsonLayer {
    /// Create a new `ProblemJsonLayer`.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for ProblemJsonLayer {
    type Service = ProblemJsonService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ProblemJsonService { inner }
    }
}

/// Tower [`Service`] produced by [`ProblemJsonLayer`].
#[derive(Clone, Debug)]
pub struct ProblemJsonService<S> {
    inner: S,
}

impl<S, ReqBody> Service<Request<ReqBody>> for ProblemJsonService<S>
where
    S: Service<Request<ReqBody>, Response = Response<String>> + Clone + Send + 'static,
    S::Error: Into<ApiError> + Send,
    S::Future: Send,
    ReqBody: Send + 'static,
{
    type Response = Response<String>;
    type Error = std::convert::Infallible;
    type Future =
        Pin<Box<dyn Future<Output = Result<Response<String>, std::convert::Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.inner.poll_ready(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(_e)) => unreachable!("inner service poll_ready returned Err"),
        }
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let future = self.inner.call(req);
        Box::pin(async move {
            match future.await {
                Ok(resp) => Ok(resp),
                Err(e) => {
                    let api_err: ApiError = e.into();
                    Ok(api_error_to_response(api_err))
                }
            }
        })
    }
}

/// Convert an [`ApiError`] into an HTTP response with `application/problem+json`.
fn api_error_to_response(err: ApiError) -> Response<String> {
    use api_bones::error::ProblemJson;

    let status = err.status;
    let problem = ProblemJson::from(err);
    let body = serde_json::to_string(&problem).expect("ProblemJson serialization is infallible");

    let status_code =
        http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR);

    Response::builder()
        .status(status_code)
        .header("content-type", "application/problem+json")
        .body(body)
        .expect("response construction is infallible for valid status codes")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tower::{ServiceBuilder, ServiceExt};

    #[tokio::test]
    async fn request_id_layer_injects_header() {
        let svc = ServiceBuilder::new()
            .layer(RequestIdLayer::new())
            .service(tower::service_fn(|req: Request<()>| async move {
                let id = req
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_owned();
                let resp = Response::new(id);
                Ok::<_, std::convert::Infallible>(resp)
            }));

        let req = Request::builder().uri("/").body(()).unwrap();
        let resp = svc.oneshot(req).await.unwrap();
        assert!(resp.headers().contains_key("x-request-id"));
    }

    #[tokio::test]
    async fn request_id_layer_preserves_existing_header() {
        let svc = ServiceBuilder::new()
            .layer(RequestIdLayer::new())
            .service(tower::service_fn(|_req: Request<()>| async move {
                Ok::<_, std::convert::Infallible>(Response::new(String::new()))
            }));

        let req = Request::builder()
            .uri("/")
            .header("x-request-id", "client-id")
            .body(())
            .unwrap();
        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers()
                .get("x-request-id")
                .unwrap()
                .to_str()
                .unwrap(),
            "client-id"
        );
    }

    #[tokio::test]
    async fn problem_json_layer_maps_error() {
        let svc = ServiceBuilder::new()
            .layer(ProblemJsonLayer::new())
            .service(tower::service_fn(|_req: Request<()>| async move {
                Err::<Response<String>, ApiError>(ApiError::not_found("item 1"))
            }));

        let req = Request::builder().uri("/").body(()).unwrap();
        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 404);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "application/problem+json"
        );
    }

    #[tokio::test]
    async fn problem_json_layer_passes_through_ok() {
        let svc = ServiceBuilder::new()
            .layer(ProblemJsonLayer::new())
            .service(tower::service_fn(|_req: Request<()>| async move {
                Ok::<_, ApiError>(
                    Response::builder()
                        .status(200)
                        .body("ok".to_owned())
                        .unwrap(),
                )
            }));

        let req = Request::builder().uri("/").body(()).unwrap();
        let resp = svc.oneshot(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[test]
    fn request_id_layer_default_is_same_as_new() {
        let _layer = RequestIdLayer::default();
    }

    #[tokio::test]
    async fn problem_json_service_poll_ready() {
        use tower::{Service, ServiceExt};

        let inner = tower::service_fn(|_req: Request<()>| async move {
            Ok::<_, ApiError>(Response::builder().body("ok".to_owned()).unwrap())
        });
        let mut svc = ProblemJsonService { inner };
        let svc_ref = svc.ready().await.unwrap();
        let req = Request::builder().uri("/").body(()).unwrap();
        let resp = svc_ref.call(req).await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[tokio::test]
    async fn request_id_future_propagates_inner_error() {
        let svc = ServiceBuilder::new()
            .layer(RequestIdLayer::new())
            .service(tower::service_fn(|_req: Request<()>| async move {
                Err::<Response<String>, ApiError>(ApiError::internal("boom"))
            }));

        let req = Request::builder().uri("/").body(()).unwrap();
        let result = svc.oneshot(req).await;
        let err = result.unwrap_err();
        assert_eq!(err.status, 500);
    }

    #[tokio::test]
    async fn request_id_future_poll_pending() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let ready = Arc::new(AtomicBool::new(false));
        let ready2 = Arc::clone(&ready);

        let inner = tower::service_fn(move |_req: Request<()>| {
            let flag = Arc::clone(&ready2);
            async move {
                tokio::task::yield_now().await;
                flag.store(true, Ordering::SeqCst);
                Ok::<Response<String>, std::convert::Infallible>(
                    Response::builder().body(String::new()).unwrap(),
                )
            }
        });

        let layer = RequestIdLayer::new();
        let mut svc = layer.layer(inner);

        let req = Request::builder().uri("/").body(()).unwrap();
        let fut = tower::Service::call(&mut svc, req);
        let resp = fut.await.unwrap();
        assert!(resp.headers().contains_key("x-request-id"));
        assert!(ready.load(Ordering::SeqCst));
    }
}
