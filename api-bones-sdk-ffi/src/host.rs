// SPDX-License-Identifier: MIT
//! The host a native app describes, and the env built over it.
//!
//! Only the native Kotlin or Swift layer implements [`FfiHost`]: the app code
//! above it never sees a credential. A host call that fails, throws or panics
//! answers nothing, and the call fails closed with the fixed "could not
//! answer" refusal; the foreign failure's text never enters an error.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, LazyLock};

use api_bones_batch::{CredentialAsk, CredentialFuture, CredentialSource};
use api_bones_page::RevisionCache;
use api_bones_sdk::__private::{credential_read, span_end, span_start, span_started};
use api_bones_sdk::call::host::unanswered;
use api_bones_sdk::call::{CallEnv, Notify};
use api_bones_sdk::{SpanEnd, SpanSink, SpanStart, SpanStarted};

use crate::FfiError;

/// What a native app answers to the core.
///
/// Every value is the protobuf encoding of a `bones.sdk.v1` message: the
/// credential is a `CredentialAnswer`, a span start is a `SpanStart` answered
/// with a `SpanStarted`, a span end is a `SpanEnd`.
#[uniffi::export(with_foreign)]
pub trait FfiHost: Send + Sync {
    /// The caller's credential as the bytes of a `CredentialAnswer`, renewed
    /// first when `refresh` says the server refused the last one. Asked before
    /// every send.
    ///
    /// # Errors
    /// When the host cannot answer; nothing is sent.
    fn credential(&self, refresh: bool) -> Result<Vec<u8>, FfiError>;

    /// Whether the caller presents no credential.
    fn anonymous(&self) -> bool;

    /// A server refused the caller as unauthenticated and the host's own
    /// credential could not cure it.
    fn unauthenticated(&self);

    /// A span began: the bytes of a `SpanStart`; answers a `SpanStarted`.
    fn span_start(&self, start: Vec<u8>) -> Vec<u8>;

    /// A span the host started ended: the bytes of a `SpanEnd`.
    fn span_end(&self, end: Vec<u8>);
}

/// Runs a host call; `None` when it panicked.
fn guarded<T>(call: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(call)).ok()
}

/// The revision cache of this process, shared by every env. It holds one slot
/// per caller identity, so envs of different callers never invalidate each
/// other, and a refusal seen through one client clears what another client of
/// the same caller filled.
static REVISIONS: LazyLock<RevisionCache> = LazyLock::new(RevisionCache::default);

/// The credential of a host, asked on every read.
struct HostCredential(Arc<dyn FfiHost>);

impl HostCredential {
    /// The bytes the host answered, read off the async worker so a host that
    /// blocks does not stall it.
    async fn answer(&self, ask: CredentialAsk) -> Result<Vec<u8>, connectrpc::ConnectError> {
        let host = Arc::clone(&self.0);
        tokio::task::spawn_blocking(move || guarded(|| host.credential(ask.refresh)))
            .await
            .ok()
            .flatten()
            .and_then(Result::ok)
            .ok_or_else(unanswered)
    }
}

impl CredentialSource for HostCredential {
    fn current(&self, ask: CredentialAsk) -> CredentialFuture<'_> {
        Box::pin(async move { credential_read(&self.answer(ask).await?) })
    }
}

/// The spans of an env, reported to the host.
struct HostSink(Arc<dyn FfiHost>);

impl SpanSink for HostSink {
    fn start(&self, span: &SpanStart) -> SpanStarted {
        guarded(|| self.0.span_start(span_start(span)))
            .and_then(|answer| span_started(&answer))
            .unwrap_or_default()
    }

    fn end(&self, end: &SpanEnd) {
        let _ = guarded(|| self.0.span_end(span_end(end)));
    }
}

/// The env a host describes: its `credential` is read before every send and a
/// failed or unreadable answer fails the call closed; an `anonymous` host
/// presents no credential; `unauthenticated` is told of every refusal the
/// host's own credential could not cure; the host receives the spans. Every
/// call shares the process's one revision cache, in the caller's own slot.
/// Bind the env to its server with `CallEnv::at` before listing.
#[must_use]
pub fn call_env(host: Arc<dyn FfiHost>) -> CallEnv {
    let anonymous = guarded(|| host.anonymous()).unwrap_or(false);
    let credentials = (!anonymous).then(|| HostCredential(Arc::clone(&host)));
    let notify = notifier(Arc::clone(&host));
    CallEnv::hosted(
        credentials,
        Arc::new(HostSink(host)),
        REVISIONS.clone(),
        notify,
    )
}

