// SPDX-License-Identifier: MIT
//! Per-call credential scoping for Connect RPC clients.
//!
//! A client is normally built around a *process-wide* credential: an injector
//! (`Fn() -> String`) the transport calls once per request. That is the right
//! shape when the credential is a property of the process — a service
//! presenting its own rotating token — and the wrong shape when the credential
//! varies per call, because the injector receives no request context and
//! cannot know which call it is answering for. A caller in that position has
//! to put the credential somewhere the closure can reach and hope no
//! concurrent call observes it, which is a data race the type system permits.
//!
//! [`ScopedClient`] closes that hole. It binds one credential to one call and
//! **borrows** the client that owns the transport, so the scoped call reuses
//! the same TLS configuration and the same connection pool — no extra
//! handshake, no per-identity pool.
//!
//! ```rust,ignore
//! use api_bones::connect::{CallCredential, ScopeCall, ScopedClient};
//!
//! // A generated client keeps its own process-wide credential…
//! struct Client { transport: HttpClient, config: ClientConfig, auth: CallCredential }
//! impl ScopeCall for Client {}
//!
//! // …and gains a per-call handle over the very same transport.
//! let scoped = client.with_credential(token_for_this_caller);
//! let options = scoped.call_options();
//! ```

use std::sync::Arc;

use connectrpc::client::CallOptions;

/// Supplies the credential a client presents **on its own behalf**, re-read
/// once per request so a rotating token stays current without rebuilding the
/// client.
///
/// This is the correct mechanism for an identity that belongs to the process.
/// It is not the mechanism for an identity that belongs to a call — use
/// [`ScopedClient`] for that.
pub type TokenInjector = Arc<dyn Fn() -> String + Send + Sync>;

/// The process-wide credential a client presents when a call is not scoped to
/// one of its own.
///
/// Cheap to clone: the injector is shared, never re-invoked on clone.
#[derive(Clone, Default)]
pub struct CallCredential {
    injector: Option<TokenInjector>,
}

impl CallCredential {
    /// A client that sends no bearer credential.
    ///
    /// Used by transports whose authority comes from the connection itself
    /// (mutual TLS) rather than from a bearer token.
    #[must_use]
    pub fn none() -> Self {
        Self { injector: None }
    }

    /// Read the credential from `f` on every request.
    ///
    /// `f` runs per request so short-lived tokens stay current. It takes no
    /// request context by design: what it returns is the *process's* identity,
    /// the same for every in-flight call.
    #[must_use]
    pub fn from_token_injector<F>(f: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        Self {
            injector: Some(Arc::new(f)),
        }
    }

    /// Read the credential from an already-shared injector.
    #[must_use]
    pub fn shared(injector: TokenInjector) -> Self {
        Self {
            injector: Some(injector),
        }
    }

    /// A fixed credential for the life of the client.
    #[must_use]
    pub fn bearer(token: impl Into<String>) -> Self {
        let token = token.into();
        Self::from_token_injector(move || token.clone())
    }

    /// Whether a credential is configured at all.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.injector.is_some()
    }

    /// The credential for a call made on the process's own identity.
    #[must_use]
    pub fn current(&self) -> Option<String> {
        self.injector.as_ref().map(|f| f())
    }

    /// Call options for a call made on the process's own identity.
    #[must_use]
    pub fn call_options(&self) -> CallOptions {
        call_options(self.current().as_deref())
    }
}

impl std::fmt::Debug for CallCredential {
    /// Never prints the credential — only whether one is configured.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallCredential")
            .field("configured", &self.is_configured())
            .finish()
    }
}

/// One credential bound to one call, over the transport its parent client owns.
///
/// The handle borrows the client rather than rebuilding it, so the scoped call
/// travels the same TLS configuration and the same connection pool. The
/// credential lives in the handle, so two scoped handles taken from one client
/// cannot observe each other's credential however their calls interleave.
pub struct ScopedClient<'a, C> {
    client: &'a C,
    credential: Arc<str>,
}

impl<'a, C> ScopedClient<'a, C> {
    /// Bind `credential` to calls made through this handle.
    #[must_use]
    pub fn new(client: &'a C, credential: impl Into<String>) -> Self {
        Self {
            client,
            credential: Arc::from(credential.into()),
        }
    }

    /// The client this handle borrows — the same instance, holding the same
    /// transport, that produced the handle.
    #[must_use]
    pub fn client(&self) -> &'a C {
        self.client
    }

    /// The credential bound to this handle.
    #[must_use]
    pub fn credential(&self) -> &str {
        &self.credential
    }

    /// Call options carrying this handle's credential.
    #[must_use]
    pub fn call_options(&self) -> CallOptions {
        call_options(Some(&self.credential))
    }
}

impl<C> Clone for ScopedClient<'_, C> {
    /// Clones the binding, not the transport: both handles keep borrowing the
    /// one client.
    fn clone(&self) -> Self {
        Self {
            client: self.client,
            credential: Arc::clone(&self.credential),
        }
    }
}

impl<C> std::fmt::Debug for ScopedClient<'_, C> {
    /// Never prints the credential.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedClient").finish_non_exhaustive()
    }
}

/// Lets a client hand out a [`ScopedClient`] over its own transport.
///
/// Implement it on a generated client (`impl ScopeCall for Client {}`) to give
/// callers whose credential varies per call a way to say so at the call site.
pub trait ScopeCall: Sized {
    /// Bind `credential` to exactly one call over this client's transport.
    #[must_use]
    fn with_credential(&self, credential: impl Into<String>) -> ScopedClient<'_, Self> {
        ScopedClient::new(self, credential)
    }
}

