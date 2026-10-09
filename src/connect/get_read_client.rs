// SPDX-License-Identifier: MIT
//! Connect GET for policy-eligible reads, chosen from the transport.
//!
//! A generated Connect client always sends a unary call as a POST. A read the
//! generated policy grants as a GET may instead travel as a Connect GET, which
//! a cache or a GET-only lane can serve. This module is the Rust half of the
//! browser transport's GET selection in `api-bones-connect-ts`: the same
//! policy reader and the same decision, from the same generated artifact
//! (`connect-method-policy.json`), taken where the client is constructed and
//! never per call.
//!
//! Two transports, each wrapping the transport the client already uses:
//!
//! - [`GetReadTransport`] sends a **credentialed** read as a Connect GET to the
//!   same URL path. A method qualifies when its policy is unary,
//!   `NO_SIDE_EFFECTS`, `NON_SENSITIVE`, privately cacheable
//!   (`browserCache.scope == PRIVATE`, at most 300 seconds), has a URL budget,
//!   and is not a public read. The request keeps its headers; only the body,
//!   content type and content encoding move into the query. A call whose URL
//!   would exceed the method's budget stays a POST.
//! - [`PublicReadTransport`] sends a `publicRead` method to the product's
//!   public lane (`/public` ahead of the mount) as an **anonymous** GET: the
//!   request keeps only protocol headers, so no bearer, cookie, CSRF token or
//!   product header travels. The lane serves nothing else, so the URL budget
//!   does not apply.
//!
//! Every other request, including every streaming call, passes through
//! untouched. The message always travels as base64url in the Connect GET
//! query: `connect=v1`, `base64=1`, `compression` when the body was
//! compressed, `encoding` (`proto` or `json`), `message`.
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
use http::{HeaderMap, Method, Request, Response, Uri};
use http_body_util::BodyExt as _;

/// Hard ceiling on a GET URL, and on any method's declared budget. A policy
/// declaring more is rejected, not clamped.
pub const MAX_CONNECT_GET_URL_BYTES: usize = 4096;

/// Longest private cache lifetime a credentialed GET read may declare.
const MAX_PRIVATE_CACHE_TTL_SECONDS: u64 = 300;

/// Longest cache lifetime a public read may declare.
const MAX_PUBLIC_READ_AGE_SECONDS: u64 = 300;

/// Headers a public read keeps: protocol negotiation only, nothing that
/// identifies the caller.
const PUBLIC_LANE_HEADERS: [&str; 3] = [
    "connect-protocol-version",
    "connect-timeout-ms",
    "accept-encoding",
];

/// What the generated policy grants one method as a GET read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadPolicy {
    max_url_bytes: usize,
}

impl ReadPolicy {
    /// The longest URL, in bytes, the method may be sent with as a credentialed
    /// GET. A call whose encoded URL is longer stays a POST.
    #[must_use]
    pub fn max_url_bytes(self) -> usize {
        self.max_url_bytes
    }
}

struct PolicyMethod {
    rpc: String,
    unary: bool,
    idempotency: String,
    sensitivity: String,
    cache_scope: String,
    cache_max_age: u64,
    max_url_bytes: usize,
    public_read: bool,
}

fn non_negative_integer(value: &serde_json::Value) -> Option<u64> {
    value.as_u64()
}

fn public_read_is_well_formed(value: &serde_json::Value) -> bool {
    let (Some(age), Some(origins), Some(org)) = (
        value.get("maxAgeSeconds").and_then(non_negative_integer),
        value.get("origins").and_then(serde_json::Value::as_str),
        value.get("orgField").filter(|v| v.is_object()),
    ) else {
        return false;
    };
    let named = |key: &str| {
        org.get(key)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.is_empty())
    };
    (1..=MAX_PUBLIC_READ_AGE_SECONDS).contains(&age)
        && matches!(origins, "ANY" | "OWNER_CONFIRMED")
        && named("name")
        && named("jsonName")
        && org.get("number").and_then(non_negative_integer) >= Some(1)
}

