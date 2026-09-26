// SPDX-License-Identifier: MIT
//! Default `If-Match` precondition, attached from the transport.
//!
//! A Connect handler enforcing [`crate::connect::check_if_match`] rejects a
//! write missing the header — but nothing built into a Rust client ever
//! attached one, so every call site had to remember it by hand. This module
//! is the Rust half of `api-bones-connect-ts/src/precondition.ts`: the same
//! decision, read from the same generated policy artifact
//! (`connect-method-policy.json`), so a Rust service, CLI, or test harness
//! gets the same default a browser SDK gets.
//!
//! [`retry_eligibility`](crate::connect::retry_eligibility) notes this
//! artifact "has no Rust equivalent yet" for retry eligibility; this module
//! is that equivalent, scoped to the one field precondition needs.

use std::collections::HashMap;
use std::sync::Arc;

use connectrpc::client::{BoxFuture, ClientBody, ClientTransport};
use http::{HeaderValue, Request, Response};

/// The wildcard, last-write-wins precondition every hand-written call site
/// used before this existed.
pub const IF_MATCH_ANY: &str = "*";

/// The declared idempotency contract for one RPC, the one field this module
/// needs out of the generated policy document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Idempotency {
    NoSideEffects,
    Idempotent,
    Unspecified,
}

impl Idempotency {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "NO_SIDE_EFFECTS" => Some(Self::NoSideEffects),
            "IDEMPOTENT" => Some(Self::Idempotent),
            "UNSPECIFIED" => Some(Self::Unspecified),
            _ => None,
        }
    }

    /// Whether a method with this contract mutates state and is therefore
    /// preconditioned by default. Mirrors
    /// `isPreconditionedMethod` in `precondition.ts`: only a declared
    /// `NO_SIDE_EFFECTS` read is exempt.
    #[must_use]
    pub fn is_preconditioned(self) -> bool {
        !matches!(self, Self::NoSideEffects)
    }
}

/// Parse `connect-method-policy.json` into an RPC-identity → idempotency
/// index.
///
/// Fails CLOSED to an empty map on any malformed or duplicated entry, exactly
/// like `indexGeneratedPolicy` on the TypeScript side: a policy that cannot be
/// trusted in full guards nothing, rather than guarding the subset that
/// happened to parse. An empty map (including "no such file") disables the
/// default precondition entirely — a client that ships with no policy behaves
/// exactly as it did before this existed.
#[must_use]
pub fn index_generated_policy(json: &str) -> HashMap<String, Idempotency> {
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(json) else {
        return HashMap::new();
    };
    let mut index = HashMap::new();
    let Some(1) = doc.get("schemaVersion").and_then(serde_json::Value::as_u64) else {
        return HashMap::new();
    };
    let Some(methods) = doc.get("methods").and_then(serde_json::Value::as_array) else {
        return HashMap::new();
    };
    for entry in methods {
        let (Some(rpc), Some(procedure), Some(idempotency)) = (
            entry.get("rpc").and_then(serde_json::Value::as_str),
            entry.get("procedure").and_then(serde_json::Value::as_str),
            entry
                .get("idempotency")
                .and_then(serde_json::Value::as_str)
                .and_then(Idempotency::parse),
        ) else {
            return HashMap::new();
        };
        if procedure != "unary" || !rpc.starts_with('/') || index.contains_key(rpc) {
            return HashMap::new();
        }
        index.insert(rpc.to_owned(), idempotency);
    }
    index
}

/// A [`ClientTransport`] that attaches `if-match: *` to a mutating call the
/// caller named no precondition for.
///
/// Wraps any transport rather than replacing it — same shape as `retry.ts`'s
/// interceptor, but built at this layer because `connectrpc`'s generated
/// clients expose no client-side interceptor chain (see
/// [`connectrpc::client::ClientTransport::send`]): the request's URI path
/// already carries the RPC identity (`/pkg.Service/Method`), which is all the
/// policy lookup needs.
///
/// A caller-supplied `if-match` (a real `ETag`, for conflict detection) is
/// never overridden.
#[derive(Clone)]
pub struct PreconditionedTransport<T> {
    inner: T,
    policy: Arc<HashMap<String, Idempotency>>,
}

impl<T> PreconditionedTransport<T> {
    /// Wrap `inner`, guarding calls per `policy` (see
    /// [`index_generated_policy`]).
    #[must_use]
    pub fn new(inner: T, policy: HashMap<String, Idempotency>) -> Self {
        Self {
            inner,
            policy: Arc::new(policy),
        }
    }
}

impl<T: ClientTransport> ClientTransport for PreconditionedTransport<T> {
    type ResponseBody = T::ResponseBody;
    type Error = T::Error;

