// SPDX-License-Identifier: MIT
//! The env a browser host describes. The bytes a host answers are read by
//! `crate::boundary::credential_read`, the one place the answer's wire form
//! is decoded.
//!
//! The host is a plain object: `credential(refresh, signal)` answers the
//! caller's credential as the bytes of a `bones.sdk.v1.CredentialAnswer`
//! (through a promise or not), `anonymous` says the caller presents none,
//! `unauthenticated()` is told when a server refuses the caller, and
//! `start(bytes)` / `end(bytes)` receive the call's spans.

/// The host's credential answer is not usable: it is not a function, not
/// bytes, or ends before it names an answer. The call fails closed with
/// nothing sent as an unknown caller.
#[doc(hidden)]
#[must_use]
pub fn unreadable(why: &str) -> connectrpc::ConnectError {
    connectrpc::ConnectError::failed_precondition(format!("the host's credential {why}"))
}

/// The host's `credential()` threw or rejected: the source could not answer.
/// The call fails closed with nothing sent.
#[doc(hidden)]
#[must_use]
pub fn unanswered() -> connectrpc::ConnectError {
    connectrpc::ConnectError::unavailable("the host's credential source could not answer")
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use std::sync::Arc;

    use api_bones_batch::{CredentialAsk, CredentialFuture, CredentialSource};
    use api_bones_page::RevisionCache;
    use js_sys::{Function, Promise, Reflect, Uint8Array};
    use wasm_bindgen::{JsCast as _, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::AbortController;

    use super::{unanswered, unreadable};
    use crate::boundary::credential_read;
    use crate::call::CallEnv;
    use crate::span::JsSink;

    /// The member `name` of `host` when it is a function.
    fn function(host: &JsValue, name: &str) -> Option<Function> {
        Reflect::get(host, &JsValue::from_str(name))
            .ok()?
            .dyn_into::<Function>()
            .ok()
    }

    thread_local! {
        /// The revision cache of this wasm instance, shared by every env. It
        /// holds one slot per caller identity, so envs of different callers
        /// never invalidate each other, and a refusal seen through one client
        /// clears what another client of the same caller filled.
        static REVISIONS: RevisionCache = RevisionCache::default();
    }

    /// The signal a credential read runs under: aborted when the read is
    /// dropped before it settled, so a call that is abandoned or out of
    /// budget does not leave the host's token request running.
    struct ReadSignal(Option<AbortController>);

    impl ReadSignal {
        fn new() -> Result<Self, wasm_bindgen::JsValue> {
            AbortController::new().map(|controller| Self(Some(controller)))
        }

        fn signal(&self) -> JsValue {
            self.0
                .as_ref()
                .map_or(JsValue::UNDEFINED, |controller| controller.signal().into())
        }

        fn settled(mut self) {
            self.0 = None;
        }
    }

    impl Drop for ReadSignal {
        fn drop(&mut self) {
            if let Some(controller) = self.0.take() {
                controller.abort();
            }
        }
    }

    /// The credential of a host, asked on every read. The host answers its
    /// credential every time and keeps no state the core relies on.
    struct HostCredential {
        host: JsValue,
        /// The host's `credential`, looked up once.
        read: Option<Function>,
    }

    impl HostCredential {
        fn new(host: JsValue) -> Self {
            Self {
                read: function(&host, "credential"),
                host,
            }
        }

        /// The bytes of the host's answer, once its promise settled.
        async fn answer(&self, ask: CredentialAsk) -> Result<Vec<u8>, connectrpc::ConnectError> {
            let read = self
                .read
                .as_ref()
                .ok_or_else(|| unreadable("is not a function"))?;
            let signal = ReadSignal::new().map_err(|_| unanswered())?;
            let promise = read
                .call2(
                    &self.host,
                    &JsValue::from_bool(ask.refresh),
                    &signal.signal(),
                )
                .map_err(|_| unanswered())?;
            let answer = JsFuture::from(Promise::resolve(&promise))
                .await
                .map_err(|_| unanswered())?;
            signal.settled();
            answer
                .dyn_into::<Uint8Array>()
                .map(|bytes| bytes.to_vec())
                .map_err(|_| unreadable("is not bytes"))
        }
    }

    impl CredentialSource for HostCredential {
        fn current(&self, ask: CredentialAsk) -> CredentialFuture<'_> {
            Box::pin(async move { credential_read(&self.answer(ask).await?) })
        }
    }

    impl CallEnv {
        /// The env a browser host describes: its `credential(refresh,
        /// signal)` is read before every send and a rejection or unreadable
        /// answer fails the call closed; `anonymous === true` presents no
        /// credential; `unauthenticated()` is told of every unauthenticated
        /// refusal the host's own credential could not cure; `start` and
        /// `end` receive the spans. Every call of the env shares the
        /// instance's one revision cache, in the caller's own slot. Bind the
        /// env to its server with [`CallEnv::at`] before listing.
        #[must_use]
        pub fn from_host(host: JsValue) -> Self {
            let anonymous = Reflect::get(&host, &JsValue::from_str("anonymous"))
                .is_ok_and(|value| value.as_bool() == Some(true));
            let credentials = (!anonymous).then(|| HostCredential::new(host.clone()));
            let told = host.clone();
            Self::hosted(
                credentials,
                Arc::new(JsSink::new(host)),
                REVISIONS.with(RevisionCache::clone),
                Arc::new(move || {
                    if let Some(notify) = function(&told, "unauthenticated") {
                        let _ = notify.call0(&told);
                    }
                }),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use api_bones_batch::Anonymous;
    use api_bones_page::RevisionCache;
    use connectrpc::ErrorCode;

    use super::*;
    use crate::call::CallEnv;
    use crate::span::{SpanEnd, SpanSink, SpanStart, SpanStarted};

    struct Quiet;

    impl SpanSink for Quiet {
        fn start(&self, _: &SpanStart) -> SpanStarted {
            SpanStarted::default()
        }

        fn end(&self, _: &SpanEnd) {}
    }

    #[test]
    fn a_hosted_env_wires_its_cache_and_its_refusal_notice_with_or_without_a_credential() {
        for credentials in [Some(Anonymous), None] {
            let env = CallEnv::hosted(
                credentials,
                Arc::new(Quiet),
                RevisionCache::default(),
                Arc::new(|| {}),
            );
            assert!(env.cache.is_some());
            assert!(env.unauthenticated.is_some());
        }
    }

    #[test]
    fn a_throwing_credential_source_is_unavailable() {
        let error = unanswered();
        assert_eq!(error.code, ErrorCode::Unavailable);
    }

    #[test]
    fn an_unreadable_answer_is_a_failed_precondition_naming_why() {
        let error = unreadable("is not bytes");
        assert_eq!(error.code, ErrorCode::FailedPrecondition);
        assert!(
            error
                .message
                .as_deref()
                .is_some_and(|m| m.contains("is not bytes")),
            "{error:?}"
        );
    }
}