fn parse_method(entry: &serde_json::Value) -> Option<PolicyMethod> {
    let text = |key: &str| entry.get(key).and_then(serde_json::Value::as_str);
    let cache = entry.get("browserCache").filter(|v| v.is_object())?;
    let rpc = text("rpc").filter(|rpc| rpc.starts_with('/'))?;
    let unary = match text("procedure")? {
        "unary" => true,
        "streaming" => false,
        _ => return None,
    };
    let cache_scope = cache
        .get("scope")
        .and_then(serde_json::Value::as_str)
        .filter(|scope| matches!(*scope, "PRIVATE" | "NO_STORE"))?;
    let cache_max_age = cache.get("maxAgeSeconds").and_then(non_negative_integer)?;
    let max_url_bytes = entry
        .get("maxEncodedUrlBytes")
        .and_then(non_negative_integer)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n <= MAX_CONNECT_GET_URL_BYTES)?;
    let public_read = match entry.get("publicRead") {
        None => false,
        Some(value) if public_read_is_well_formed(value) => true,
        Some(_) => return None,
    };
    // A value this reader does not know is ineligible, never an error.
    let idempotency = text("idempotency")?;
    let sensitivity = text("sensitivity")?;
    Some(PolicyMethod {
        rpc: rpc.to_owned(),
        unary,
        idempotency: idempotency.to_owned(),
        sensitivity: sensitivity.to_owned(),
        cache_scope: cache_scope.to_owned(),
        cache_max_age,
        max_url_bytes,
        public_read,
    })
}

/// Parse a policy document the way the TypeScript reader does: every entry
/// must be well formed and unique, or the whole document grants nothing.
fn parse_policy(json: &str) -> Vec<PolicyMethod> {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let versioned = matches!(
        doc.get("schemaVersion").and_then(non_negative_integer),
        Some(1 | 2)
    );
    let Some(methods) = doc.get("methods").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    if !versioned {
        return Vec::new();
    }
    let mut seen = std::collections::HashSet::new();
    let mut parsed = Vec::with_capacity(methods.len());
    for entry in methods {
        match parse_method(entry) {
            Some(method) if seen.insert(method.rpc.clone()) => parsed.push(method),
            _ => return Vec::new(),
        }
    }
    parsed
}

fn index_where(json: &str, grants: impl Fn(&PolicyMethod) -> bool) -> HashMap<String, ReadPolicy> {
    parse_policy(json)
        .into_iter()
        .filter(|method| method.unary && method.idempotency == "NO_SIDE_EFFECTS")
        .filter(|method| method.sensitivity == "NON_SENSITIVE" && method.max_url_bytes > 0)
        .filter(|method| grants(method))
        .map(|method| {
            let policy = ReadPolicy {
                max_url_bytes: method.max_url_bytes,
            };
            (method.rpc, policy)
        })
        .collect()
}

/// Index the methods that may be sent as a credentialed GET.
///
/// A method qualifies when it is unary, `NO_SIDE_EFFECTS`, `NON_SENSITIVE`,
/// privately cacheable for at most 300 seconds, has a positive URL budget, and
/// is not a public read.
///
/// Fails CLOSED to an empty map, like the TypeScript reader: a malformed or
/// duplicated entry anywhere in the document, an unknown schema version, a
/// budget above [`MAX_CONNECT_GET_URL_BYTES`] (rejected, not clamped) or a
/// malformed `publicRead` grants nothing at all. A value this reader does not
/// know makes only its own method ineligible.
#[must_use]
pub fn index_read_policy(json: &str) -> HashMap<String, ReadPolicy> {
    index_where(json, |method| {
        !method.public_read
            && method.cache_scope == "PRIVATE"
            && method.cache_max_age <= MAX_PRIVATE_CACHE_TTL_SECONDS
    })
}

