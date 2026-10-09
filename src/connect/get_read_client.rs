// SPDX-License-Identifier: MIT
//! Connect GET for policy-eligible reads, chosen from the transport.
//!
//! A generated Connect client always sends a unary call as a POST. A read the
//! generated policy declares `NO_SIDE_EFFECTS` may instead travel as a Connect
//! GET, which a cache or a GET-only lane can serve. This module is the Rust
//! half of the browser transport's GET selection in `api-bones-connect-ts`:
//! the same decision, read from the same generated policy artifact
//! (`connect-method-policy.json`), at the same point -- the client
//! constructor, never per call.
//!
//! The wrapped transport receives the request the generated client built. For
//! an eligible unary read it is rewritten into the Connect GET form:
//! `connect=v1`, `base64=1`, `compression` when the body was compressed,
//! `encoding` (`proto` or `json`) and the base64url `message`; no body, no
//! content type and no content encoding. Every other request passes through
//! untouched.
//!
//! Composes with [`PreconditionedTransport`](super::PreconditionedTransport) in
//! either order: both key on the RPC identity in the URI path, and a read
//! never carries `if-match`.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine as _;
use bytes::Bytes;
use connectrpc::ConnectError;
use connectrpc::client::{BoxFuture, ClientBody, ClientTransport, full_body};
use http::header::{CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE};
use http::{Method, Request, Response, Uri};
use http_body_util::BodyExt as _;

/// Hard ceiling on a GET URL, whatever the policy allows. Matches the
/// browser transport's limit.
pub const MAX_CONNECT_GET_URL_BYTES: usize = 4096;

/// What the generated policy grants one method as a GET read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadPolicy {
    max_url_bytes: usize,
}

impl ReadPolicy {
    /// The longest URL, in bytes, the method may be sent with as a GET. A
    /// call whose encoded URL is longer stays a POST.
    #[must_use]
    pub fn max_url_bytes(self) -> usize {
        self.max_url_bytes.min(MAX_CONNECT_GET_URL_BYTES)
    }
}

/// Parse `connect-method-policy.json` into an RPC-identity -> read-policy
/// index holding only the methods eligible for GET: unary, `NO_SIDE_EFFECTS`,
/// non-sensitive, with a positive URL budget.
///
/// Fails CLOSED to an empty map on a malformed document or a duplicated or
/// malformed entry, like [`index_generated_policy`](super::index_generated_policy):
/// a policy that cannot be trusted in full grants nothing. A method whose
/// sensitivity or URL budget is absent or unreadable is simply not eligible.
#[must_use]
pub fn index_read_policy(json: &str) -> HashMap<String, ReadPolicy> {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(json) else {
        return HashMap::new();
    };
    let Some(1 | 2) = doc.get("schemaVersion").and_then(serde_json::Value::as_u64) else {
        return HashMap::new();
    };
    let Some(methods) = doc.get("methods").and_then(serde_json::Value::as_array) else {
        return HashMap::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut index = HashMap::new();
    for entry in methods {
        let (Some(rpc), Some(procedure), Some(idempotency)) = (
            entry.get("rpc").and_then(serde_json::Value::as_str),
            entry.get("procedure").and_then(serde_json::Value::as_str),
            entry.get("idempotency").and_then(serde_json::Value::as_str),
        ) else {
            return HashMap::new();
        };
        if !matches!(procedure, "unary" | "streaming")
            || !rpc.starts_with('/')
            || !seen.insert(rpc.to_owned())
        {
            return HashMap::new();
        }
        let max_url_bytes = entry
            .get("maxEncodedUrlBytes")
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0);
        let non_sensitive =
            entry.get("sensitivity").and_then(serde_json::Value::as_str) == Some("NON_SENSITIVE");
        if procedure == "unary"
            && idempotency == "NO_SIDE_EFFECTS"
            && non_sensitive
            && max_url_bytes > 0
        {
            index.insert(rpc.to_owned(), ReadPolicy { max_url_bytes });
        }
    }
    index
}

/// A [`ClientTransport`] that sends a policy-eligible unary read as a Connect
/// GET.
///
/// Enabled by wrapping the transport when the client is constructed; there is
/// no per-call switch. A request that is not a unary Connect POST for an
/// eligible method, or whose encoded URL would exceed the method's budget, is
/// forwarded unchanged.
///
/// The inner transport's error type must be able to carry a [`ConnectError`]
/// (the stock HTTP clients' is one), so a request body that cannot be read
/// surfaces as a Connect error rather than being sent truncated.
#[derive(Clone)]
pub struct GetReadTransport<T> {
    inner: T,
    policy: Arc<HashMap<String, ReadPolicy>>,
}