    fn send(
        &self,
        mut request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<Self::ResponseBody>, Self::Error>> {
        let preconditioned = self
            .policy
            .get(request.uri().path())
            .is_some_and(|idempotency| idempotency.is_preconditioned());
        if preconditioned && !request.headers().contains_key("if-match") {
            request
                .headers_mut()
                .insert("if-match", HeaderValue::from_static(IF_MATCH_ANY));
        }
        self.inner.send(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use connectrpc::ConnectError;
    use http_body_util::Empty;
    use std::sync::Mutex;

    fn policy_json() -> &'static str {
        r#"{
            "schemaVersion": 1,
            "methods": [
                {"rpc": "/pkg.v1.Svc/Update", "procedure": "unary", "idempotency": "IDEMPOTENT"},
                {"rpc": "/pkg.v1.Svc/Get", "procedure": "unary", "idempotency": "NO_SIDE_EFFECTS"}
            ]
        }"#
    }

    #[test]
    fn indexes_a_well_formed_policy() {
        let index = index_generated_policy(policy_json());
        assert_eq!(index.len(), 2);
        assert!(index["/pkg.v1.Svc/Update"].is_preconditioned());
        assert!(!index["/pkg.v1.Svc/Get"].is_preconditioned());
    }

    #[test]
    fn fails_closed_on_malformed_or_duplicate_entries() {
        assert!(index_generated_policy("not json").is_empty());
        assert!(index_generated_policy(r#"{"schemaVersion": 2, "methods": []}"#).is_empty());
        let dup = r#"{"schemaVersion": 1, "methods": [
            {"rpc": "/pkg.v1.Svc/Update", "procedure": "unary", "idempotency": "IDEMPOTENT"},
            {"rpc": "/pkg.v1.Svc/Update", "procedure": "unary", "idempotency": "IDEMPOTENT"}
        ]}"#;
        assert!(index_generated_policy(dup).is_empty());
    }

    #[derive(Clone)]
    struct RecordingTransport {
        seen_headers: Arc<Mutex<Vec<http::HeaderMap>>>,
    }

    impl ClientTransport for RecordingTransport {
        type ResponseBody = Empty<bytes::Bytes>;
        type Error = ConnectError;

        fn send(
            &self,
            request: Request<ClientBody>,
        ) -> BoxFuture<'static, Result<Response<Self::ResponseBody>, Self::Error>> {
            self.seen_headers
                .lock()
                .unwrap()
                .push(request.headers().clone());
            Box::pin(async { Ok(Response::new(Empty::new())) })
        }
    }

    fn request(path: &str, if_match: Option<&str>) -> Request<ClientBody> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("https://svc{path}"));
        if let Some(value) = if_match {
            builder = builder.header("if-match", value);
        }
        builder
            .body(connectrpc::client::full_body(bytes::Bytes::new()))
            .unwrap()
    }

    #[tokio::test]
    async fn attaches_if_match_any_to_a_mutating_call_by_default() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let transport = PreconditionedTransport::new(
            RecordingTransport {
                seen_headers: seen.clone(),
            },
            index_generated_policy(policy_json()),
        );
        transport
            .send(request("/pkg.v1.Svc/Update", None))
            .await
            .unwrap();
        let headers = seen.lock().unwrap();
        assert_eq!(headers[0].get("if-match").unwrap(), IF_MATCH_ANY);
    }

    #[tokio::test]
    async fn sends_no_if_match_on_a_no_side_effects_read() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let transport = PreconditionedTransport::new(
            RecordingTransport {
                seen_headers: seen.clone(),
            },
            index_generated_policy(policy_json()),
        );
        transport
            .send(request("/pkg.v1.Svc/Get", None))
            .await
            .unwrap();
        assert!(seen.lock().unwrap()[0].get("if-match").is_none());
    }

    #[tokio::test]
    async fn never_overrides_a_caller_supplied_precondition() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let transport = PreconditionedTransport::new(
            RecordingTransport {
                seen_headers: seen.clone(),
            },
            index_generated_policy(policy_json()),
        );
        transport
            .send(request("/pkg.v1.Svc/Update", Some("\"real-etag\"")))
            .await
            .unwrap();
        assert_eq!(
            seen.lock().unwrap()[0].get("if-match").unwrap(),
            "\"real-etag\""
        );
    }

    #[tokio::test]
    async fn leaves_an_unannotated_method_untouched() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let transport = PreconditionedTransport::new(
            RecordingTransport {
                seen_headers: seen.clone(),
            },
            HashMap::new(),
        );
        transport
            .send(request("/pkg.v1.Svc/Unknown", None))
            .await
            .unwrap();
        assert!(seen.lock().unwrap()[0].get("if-match").is_none());
    }
}