/// Index the methods that may be sent to the public lane as an anonymous GET.
///
/// A method qualifies when it is unary, `NO_SIDE_EFFECTS`, `NON_SENSITIVE`,
/// `NO_STORE`, has a positive URL budget and a well-formed `publicRead`. Fails
/// closed exactly like [`index_read_policy`].
#[must_use]
pub fn index_public_read_policy(json: &str) -> HashMap<String, ReadPolicy> {
    index_where(json, |method| {
        method.public_read && method.cache_scope == "NO_STORE"
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lane {
    Credentialed,
    Public,
}

/// A unary Connect POST the policy grants as a GET, ready to be rewritten.
struct Plan {
    policy: ReadPolicy,
    encoding: &'static str,
    /// The path ahead of the RPC identity: the product's mount.
    mount: String,
    rpc: String,
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

/// Split a request path into the mount and the RPC identity
/// (`/pkg.Service/Method`, the last two segments).
fn split_rpc(path: &str) -> Option<(&str, &str)> {
    let method_at = path.rfind('/')?;
    let service_at = path[..method_at].rfind('/')?;
    Some((&path[..service_at], &path[service_at..]))
}

fn plan(request: &Request<ClientBody>, policy: &HashMap<String, ReadPolicy>) -> Option<Plan> {
    if request.method() != Method::POST {
        return None;
    }
    let (mount, rpc) = split_rpc(request.uri().path())?;
    let granted = *policy.get(rpc)?;
    let encoding = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(unary_encoding)?;
    Some(Plan {
        policy: granted,
        encoding,
        mount: mount.to_owned(),
        rpc: rpc.to_owned(),
    })
}

/// Whether `value` is a plain content-coding token, the only thing that may be
/// copied into the query.
fn is_coding_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The query for one GET, or `None` when the content encoding is not a plain
/// token.
fn get_query(plan: &Plan, content_encoding: Option<&str>, body: &[u8]) -> Option<String> {
    let compression = match content_encoding {
        None => None,
        Some(value) if value.eq_ignore_ascii_case("identity") => None,
        Some(value) if is_coding_token(value) => Some(value),
        Some(_) => return None,
    };
    let message = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(body);
    let mut query = String::from("connect=v1&base64=1");
    if let Some(compression) = compression {
        query.push_str("&compression=");
        query.push_str(compression);
    }
    query.push_str("&encoding=");
    query.push_str(plan.encoding);
    query.push_str("&message=");
    query.push_str(&message);
    Some(query)
}

fn with_path(uri: &Uri, path: &str, query: &str) -> Option<Uri> {
    let mut parts = uri.clone().into_parts();
    parts.path_and_query = Some(format!("{path}?{query}").parse().ok()?);
    Uri::from_parts(parts).ok()
}

fn anonymous_headers(headers: &HeaderMap) -> HeaderMap {
    let mut kept = HeaderMap::new();
    for name in PUBLIC_LANE_HEADERS {
        for value in headers.get_all(name) {
            kept.append(http::HeaderName::from_static(name), value.clone());
        }
    }
    kept
}

async fn send_planned<T>(
    inner: T,
    request: Request<ClientBody>,
    plan: Plan,
    lane: Lane,
) -> Result<Response<T::ResponseBody>, T::Error>
where
    T: ClientTransport,
    T::Error: From<ConnectError>,
{
    let (mut parts, body) = request.into_parts();
    let payload = body.collect().await.map_err(T::Error::from)?.to_bytes();
    let content_encoding = parts.headers.get(CONTENT_ENCODING).map(|v| v.to_str().ok());
    let query = match content_encoding {
        None => get_query(&plan, None, &payload),
        Some(Some(value)) => get_query(&plan, Some(value), &payload),
        Some(None) => None,
    };
    let path = match lane {
        Lane::Credentialed => parts.uri.path().to_owned(),
        Lane::Public => format!("/public{}{}", plan.mount, plan.rpc),
    };
    let uri = query
        .and_then(|query| with_path(&parts.uri, &path, &query))
        .filter(|uri| lane == Lane::Public || uri.to_string().len() <= plan.policy.max_url_bytes());
    let Some(uri) = uri else {
        // Not expressible as a GET within the grant: the original POST.
        return inner
            .send(Request::from_parts(parts, full_body(payload)))
            .await;
    };

    parts.method = Method::GET;
    parts.uri = uri;
    if lane == Lane::Public {
        parts.headers = anonymous_headers(&parts.headers);
    } else {
        parts.headers.remove(CONTENT_TYPE);
        parts.headers.remove(CONTENT_ENCODING);
        parts.headers.remove(CONTENT_LENGTH);
    }
    inner
        .send(Request::from_parts(parts, full_body(Bytes::new())))
        .await
}

fn dispatch<T>(
    inner: &T,
    policy: &HashMap<String, ReadPolicy>,
    lane: Lane,
    request: Request<ClientBody>,
) -> BoxFuture<'static, Result<Response<T::ResponseBody>, T::Error>>
where
    T: ClientTransport,
    T::Error: From<ConnectError>,
{
    match plan(&request, policy) {
        None => inner.send(request),
        Some(plan) => Box::pin(send_planned(inner.clone(), request, plan, lane)),
    }
}

/// A [`ClientTransport`] that sends a policy-eligible unary read as a
/// credentialed Connect GET.
///
/// Enabled by wrapping the transport when the client is constructed; there is
/// no per-call switch. A request that is not a unary Connect POST for an
/// eligible method, whose content encoding is not a plain token, or whose
/// encoded URL would exceed the method's budget, is forwarded unchanged.
///
/// The inner transport's error type must be able to carry a [`ConnectError`]
/// (the stock HTTP clients' is one), so a request body that cannot be read
/// surfaces as a Connect error and nothing is sent.
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
        dispatch(&self.inner, &self.policy, Lane::Credentialed, request)
    }
}

/// A [`ClientTransport`] that sends a `publicRead` method to the product's
/// public lane as an anonymous Connect GET.
///
/// The request is rewritten to `/public<mount>/pkg.Service/Method`, keeps only
/// protocol headers (`connect-protocol-version`, `connect-timeout-ms`,
/// `accept-encoding`), and carries its message in the query. Connection-level
/// identity belongs to the wrapped transport, so wrap one that holds none.
/// Anything the public policy does not grant passes through unchanged.
#[derive(Clone)]
pub struct PublicReadTransport<T> {
    inner: T,
    policy: Arc<HashMap<String, ReadPolicy>>,
}

impl<T> PublicReadTransport<T> {
    /// Wrap `inner`, sending public reads per `policy` (see
    /// [`index_public_read_policy`]).
    #[must_use]
    pub fn new(inner: T, policy: HashMap<String, ReadPolicy>) -> Self {
        Self {
            inner,
            policy: Arc::new(policy),
        }
    }
}

impl<T> ClientTransport for PublicReadTransport<T>
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
        dispatch(&self.inner, &self.policy, Lane::Public, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::{PreconditionedTransport, index_generated_policy};
    use http_body_util::{Empty, Limited};
    use std::sync::Mutex;

    fn entry(rpc: &str, idempotency: &str, sensitivity: &str, scope: &str, budget: u64) -> String {
        format!(
            r#"{{"rpc": "/pkg.v1.Svc/{rpc}", "procedure": "unary", "idempotency": "{idempotency}",
                "browserCache": {{"scope": "{scope}", "maxAgeSeconds": 60}},
                "sensitivity": "{sensitivity}", "maxEncodedUrlBytes": {budget}}}"#
        )
    }

    fn public_entry(rpc: &str) -> String {
        let base = entry(rpc, "NO_SIDE_EFFECTS", "NON_SENSITIVE", "NO_STORE", 512);
        let base = base.trim_end().trim_end_matches('}');
        format!(
            r#"{base}, "publicRead": {{"maxAgeSeconds": 60, "origins": "ANY",
                "orgField": {{"name": "org", "jsonName": "org", "number": 1}}}}}}"#
        )
    }

    fn document(entries: &[String]) -> String {
        format!(
            r#"{{"schemaVersion": 2, "methods": [{}]}}"#,
            entries.join(",")
        )
    }

    fn policy() -> String {
        document(&[
            entry("Get", "NO_SIDE_EFFECTS", "NON_SENSITIVE", "PRIVATE", 512),
            entry("Secret", "NO_SIDE_EFFECTS", "SENSITIVE", "PRIVATE", 512),
            entry("Update", "IDEMPOTENT", "NON_SENSITIVE", "PRIVATE", 512),
            entry("Cold", "NO_SIDE_EFFECTS", "NON_SENSITIVE", "NO_STORE", 512),
            public_entry("Open"),
        ])
    }

    type Sent = (Method, Uri, HeaderMap, Bytes);

    #[derive(Clone, Default)]
    struct Recorder {
        seen: Arc<Mutex<Vec<Sent>>>,
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

    fn post_at(url: &str, content_type: &str, body: Vec<u8>) -> Request<ClientBody> {
        Request::builder()
            .method(Method::POST)
            .uri(url)
            .header(CONTENT_TYPE, content_type)
            .header("connect-protocol-version", "1")
            .header(CONTENT_LENGTH, body.len())
            .body(full_body(Bytes::from(body)))
            .unwrap()
    }

    fn post(path: &str, content_type: &str, body: &[u8]) -> Request<ClientBody> {
        post_at(&format!("https://svc{path}"), content_type, body.to_vec())
    }

    fn wrapped(recorder: &Recorder) -> GetReadTransport<Recorder> {
        GetReadTransport::new(recorder.clone(), index_read_policy(&policy()))
    }

    fn last(recorder: &Recorder) -> Sent {
        recorder.seen.lock().unwrap().last().cloned().unwrap()
    }

    fn assert_untouched_post(recorder: &Recorder, content_type: &str, body: &[u8]) {
        let (method, uri, headers, sent) = last(recorder);
        assert_eq!(method, Method::POST);
        assert!(uri.query().is_none());
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), content_type);
        assert_eq!(&sent[..], body);
    }

    #[test]
    fn credentialed_index_holds_only_private_reads() {
        let index = index_read_policy(&policy());
        assert_eq!(index.keys().collect::<Vec<_>>(), ["/pkg.v1.Svc/Get"]);
        assert_eq!(index["/pkg.v1.Svc/Get"].max_url_bytes(), 512);
    }

    #[test]
    fn public_index_holds_only_public_reads() {
        let index = index_public_read_policy(&policy());
        assert_eq!(index.keys().collect::<Vec<_>>(), ["/pkg.v1.Svc/Open"]);
    }

    #[test]
    fn a_private_cache_lifetime_over_the_ceiling_is_not_eligible() {
        let long = entry("Get", "NO_SIDE_EFFECTS", "NON_SENSITIVE", "PRIVATE", 512)
            .replace("\"maxAgeSeconds\": 60", "\"maxAgeSeconds\": 301");
        assert!(index_read_policy(&document(&[long])).is_empty());
        let edge = entry("Get", "NO_SIDE_EFFECTS", "NON_SENSITIVE", "PRIVATE", 512)
            .replace("\"maxAgeSeconds\": 60", "\"maxAgeSeconds\": 300");
        assert_eq!(index_read_policy(&document(&[edge])).len(), 1);
    }

    #[test]
    fn future_enum_values_are_ineligible_but_do_not_fail_the_document() {
        let json = document(&[
            entry("Get", "NO_SIDE_EFFECTS", "NON_SENSITIVE", "PRIVATE", 512),
            entry("A", "FUTURE_VALUE", "NON_SENSITIVE", "PRIVATE", 512),
            entry("B", "NO_SIDE_EFFECTS", "FUTURE_VALUE", "PRIVATE", 512),
        ]);
        let index = index_read_policy(&json);
        assert_eq!(index.keys().collect::<Vec<_>>(), ["/pkg.v1.Svc/Get"]);
        let unknown_scope = entry("C", "NO_SIDE_EFFECTS", "NON_SENSITIVE", "FUTURE_VALUE", 512);
        assert!(index_read_policy(&document(&[unknown_scope])).is_empty());
    }

    #[test]
    fn zero_budget_is_not_eligible() {
        let json = document(&[entry(
            "Get",
            "NO_SIDE_EFFECTS",
            "NON_SENSITIVE",
            "PRIVATE",
            0,
        )]);
        assert!(index_read_policy(&json).is_empty());
    }

    #[test]
    fn any_malformed_entry_fails_the_whole_document_closed() {
        let good = entry("Get", "NO_SIDE_EFFECTS", "NON_SENSITIVE", "PRIVATE", 512);
        let broken: Vec<(&str, String)> = vec![
            (
                "no browserCache",
                good.replace("\"browserCache\"", "\"other\""),
            ),
            ("unknown scope", good.replace("PRIVATE", "SHARED")),
            (
                "non-string sensitivity",
                good.replace("\"NON_SENSITIVE\"", "7"),
            ),
            (
                "non-string idempotency",
                good.replace("\"NO_SIDE_EFFECTS\"", "null"),
            ),
            ("budget over ceiling", good.replace("512", "4097")),
            ("negative budget", good.replace("512", "-1")),
            ("fractional budget", good.replace("512", "1.5")),
            ("string budget", good.replace("512", "\"512\"")),
            (
                "negative age",
                good.replace("\"maxAgeSeconds\": 60", "\"maxAgeSeconds\": -5"),
            ),
            ("unknown procedure", good.replace("\"unary\"", "\"bidi\"")),
            (
                "rpc without slash",
                good.replace("\"/pkg.v1.Svc/Get\"", "\"pkg.v1.Svc/Get\""),
            ),
            (
                "malformed publicRead",
                good.replace(
                    "\"maxEncodedUrlBytes\"",
                    "\"publicRead\": {}, \"maxEncodedUrlBytes\"",
                ),
            ),
            (
                "null publicRead",
                good.replace(
                    "\"maxEncodedUrlBytes\"",
                    "\"publicRead\": null, \"maxEncodedUrlBytes\"",
                ),
            ),
        ];
        for (why, bad) in broken {
            // Beside a valid entry, before and after it.
            let json = document(&[good.replace("Get", "Other"), bad.clone()]);
            assert!(index_read_policy(&json).is_empty(), "{why}");
            assert!(index_public_read_policy(&json).is_empty(), "{why}");
            let json = document(&[bad, good.replace("Get", "Other")]);
            assert!(index_read_policy(&json).is_empty(), "{why} (first)");
        }
    }

    #[test]
    fn malformed_documents_grant_nothing() {
        let entries = entry("Get", "NO_SIDE_EFFECTS", "NON_SENSITIVE", "PRIVATE", 512);
        for json in [
            "not json".to_owned(),
            format!(r#"{{"schemaVersion": 3, "methods": [{entries}]}}"#),
            format!(r#"{{"schemaVersion": "2", "methods": [{entries}]}}"#),
            format!(r#"{{"methods": [{entries}]}}"#),
            r#"{"schemaVersion": 2}"#.to_owned(),
            r#"{"schemaVersion": 2, "methods": {}}"#.to_owned(),
            document(&[entries.clone(), entries]),
        ] {
            assert!(index_read_policy(&json).is_empty(), "{json}");
        }
    }

    #[test]
    fn a_malformed_public_read_field_grants_nothing() {
        let base = entry("Open", "NO_SIDE_EFFECTS", "NON_SENSITIVE", "NO_STORE", 512);
        let open = base.trim_end().trim_end_matches('}');
        for public in [
            r#"{"maxAgeSeconds": 0, "origins": "ANY", "orgField": {"name": "o", "jsonName": "o", "number": 1}}"#,
            r#"{"maxAgeSeconds": 301, "origins": "ANY", "orgField": {"name": "o", "jsonName": "o", "number": 1}}"#,
            r#"{"maxAgeSeconds": 5, "origins": "SOME", "orgField": {"name": "o", "jsonName": "o", "number": 1}}"#,
            r#"{"maxAgeSeconds": 5, "origins": "ANY", "orgField": {"name": "", "jsonName": "o", "number": 1}}"#,
            r#"{"maxAgeSeconds": 5, "origins": "ANY", "orgField": {"name": "o", "jsonName": "o", "number": 0}}"#,
            r#"{"maxAgeSeconds": 5, "origins": "ANY"}"#,
        ] {
            let json = document(&[format!(r#"{open}, "publicRead": {public}}}"#)]);
            assert!(index_public_read_policy(&json).is_empty(), "{public}");
        }
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
    async fn a_credentialed_get_keeps_the_callers_headers_and_mount() {
        let recorder = Recorder::default();
        let mut request = post("/mount/pkg.v1.Svc/Get", "application/proto", b"a");
        request
            .headers_mut()
            .insert("authorization", "Bearer t".parse().unwrap());
        wrapped(&recorder).send(request).await.unwrap();
        let (method, uri, headers, _) = last(&recorder);
        assert_eq!(method, Method::GET);
        assert_eq!(uri.path(), "/mount/pkg.v1.Svc/Get");
        assert_eq!(headers.get("authorization").unwrap(), "Bearer t");
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
    async fn identity_content_encoding_names_no_compression() {
        let recorder = Recorder::default();
        let mut request = post("/pkg.v1.Svc/Get", "application/proto", b"a");
        request
            .headers_mut()
            .insert(CONTENT_ENCODING, "identity".parse().unwrap());
        wrapped(&recorder).send(request).await.unwrap();
        assert!(!last(&recorder).1.query().unwrap().contains("compression"));
    }

    #[tokio::test]
    async fn a_content_encoding_that_is_not_a_token_stays_a_post() {
        for bad in ["gzip&x=1", "gz ip", "gzip, br", "gz\u{e9}", "a=b", "x#"] {
            let recorder = Recorder::default();
            let mut request = post("/pkg.v1.Svc/Get", "application/proto", b"payload");
            request.headers_mut().insert(
                CONTENT_ENCODING,
                http::HeaderValue::from_bytes(bad.as_bytes()).unwrap(),
            );
            wrapped(&recorder).send(request).await.unwrap();
            let (method, uri, headers, body) = last(&recorder);
            assert_eq!(method, Method::POST, "{bad}");
            assert!(uri.query().is_none(), "{bad}");
            assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "application/proto");
            assert_eq!(&body[..], b"payload");
        }
    }

    #[tokio::test]
    async fn query_of_the_original_uri_is_replaced_not_appended() {
        let recorder = Recorder::default();
        let request = post_at(
            "https://svc/pkg.v1.Svc/Get?stale=1",
            "application/proto",
            Vec::new(),
        );
        wrapped(&recorder).send(request).await.unwrap();
        assert!(!last(&recorder).1.query().unwrap().contains("stale"));
    }

    #[tokio::test]
    async fn methods_the_policy_does_not_grant_pass_through_unchanged() {
        // Mutating, sensitive, NO_STORE, public (never credentialed), unknown.
        for name in ["Update", "Secret", "Cold", "Open", "Unknown"] {
            let recorder = Recorder::default();
            wrapped(&recorder)
                .send(post(
                    &format!("/pkg.v1.Svc/{name}"),
                    "application/proto",
                    b"payload",
                ))
                .await
                .unwrap();
            assert_untouched_post(&recorder, "application/proto", b"payload");
        }
    }

    #[tokio::test]
    async fn a_request_that_is_not_a_post_passes_through() {
        for method in [Method::GET, Method::PUT, Method::DELETE] {
            let recorder = Recorder::default();
            let mut request = post("/pkg.v1.Svc/Get", "application/proto", b"x");
            *request.method_mut() = method.clone();
            wrapped(&recorder).send(request).await.unwrap();
            let (sent, uri, _, body) = last(&recorder);
            assert_eq!(sent, method);
            assert!(uri.query().is_none());
            assert_eq!(&body[..], b"x");
        }
    }

    #[tokio::test]
    async fn streaming_is_never_sent_as_get() {
        let json = format!(
            r#"{{"schemaVersion": 2, "methods": [{}]}}"#,
            entry("Watch", "NO_SIDE_EFFECTS", "NON_SENSITIVE", "PRIVATE", 512)
                .replace("\"unary\"", "\"streaming\"")
        );
        let index = index_read_policy(&json);
        assert!(index.is_empty());
        // A streaming envelope to an eligible path is left alone too.
        let recorder = Recorder::default();
        wrapped(&recorder)
            .send(post("/pkg.v1.Svc/Get", "application/connect+proto", b"x"))
            .await
            .unwrap();
        assert_untouched_post(&recorder, "application/connect+proto", b"x");
    }

    /// The length of the GET URL for a body of `n` bytes.
    fn url_len_for(n: usize) -> usize {
        "https://svc/pkg.v1.Svc/Get?connect=v1&base64=1&encoding=proto&message=".len()
            + (n * 4).div_ceil(3)
    }

    async fn sent_with_budget(budget: u64, body: &[u8]) -> Sent {
        let recorder = Recorder::default();
        let json = document(&[entry(
            "Get",
            "NO_SIDE_EFFECTS",
            "NON_SENSITIVE",
            "PRIVATE",
            budget,
        )]);
        GetReadTransport::new(recorder.clone(), index_read_policy(&json))
            .send(post("/pkg.v1.Svc/Get", "application/proto", body))
            .await
            .unwrap();
        last(&recorder)
    }

    #[tokio::test]
    async fn a_url_exactly_at_budget_is_a_get_and_one_byte_over_is_a_post() {
        let body = vec![7u8; 90];
        let len = u64::try_from(url_len_for(body.len())).unwrap();
        assert_eq!(sent_with_budget(len, &body).await.0, Method::GET);
        let over = sent_with_budget(len - 1, &body).await;
        assert_eq!(over.0, Method::POST);
        assert!(over.1.query().is_none());
        assert_eq!(over.2.get(CONTENT_TYPE).unwrap(), "application/proto");
        assert_eq!(&over.3[..], &body[..]);
    }

    #[tokio::test]
    async fn the_hard_ceiling_applies_at_4096() {
        // The largest body whose URL still fits in 4096 bytes.
        let mut n = 3000;
        while url_len_for(n + 1) <= 4096 {
            n += 1;
        }
        let (fits, over) = (vec![1u8; n], vec![1u8; n + 1]);
        assert!(url_len_for(n) <= 4096 && url_len_for(n + 1) > 4096);
        assert_eq!(sent_with_budget(4096, &fits).await.0, Method::GET);
        assert_eq!(sent_with_budget(4096, &over).await.0, Method::POST);
    }

    #[tokio::test]
    async fn an_unreadable_body_is_a_connect_error_and_nothing_is_sent() {
        let recorder = Recorder::default();
        let body = Limited::new(full_body(Bytes::from_static(b"abcdef")), 1)
            .map_err(|e| ConnectError::internal(e.to_string()))
            .boxed();
        let request = Request::builder()
            .method(Method::POST)
            .uri("https://svc/pkg.v1.Svc/Get")
            .header(CONTENT_TYPE, "application/proto")
            .body(body)
            .unwrap();
        let err = wrapped(&recorder).send(request).await.unwrap_err();
        assert_eq!(err.code, connectrpc::ErrorCode::Internal);
        assert!(recorder.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_public_read_goes_to_the_lane_anonymously() {
        let recorder = Recorder::default();
        let transport =
            PublicReadTransport::new(recorder.clone(), index_public_read_policy(&policy()));
        let mut request = post(
            "/itinerwiz/pkg.v1.Svc/Open",
            "application/proto",
            b"\x0a\x03abc",
        );
        for (name, value) in [
            ("authorization", "Bearer t"),
            ("cookie", "sid=1"),
            ("x-csrf-token", "c"),
            ("x-product", "p"),
            ("connect-timeout-ms", "1500"),
            ("accept-encoding", "gzip"),
        ] {
            request.headers_mut().insert(name, value.parse().unwrap());
        }
        transport.send(request).await.unwrap();
        let (method, uri, headers, body) = last(&recorder);
        assert_eq!(method, Method::GET);
        assert_eq!(uri.path(), "/public/itinerwiz/pkg.v1.Svc/Open");
        assert_eq!(
            uri.query().unwrap(),
            "connect=v1&base64=1&encoding=proto&message=CgNhYmM"
        );
        assert!(body.is_empty());
        let mut names: Vec<_> = headers.keys().map(|n| n.as_str().to_owned()).collect();
        names.sort();
        assert_eq!(
            names,
            [
                "accept-encoding",
                "connect-protocol-version",
                "connect-timeout-ms"
            ]
        );
    }

    #[tokio::test]
    async fn a_public_read_ignores_the_url_budget_and_other_methods_pass_through() {
        let recorder = Recorder::default();
        let transport =
            PublicReadTransport::new(recorder.clone(), index_public_read_policy(&policy()));
        transport
            .send(post("/pkg.v1.Svc/Open", "application/proto", &[9u8; 2000]))
            .await
            .unwrap();
        assert_eq!(last(&recorder).0, Method::GET);
        for name in ["Get", "Update", "Unknown"] {
            let mut request = post(&format!("/pkg.v1.Svc/{name}"), "application/proto", b"p");
            request
                .headers_mut()
                .insert("authorization", "Bearer t".parse().unwrap());
            transport.send(request).await.unwrap();
            let (method, uri, headers, _) = last(&recorder);
            assert_eq!(method, Method::POST, "{name}");
            assert!(uri.query().is_none());
            assert!(headers.get("authorization").is_some());
        }
    }

    #[tokio::test]
    async fn a_public_read_with_a_bad_compression_token_stays_the_original_post() {
        let recorder = Recorder::default();
        let transport =
            PublicReadTransport::new(recorder.clone(), index_public_read_policy(&policy()));
        let mut request = post("/pkg.v1.Svc/Open", "application/proto", b"p");
        request
            .headers_mut()
            .insert(CONTENT_ENCODING, "gz&ip".parse().unwrap());
        transport.send(request).await.unwrap();
        assert_eq!(last(&recorder).0, Method::POST);
    }

    #[tokio::test]
    async fn read_carries_no_if_match_under_the_precondition_transport() {
        let json = policy();
        // Both nesting orders; the policy holds only unary methods.
        for get_outside in [true, false] {
            let recorder = Recorder::default();
            let send = |request| {
                let recorder = recorder.clone();
                let json = json.clone();
                async move {
                    if get_outside {
                        GetReadTransport::new(
                            PreconditionedTransport::new(recorder, index_generated_policy(&json)),
                            index_read_policy(&json),
                        )
                        .send(request)
                        .await
                    } else {
                        PreconditionedTransport::new(
                            GetReadTransport::new(recorder, index_read_policy(&json)),
                            index_generated_policy(&json),
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
            assert_eq!(seen[0].0, Method::GET, "get_outside={get_outside}");
            assert!(seen[0].2.get("if-match").is_none());
            assert_eq!(seen[1].0, Method::POST);
            assert_eq!(seen[1].2.get("if-match").unwrap(), "*");
        }
    }
}