impl<T> GetReadTransport<T> {
    /// Wrap `inner`, sending reads per `policy` (see [`index_read_policy`]).
    #[must_use]
    pub fn new(inner: T, policy: HashMap<String, ReadPolicy>) -> Self {
        Self {
            inner,
            policy: Arc::new(policy),
        }
    }
}

/// The wire `encoding` a unary Connect content type names, or `None` for
/// anything else (streaming `application/connect+...`, gRPC, unknown).
fn unary_encoding(content_type: &str) -> Option<&'static str> {
    let essence = content_type.split(';').next().unwrap_or("").trim();
    match essence.to_ascii_lowercase().as_str() {
        "application/proto" => Some("proto"),
        "application/json" => Some("json"),
        _ => None,
    }
}

fn get_uri(uri: &Uri, encoding: &str, compression: Option<&str>, body: &[u8]) -> Option<Uri> {
    let message = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(body);
    let mut query = String::from("connect=v1&base64=1");
    if let Some(compression) = compression {
        query.push_str("&compression=");
        query.push_str(compression);
    }
    query.push_str("&encoding=");
    query.push_str(encoding);
    query.push_str("&message=");
    query.push_str(&message);

    let mut parts = uri.clone().into_parts();
    let path = parts.path_and_query.as_ref().map_or("/", |pq| pq.path());
    parts.path_and_query = Some(format!("{path}?{query}").parse().ok()?);
    Uri::from_parts(parts).ok()
}