/// Tells the host of an unauthenticated refusal.
fn notifier(host: Arc<dyn FfiHost>) -> Notify {
    Arc::new(move || {
        let _ = guarded(|| host.unauthenticated());
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use api_bones_sdk::{Attributes, CallCtx, SpanKind, Tracer};
    use connectrpc::ErrorCode;

    use super::*;

    /// A host whose answers a test sets, counting what the core asked.
    #[derive(Default)]
    pub(crate) struct Scripted {
        pub(crate) credential: Mutex<Option<Result<Vec<u8>, FfiError>>>,
        pub(crate) anonymous: bool,
        pub(crate) panics: bool,
        pub(crate) credential_reads: AtomicUsize,
        pub(crate) refusals: AtomicUsize,
        pub(crate) started: Mutex<Vec<Vec<u8>>>,
        pub(crate) ended: Mutex<Vec<Vec<u8>>>,
        pub(crate) span_answer: Vec<u8>,
    }

    impl FfiHost for Scripted {
        fn credential(&self, _refresh: bool) -> Result<Vec<u8>, FfiError> {
            self.credential_reads.fetch_add(1, Ordering::SeqCst);
            assert!(!self.panics, "sentinel-secret");
            self.credential
                .lock()
                .expect("lock")
                .take()
                .unwrap_or_else(|| Ok(Vec::new()))
        }

        fn anonymous(&self) -> bool {
            assert!(!self.panics, "sentinel-secret");
            self.anonymous
        }

        fn unauthenticated(&self) {
            assert!(!self.panics, "sentinel-secret");
            self.refusals.fetch_add(1, Ordering::SeqCst);
        }

        fn span_start(&self, start: Vec<u8>) -> Vec<u8> {
            assert!(!self.panics, "sentinel-secret");
            self.started.lock().expect("lock").push(start);
            self.span_answer.clone()
        }

        fn span_end(&self, end: Vec<u8>) {
            assert!(!self.panics, "sentinel-secret");
            self.ended.lock().expect("lock").push(end);
        }
    }

    /// A `CredentialAnswer` naming an empty credential.
    const EMPTY_CREDENTIAL: [u8; 2] = [0x0a, 0x00];

    async fn read(host: Arc<Scripted>) -> Result<(), connectrpc::ConnectError> {
        HostCredential(host)
            .current(CredentialAsk { refresh: false })
            .await
            .map(|_| ())
    }

    #[tokio::test]
    async fn a_host_answer_naming_a_credential_is_read() {
        let host = Arc::new(Scripted {
            credential: Mutex::new(Some(Ok(EMPTY_CREDENTIAL.to_vec()))),
            ..Scripted::default()
        });
        read(Arc::clone(&host)).await.expect("a credential");
        assert_eq!(host.credential_reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_answer_that_is_not_a_credential_answer_is_a_failed_precondition() {
        let host = Arc::new(Scripted {
            credential: Mutex::new(Some(Ok(vec![0x0a]))),
            ..Scripted::default()
        });
        let error = read(host).await.expect_err("unreadable");
        assert_eq!(error.code, ErrorCode::FailedPrecondition);
    }

    #[tokio::test]
    async fn a_host_that_cannot_answer_is_unavailable() {
        let host = Arc::new(Scripted {
            credential: Mutex::new(Some(Err(FfiError::Call { bytes: vec![1] }))),
            ..Scripted::default()
        });
        let error = read(host).await.expect_err("unanswered");
        assert_eq!(error.code, ErrorCode::Unavailable);
    }

    #[tokio::test]
    async fn a_host_that_panics_is_unavailable_and_its_text_never_crosses() {
        let host = Arc::new(Scripted {
            panics: true,
            ..Scripted::default()
        });
        let error = read(host).await.expect_err("unanswered");
        assert_eq!(error.code, ErrorCode::Unavailable);
        assert!(!format!("{error:?}").contains("sentinel-secret"));
    }

    #[tokio::test]
    async fn an_env_over_a_host_reads_nothing_until_a_call_asks() {
        let host = Arc::new(Scripted::default());
        let env = call_env(host.clone());
        let _ = env.call("acme.v1.Widgets", "GetWidget");
        assert_eq!(host.credential_reads.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn an_anonymous_host_presents_no_credential_and_a_panicking_one_is_not_anonymous() {
        let anonymous = Arc::new(Scripted {
            anonymous: true,
            ..Scripted::default()
        });
        let env = call_env(anonymous.clone());
        let _ = env.call("acme.v1.Widgets", "GetWidget");
        assert_eq!(anonymous.credential_reads.load(Ordering::SeqCst), 0);

        let panicking = Arc::new(Scripted {
            panics: true,
            ..Scripted::default()
        });
        let _ = call_env(panicking);
    }

    #[test]
    fn an_unauthenticated_refusal_reaches_the_host_and_a_panic_there_is_contained() {
        let host = Arc::new(Scripted::default());
        notifier(host.clone())();
        assert_eq!(host.refusals.load(Ordering::SeqCst), 1);
        let panicking = Arc::new(Scripted {
            panics: true,
            ..Scripted::default()
        });
        notifier(panicking)();
    }

    #[test]
    fn spans_reach_the_host_and_a_silent_or_panicking_host_answers_not_recording() {
        let host = Arc::new(Scripted {
            span_answer: vec![0x0a, 0x03, b'a', b'b', b'c', 0x10, 0x01],
            ..Scripted::default()
        });
        let tracer = Tracer::new(Arc::new(HostSink(host.clone())));
        let span = tracer.start(
            "acme.v1.Widgets/GetWidget",
            SpanKind::Client,
            &CallCtx::default(),
            Attributes::new(),
        );
        assert_eq!(host.started.lock().expect("lock").len(), 1);
        span.end(api_bones_sdk::SpanStatus::Ok);
        assert_eq!(host.ended.lock().expect("lock").len(), 1);

        let silent = Arc::new(Scripted::default());
        let sink = HostSink(silent);
        let started = sink.start(&SpanStart {
            name: "n".to_owned(),
            start_time_ms: 0.0,
            kind: SpanKind::Internal,
            parent: None,
            tracestate: None,
            attributes: Attributes::new(),
        });
        assert_eq!(started, SpanStarted::default());

        let panicking = HostSink(Arc::new(Scripted {
            panics: true,
            ..Scripted::default()
        }));
        let start = SpanStart {
            name: "n".to_owned(),
            start_time_ms: 0.0,
            kind: SpanKind::Internal,
            parent: None,
            tracestate: None,
            attributes: Attributes::new(),
        };
        assert_eq!(panicking.start(&start), SpanStarted::default());
        panicking.end(&SpanEnd {
            traceparent: String::new(),
            end_time_ms: 0.0,
            status: api_bones_sdk::SpanStatus::Unset,
            error: None,
            attributes: Attributes::new(),
        });
    }
}
