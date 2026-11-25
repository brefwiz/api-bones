// SPDX-License-Identifier: MIT
//! The native Connect server the wasm32 fetch transport suite runs against.
//!
//! It serves `bones.test.v1.Fetch` over loopback with the real connectrpc
//! server stack. The CSRF decision is the one `api-bones-connect` ships, so
//! the server and the transport's replay rule cannot disagree. A request
//! whose path starts with `public/` is served as the same RPC without the
//! prefix (and without any mount segments between the prefix and the
//! `Service/Method` pair), the way a BFF fronts its public read lane.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use api_bones_connect::{CSRF_HEADER, csrf_replay_token};
use buffa_types::google::protobuf::{Empty, Int64Value, StringValue};
use bytes::Bytes;
use connectrpc::codec::{CodecFormat, header::TIMEOUT_MS};
use connectrpc::dispatcher::{RequestStream, StreamingResult, UnaryResult};
use connectrpc::payload::Payload;
use connectrpc::{
    ConnectError, ConnectRpcService, Dispatcher, Encodable, MethodDescriptor, RequestContext,
    Response, Router, Server, ServiceRequest, ServiceResult, ServiceStream,
};
use futures_util::stream;
use http::{HeaderMap, HeaderName, HeaderValue};

#[expect(
    clippy::too_many_lines,
    reason = "connectrpc and buffa generate this module; a fix here dies at the next codegen run"
)]
mod generated {
    connectrpc::include_generated!();
}

use generated::bones::test::v1::{Fetch, FetchExt};

/// Gap between two messages of the `Watch` stream.
const TICK: Duration = Duration::from_millis(20);

/// What the server remembers between calls.
struct State {
    /// The current CSRF token is `csrf-<epoch>`.
    epoch: AtomicU64,
    /// Labels of the requests the client abandoned, in the order noticed.
    aborted: Mutex<Vec<&'static str>>,
}

impl State {
    fn token(&self) -> String {
        format!("csrf-{}", self.epoch.load(Ordering::SeqCst))
    }

    /// Replace the current token and return the new one.
    fn mint(&self) -> String {
        format!("csrf-{}", self.epoch.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

/// Records its request as abandoned when dropped before [`finish`](Self::finish):
/// the server drops a handler's future or response stream when the client
/// aborts the fetch.
struct AbortGuard {
    label: &'static str,
    state: Arc<State>,
    finished: bool,
}

impl AbortGuard {
    fn new(label: &'static str, state: &Arc<State>) -> Self {
        Self {
            label,
            state: Arc::clone(state),
            finished: false,
        }
    }

    fn finish(&mut self) {
        self.finished = true;
    }
}

impl Drop for AbortGuard {
    fn drop(&mut self) {
        if !self.finished {
            self.state
                .aborted
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(self.label);
        }
    }
}

struct FetchService {
    state: Arc<State>,
    /// Base URL of the origin that redirects to this server.
    redirector: String,
}

impl FetchService {
    fn new(redirector: String) -> Self {
        Self {
            redirector,
            state: Arc::new(State {
                epoch: AtomicU64::new(1),
                aborted: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Accept the call when the request echoes the current token; otherwise
    /// refuse with 403 and the current token, which is what the transport's
    /// replay rule keys on.
    fn require_csrf(&self, ctx: &RequestContext) -> Result<(), ConnectError> {
        let current = self.state.token();
        let sent = ctx
            .header(CSRF_HEADER)
            .and_then(|value| value.to_str().ok());
        if csrf_replay_token(403, Some(&current), sent).is_none() {
            return Ok(());
        }
        let value = HeaderValue::from_str(&current)
            .map_err(|error| ConnectError::internal(error.to_string()))?;
        let mut headers = HeaderMap::new();
        headers.insert(HeaderName::from_static(CSRF_HEADER), value);
        Err(ConnectError::permission_denied("missing or stale CSRF token").with_headers(headers))
    }
}

fn text(value: impl Into<String>) -> StringValue {
    StringValue {
        value: value.into(),
        ..Default::default()
    }
}

impl Fetch for FetchService {
    // Handlers without an await point return a ready future: the trait's
    // methods are `-> impl Future`, so no `async fn` is needed.
    fn session<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, Empty>,
    ) -> impl Future<Output = ServiceResult<impl Encodable<StringValue> + Send + use<'a>>> + Send
    {
        let token = self.state.mint();
        std::future::ready(
            Response::new(text(token.clone()))
                .try_with_header(CSRF_HEADER, token)
                .map_err(|error| ConnectError::internal(error.to_string())),
        )
    }

    fn rotate<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, Empty>,
    ) -> impl Future<Output = ServiceResult<impl Encodable<Empty> + Send + use<'a>>> + Send {
        self.state.mint();
        std::future::ready(Response::ok(Empty::default()))
    }

    fn write<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, StringValue>,
    ) -> impl Future<Output = ServiceResult<impl Encodable<StringValue> + Send + use<'a>>> + Send
    {
        std::future::ready(
            self.require_csrf(&ctx)
                .and_then(|()| Response::ok(request.to_owned_message())),
        )
    }

    fn read<'a>(
        &'a self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, StringValue>,
    ) -> impl Future<Output = ServiceResult<impl Encodable<StringValue> + Send + use<'a>>> + Send
    {
        std::future::ready(Response::ok(request.to_owned_message()))
    }

    async fn delay<'a>(
        &'a self,
        ctx: RequestContext,
        request: ServiceRequest<'_, Int64Value>,
    ) -> ServiceResult<impl Encodable<StringValue> + Send + use<'a>> {
        let timeout = ctx
            .header(TIMEOUT_MS)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let millis = u64::try_from(request.to_owned_message().value).unwrap_or(0);
        let mut guard = AbortGuard::new("Delay", &self.state);
        tokio::time::sleep(Duration::from_millis(millis)).await;
        guard.finish();
        Response::ok(text(timeout))
    }