impl<T> ClientTransport for GetReadTransport<T>
where
    T: ClientTransport,
    T::Error: From<ConnectError>,
{
    type ResponseBody = T::ResponseBody;
    type Error = T::Error;

    fn send(
        &self,
        request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<Self::ResponseBody>, Self::Error>> {
        let policy = (request.method() == Method::POST)
            .then(|| self.policy.get(request.uri().path()).copied())
            .flatten();
        let encoding = request
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(unary_encoding);
        let (Some(policy), Some(encoding)) = (policy, encoding) else {
            return self.inner.send(request);
        };

        let inner = self.inner.clone();
        Box::pin(async move {
            let (mut parts, body) = request.into_parts();
            let payload = body.collect().await.map_err(T::Error::from)?.to_bytes();
            let compression = parts
                .headers
                .get(CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok())
                .filter(|v| !v.eq_ignore_ascii_case("identity"))
                .map(str::to_owned);

            let uri = get_uri(&parts.uri, encoding, compression.as_deref(), &payload)
                .filter(|uri| uri.to_string().len() <= policy.max_url_bytes());
            let Some(uri) = uri else {
                // Over budget: the original POST, byte for byte.
                return inner
                    .send(Request::from_parts(parts, full_body(payload)))
                    .await;
            };

            parts.method = Method::GET;
            parts.uri = uri;
            parts.headers.remove(CONTENT_TYPE);
            parts.headers.remove(CONTENT_ENCODING);
            parts.headers.remove(CONTENT_LENGTH);
            inner
                .send(Request::from_parts(parts, full_body(Bytes::new())))
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::{PreconditionedTransport, index_generated_policy};
    use http_body_util::Empty;
    use std::sync::Mutex;

    const POLICY: &str = r#"{
        "schemaVersion": 2,
        "methods": [
            {"rpc": "/pkg.v1.Svc/Get", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
             "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 512},
            {"rpc": "/pkg.v1.Svc/Secret", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
             "sensitivity": "SENSITIVE", "maxEncodedUrlBytes": 512},
            {"rpc": "/pkg.v1.Svc/Update", "procedure": "unary", "idempotency": "IDEMPOTENT",
             "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 512},
            {"rpc": "/pkg.v1.Svc/Watch", "procedure": "streaming", "idempotency": "NO_SIDE_EFFECTS",
             "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 512}
        ]
    }"#;

    #[derive(Clone, Default)]
    struct Recorder {
        seen: Arc<Mutex<Vec<(Method, Uri, http::HeaderMap, Bytes)>>>,
    }

    impl ClientTransport for Recorder {
        type ResponseBody = Empty<Bytes>;
        type Error = ConnectError;

        fn send(
            &self,
            request: Request<ClientBody>,
        ) -> BoxFuture<'static, Result<Response<Self::ResponseBody>, Self::Error>> {
            let seen = self.seen.clone();
            Box::pin(async move {
                let (parts, body) = request.into_parts();
                let bytes = body.collect().await.unwrap().to_bytes();
                seen.lock()
                    .unwrap()
                    .push((parts.method, parts.uri, parts.headers, bytes));
                Ok(Response::new(Empty::new()))
            })
        }
    }

    fn post(path: &str, content_type: &str, body: &'static [u8]) -> Request<ClientBody> {
        Request::builder()
            .method(Method::POST)
            .uri(format!("https://svc{path}"))
            .header(CONTENT_TYPE, content_type)
            .header("connect-protocol-version", "1")
            .header(CONTENT_LENGTH, body.len())
            .body(full_body(Bytes::from_static(body)))
            .unwrap()
    }

    fn wrapped(recorder: &Recorder) -> GetReadTransport<Recorder> {
        GetReadTransport::new(recorder.clone(), index_read_policy(POLICY))
    }

    fn last(recorder: &Recorder) -> (Method, Uri, http::HeaderMap, Bytes) {
        recorder.seen.lock().unwrap().last().cloned().unwrap()
    }

    #[test]
    fn indexes_only_eligible_methods() {
        let index = index_read_policy(POLICY);
        assert_eq!(index.len(), 1);
        assert_eq!(index["/pkg.v1.Svc/Get"].max_url_bytes(), 512);
    }

    #[test]
    fn url_budget_is_capped_at_the_hard_ceiling() {
        let json = r#"{"schemaVersion": 1, "methods": [
            {"rpc": "/p.S/G", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
             "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 99999}
        ]}"#;
        assert_eq!(
            index_read_policy(json)["/p.S/G"].max_url_bytes(),
            MAX_CONNECT_GET_URL_BYTES
        );
    }

    #[test]
    fn missing_sensitivity_or_budget_is_not_eligible() {
        let json = r#"{"schemaVersion": 1, "methods": [
            {"rpc": "/p.S/A", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
             "maxEncodedUrlBytes": 100},
            {"rpc": "/p.S/B", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
             "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 0},
            {"rpc": "/p.S/C", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
             "sensitivity": "NON_SENSITIVE"}
        ]}"#;
        assert!(index_read_policy(json).is_empty());
    }

    #[test]
    fn fails_closed_on_malformed_or_duplicate_policy() {
        assert!(index_read_policy("not json").is_empty());
        assert!(index_read_policy(r#"{"schemaVersion": 3, "methods": []}"#).is_empty());
        let dup = r#"{"schemaVersion": 1, "methods": [
            {"rpc": "/p.S/G", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
             "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 100},
            {"rpc": "/p.S/G", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
             "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 100}
        ]}"#;
        assert!(index_read_policy(dup).is_empty());
    }

    #[tokio::test]
    async fn eligible_read_becomes_a_connect_get() {
        let recorder = Recorder::default();
        wrapped(&recorder)
            .send(post("/pkg.v1.Svc/Get", "application/proto", b"\x0a\x03abc"))
            .await
            .unwrap();
        let (method, uri, headers, body) = last(&recorder);
        assert_eq!(method, Method::GET);
        assert_eq!(uri.path(), "/pkg.v1.Svc/Get");
        assert_eq!(
            uri.query().unwrap(),
            "connect=v1&base64=1&encoding=proto&message=CgNhYmM"
        );
        assert!(body.is_empty());
        assert!(headers.get(CONTENT_TYPE).is_none());
        assert!(headers.get(CONTENT_ENCODING).is_none());
        assert!(headers.get(CONTENT_LENGTH).is_none());
        assert_eq!(headers.get("connect-protocol-version").unwrap(), "1");
    }

    #[tokio::test]
    async fn json_message_is_base64url_encoded() {
        let recorder = Recorder::default();
        wrapped(&recorder)
            .send(post(
                "/pkg.v1.Svc/Get",
                "application/json; charset=utf-8",
                br#"{"id":"?>"}"#,
            ))
            .await
            .unwrap();
        let query = last(&recorder).1.query().unwrap().to_owned();
        let message = query.rsplit("message=").next().unwrap();
        assert!(query.contains("encoding=json"));
        assert!(!message.contains(['+', '/', '=']));
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(message)
            .unwrap();
        assert_eq!(decoded, br#"{"id":"?>"}"#);
    }

    #[tokio::test]
    async fn compressed_body_names_its_compression() {
        let recorder = Recorder::default();
        let mut request = post("/pkg.v1.Svc/Get", "application/proto", b"\x1f\x8b");
        request
            .headers_mut()
            .insert(CONTENT_ENCODING, "gzip".parse().unwrap());
        wrapped(&recorder).send(request).await.unwrap();
        let (method, uri, headers, _) = last(&recorder);
        assert_eq!(method, Method::GET);
        assert_eq!(
            uri.query().unwrap(),
            "connect=v1&base64=1&compression=gzip&encoding=proto&message=H4s"
        );
        assert!(headers.get(CONTENT_ENCODING).is_none());
    }

    #[tokio::test]
    async fn query_of_the_original_uri_is_replaced_not_appended() {
        let recorder = Recorder::default();
        let mut request = post("/pkg.v1.Svc/Get", "application/proto", b"");
        *request.uri_mut() = "https://svc/pkg.v1.Svc/Get?stale=1".parse().unwrap();
        wrapped(&recorder).send(request).await.unwrap();
        assert!(!last(&recorder).1.query().unwrap().contains("stale"));
    }

    #[tokio::test]
    async fn non_eligible_methods_pass_through_unchanged() {
        for path in [
            "/pkg.v1.Svc/Update",
            "/pkg.v1.Svc/Secret",
            "/pkg.v1.Svc/Unknown",
        ] {
            let recorder = Recorder::default();
            wrapped(&recorder)
                .send(post(path, "application/proto", b"payload"))
                .await
                .unwrap();
            let (method, uri, headers, body) = last(&recorder);
            assert_eq!(method, Method::POST, "{path}");
            assert!(uri.query().is_none(), "{path}");
            assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/proto");
            assert_eq!(&body[..], b"payload");
        }
    }

    #[tokio::test]
    async fn streaming_is_never_sent_as_get() {
        // A streaming method declared NO_SIDE_EFFECTS is not indexed ...
        assert!(!index_read_policy(POLICY).contains_key("/pkg.v1.Svc/Watch"));
        let recorder = Recorder::default();
        let transport = wrapped(&recorder);
        transport
            .send(post("/pkg.v1.Svc/Watch", "application/connect+proto", b"x"))
            .await
            .unwrap();
        // ... and a streaming envelope to an eligible path is left alone too.
        transport
            .send(post("/pkg.v1.Svc/Get", "application/connect+proto", b"x"))
            .await
            .unwrap();
        for (method, uri, headers, body) in recorder.seen.lock().unwrap().iter() {
            assert_eq!(method, Method::POST);
            assert!(uri.query().is_none());
            assert_eq!(
                headers.get(CONTENT_TYPE).unwrap(),
                "application/connect+proto"
            );
            assert_eq!(&body[..], b"x");
        }
    }

    #[tokio::test]
    async fn a_url_over_budget_stays_a_post() {
        let recorder = Recorder::default();
        let body: &'static [u8] = Box::leak(vec![7u8; 1000].into_boxed_slice());
        wrapped(&recorder)
            .send(post("/pkg.v1.Svc/Get", "application/proto", body))
            .await
            .unwrap();
        let (method, uri, headers, sent) = last(&recorder);
        assert_eq!(method, Method::POST);
        assert!(uri.query().is_none());
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/proto");
        assert_eq!(&sent[..], body);
    }

    #[tokio::test]
    async fn read_carries_no_if_match_under_the_precondition_transport() {
        let policy = r#"{"schemaVersion": 2, "methods": [
            {"rpc": "/pkg.v1.Svc/Get", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS",
             "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 512},
            {"rpc": "/pkg.v1.Svc/Update", "procedure": "unary", "idempotency": "IDEMPOTENT",
             "sensitivity": "NON_SENSITIVE", "maxEncodedUrlBytes": 512}
        ]}"#;
        // Both nesting orders.
        for outer_get in [true, false] {
            let recorder = Recorder::default();
            let send = |request| {
                let recorder = recorder.clone();
                async move {
                    if outer_get {
                        GetReadTransport::new(
                            PreconditionedTransport::new(recorder, index_generated_policy(policy)),
                            index_read_policy(policy),
                        )
                        .send(request)
                        .await
                    } else {
                        PreconditionedTransport::new(
                            GetReadTransport::new(recorder, index_read_policy(policy)),
                            index_generated_policy(policy),
                        )
                        .send(request)
                        .await
                    }
                }
            };
            send(post("/pkg.v1.Svc/Get", "application/proto", b"a"))
                .await
                .unwrap();
            send(post("/pkg.v1.Svc/Update", "application/proto", b"a"))
                .await
                .unwrap();

            let seen = recorder.seen.lock().unwrap();
            assert_eq!(seen[0].0, Method::GET);
            assert!(seen[0].2.get("if-match").is_none());
            assert_eq!(seen[1].0, Method::POST);
            assert_eq!(seen[1].2.get("if-match").unwrap(), "*");
        }
    }
}