/// Build call options for one request: the bearer credential it presents, plus
/// the active trace context so the callee's span links to the caller's.
fn call_options(bearer: Option<&str>) -> CallOptions {
    let options = match bearer {
        Some(token) => CallOptions::default()
            .with_header(http::header::AUTHORIZATION, format!("Bearer {token}")),
        None => CallOptions::default(),
    };

    #[cfg(feature = "opentelemetry")]
    let options = {
        let mut options = options;
        let mut headers = http::HeaderMap::new();
        crate::propagation::inject_current(&mut headers);
        for (name, value) in headers {
            if let Some(name) = name {
                options = options.with_header(name, value);
            }
        }
        options
    };

    options
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authorization(options: &CallOptions) -> Option<String> {
        options
            .headers()
            .get(http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    }

    /// A stand-in for a generated client: it owns a transport and a
    /// process-wide credential, and opts into per-call scoping.
    struct Client {
        auth: CallCredential,
    }

    impl ScopeCall for Client {}

    #[test]
    fn a_scoped_handle_presents_the_credential_it_was_given() {
        let client = Client {
            auth: CallCredential::bearer("process-identity"),
        };

        let scoped = client.with_credential("this-call-only");

        assert_eq!(scoped.credential(), "this-call-only");
        assert_eq!(
            authorization(&scoped.call_options()).as_deref(),
            Some("Bearer this-call-only")
        );
    }

    /// The process-wide injector stays available and unchanged for the case it
    /// was built for: an identity that belongs to the process.
    #[test]
    fn an_unscoped_call_still_presents_the_process_identity() {
        let client = Client {
            auth: CallCredential::bearer("process-identity"),
        };

        assert!(client.auth.is_configured());
        assert_eq!(client.auth.current().as_deref(), Some("process-identity"));
        assert_eq!(
            authorization(&client.auth.call_options()).as_deref(),
            Some("Bearer process-identity")
        );
    }

    /// An injector is re-read per request, which is what keeps a rotating
    /// token current.
    #[test]
    fn an_injector_is_re_read_on_every_call() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let auth = CallCredential::from_token_injector(move || {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            format!("token-{n}")
        });

        assert_eq!(auth.current().as_deref(), Some("token-0"));
        assert_eq!(auth.current().as_deref(), Some("token-1"));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn a_shared_injector_is_adopted_as_is() {
        let injector: TokenInjector = Arc::new(|| "shared".to_owned());
        let auth = CallCredential::shared(injector);

        assert_eq!(auth.current().as_deref(), Some("shared"));
    }

    /// Mutual-TLS transports send no bearer at all; the absence must not be
    /// reported as an empty credential.
    #[test]
    fn no_credential_sends_no_authorization_header() {
        let auth = CallCredential::none();

        assert!(!auth.is_configured());
        assert_eq!(auth.current(), None);
        assert_eq!(authorization(&auth.call_options()), None);

        let default = CallCredential::default();
        assert!(!default.is_configured());
    }

    /// A credential that reaches a log is a leaked credential.
    #[test]
    fn debug_output_never_contains_the_credential() {
        let client = Client {
            auth: CallCredential::bearer("super-secret"),
        };
        let scoped = client.with_credential("also-secret");

        let rendered = format!("{:?} {:?}", client.auth, scoped);
        assert!(
            !rendered.contains("secret"),
            "credential leaked: {rendered}"
        );
        assert!(rendered.contains("configured: true"));
    }

    /// Cloning a handle clones the binding, not the transport.
    #[test]
    fn cloning_a_handle_keeps_borrowing_the_same_client() {
        let client = Client {
            auth: CallCredential::none(),
        };
        let scoped = client.with_credential("call-credential");
        let clone = scoped.clone();

        assert!(std::ptr::eq(scoped.client(), clone.client()));
        assert_eq!(clone.credential(), "call-credential");
    }

    /// Scoping borrows: the handle points at the very client that produced it,
    /// so nothing about the transport is rebuilt.
    #[test]
    fn scoping_borrows_the_parent_client_rather_than_rebuilding_it() {
        let client = Client {
            auth: CallCredential::bearer("process-identity"),
        };

        let first = client.with_credential("first");
        let second = client.with_credential("second");

        assert!(std::ptr::eq(first.client(), &raw const client));
        assert!(std::ptr::eq(second.client(), &raw const client));
        assert_eq!(first.credential(), "first");
        assert_eq!(second.credential(), "second");
    }

    /// The race this type exists to prevent: two scoped handles taken from one
    /// client, called concurrently, must each present their own credential.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_scoped_calls_do_not_observe_each_other() {
        let client = Client {
            auth: CallCredential::bearer("process-identity"),
        };
        // Both handles resolve their options while the other is mid-flight.
        let gate = Arc::new(tokio::sync::Barrier::new(2));

        let one = {
            let gate = Arc::clone(&gate);
            let scoped = client.with_credential("credential-one");
            async move {
                gate.wait().await;
                tokio::task::yield_now().await;
                authorization(&scoped.call_options())
            }
        };
        let two = {
            let gate = Arc::clone(&gate);
            let scoped = client.with_credential("credential-two");
            async move {
                gate.wait().await;
                tokio::task::yield_now().await;
                authorization(&scoped.call_options())
            }
        };

        let (first, second) = tokio::join!(one, two);

        assert_eq!(first.as_deref(), Some("Bearer credential-one"));
        assert_eq!(second.as_deref(), Some("Bearer credential-two"));
    }
}