    fn watch(
        &self,
        _ctx: RequestContext,
        request: ServiceRequest<'_, Int64Value>,
    ) -> impl Future<
        Output = ServiceResult<ServiceStream<impl Encodable<StringValue> + Send + use<>>>,
    > + Send {
        let count = request.to_owned_message().value;
        let guard = AbortGuard::new("Watch", &self.state);
        std::future::ready(Response::stream_ok(stream::unfold(
            (0_i64, guard),
            move |(index, mut guard)| async move {
                if index >= count {
                    guard.finish();
                    return None;
                }
                tokio::time::sleep(TICK).await;
                let item: Result<StringValue, ConnectError> = Ok(text(format!("tick-{index}")));
                Some((item, (index + 1, guard)))
            },
        )))
    }

    fn aborted<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, Empty>,
    ) -> impl Future<Output = ServiceResult<impl Encodable<StringValue> + Send + use<'a>>> + Send
    {
        let labels = self
            .state
            .aborted
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .join(",");
        std::future::ready(Response::ok(text(labels)))
    }

    fn redirector<'a>(
        &'a self,
        _ctx: RequestContext,
        _request: ServiceRequest<'_, Empty>,
    ) -> impl Future<Output = ServiceResult<impl Encodable<StringValue> + Send + use<'a>>> + Send
    {
        std::future::ready(Response::ok(text(self.redirector.clone())))
    }
}

/// The `Service/Method` pair of a public-lane path (`public/<mount>/Service/Method`),
/// or the path itself when it is not under `public/`.
fn route_path(path: &str) -> &str {
    let Some(rest) = path.strip_prefix("public/") else {
        return path;
    };
    rest.rmatch_indices('/')
        .nth(1)
        .map_or(rest, |(at, _)| &rest[at + 1..])
}

/// Serves the public lane's paths as the RPCs they front.
struct PublicLane<D>(D);

impl<D: Dispatcher> Dispatcher for PublicLane<D> {
    fn lookup(&self, path: &str) -> Option<MethodDescriptor> {
        self.0.lookup(route_path(path))
    }

    fn call_unary(
        &self,
        path: &str,
        ctx: RequestContext,
        request: Payload,
        format: CodecFormat,
    ) -> UnaryResult {
        self.0.call_unary(route_path(path), ctx, request, format)
    }

    fn call_server_streaming(
        &self,
        path: &str,
        ctx: RequestContext,
        request: Bytes,
        format: CodecFormat,
    ) -> StreamingResult {
        self.0
            .call_server_streaming(route_path(path), ctx, request, format)
    }

    fn call_client_streaming(
        &self,
        path: &str,
        ctx: RequestContext,
        requests: RequestStream,
        format: CodecFormat,
    ) -> UnaryResult {
        self.0
            .call_client_streaming(route_path(path), ctx, requests, format)
    }

    fn call_bidi_streaming(
        &self,
        path: &str,
        ctx: RequestContext,
        requests: RequestStream,
        format: CodecFormat,
    ) -> StreamingResult {
        self.0
            .call_bidi_streaming(route_path(path), ctx, requests, format)
    }
}

/// Reads one request, body included, and answers it with a 307 to the same
/// path under `target`, carrying a CSRF token a followed redirect would leak.
async fn redirect(mut stream: TcpStream, target: &str) -> std::io::Result<()> {
    const END_OF_HEAD: &[u8] = b"\r\n\r\n";
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let head_end = loop {
        if let Some(at) = buffer
            .windows(END_OF_HEAD.len())
            .position(|window| window == END_OF_HEAD)
        {
            break at;
        }
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(chunk.get(..read).unwrap_or_default());
    };
    let head = String::from_utf8_lossy(buffer.get(..head_end).unwrap_or_default()).into_owned();
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .unwrap_or("/");
    let length = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let mut missing = length.saturating_sub(buffer.len() - head_end - END_OF_HEAD.len());
    while missing > 0 {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        missing = missing.saturating_sub(read);
    }
    let response = format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}{path}\r\n\
         {CSRF_HEADER}: redirector-token\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}

/// Answers every connection to `listener` with a 307 to `target`.
async fn redirector(listener: TcpListener, target: String) {
    while let Ok((stream, _)) = listener.accept().await {
        let target = target.clone();
        tokio::spawn(async move {
            // A client that goes away mid-request is the only failure.
            let _ = redirect(stream, &target).await;
        });
    }
}

/// Bind `127.0.0.1:0`, publish the base URL (`http://127.0.0.1:<port>`) in
/// `addr_file`, and serve until the process ends.
///
/// The file appears complete or not at all: it is written beside the target
/// and renamed into place, so a waiter never reads a partial URL.
///
/// # Errors
///
/// Returns an error when the listener cannot bind, the address file cannot be
/// written, or the server fails.
pub async fn serve(addr_file: &Path) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bound = Server::bind("127.0.0.1:0").await?;
    let url = format!("http://{}", bound.local_addr()?);
    let redirecting = TcpListener::bind("127.0.0.1:0").await?;
    let redirector_url = format!("http://{}", redirecting.local_addr()?);
    tokio::spawn(redirector(redirecting, url.clone()));
    let staging = addr_file.with_extension("tmp");
    std::fs::write(&staging, url)?;
    std::fs::rename(&staging, addr_file)?;

    let router = FetchExt::register(Arc::new(FetchService::new(redirector_url)), Router::new());
    bound
        .serve_with_service(ConnectRpcService::new(PublicLane(router)))
        .await
}
