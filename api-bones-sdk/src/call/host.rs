// SPDX-License-Identifier: MIT
//! The env a browser host describes.
//!
//! The host is a plain object: `credential()` answers the caller's credential
//! as JSON text, `anonymous` says the caller presents none, `unauthenticated()`
//! is told when a server refuses the caller, and `start(json)` / `end(json)`
//! receive the call's spans.

use std::collections::BTreeMap;

use api_bones_batch::{Credential, Identity};
use connectrpc::ConnectError;
use serde::Deserialize;

/// The credential a host answers, as the JSON object it describes.
#[derive(Deserialize)]
struct Described {
    subject: String,
    org: String,
    headers: BTreeMap<String, String>,
}

/// The credential a host answered as JSON text:
/// `{"subject": "...", "org": "...", "headers": {"name": "value"}}`.
///
/// # Errors
/// `failed_precondition` when the text is not that object. The call fails
/// closed: nothing is sent as an unknown caller.
pub(super) fn credential_of(text: &str) -> Result<Credential, ConnectError> {
    let described: Described = serde_json::from_str(text)
        .map_err(|_| unreadable("is not an object of `subject`, `org` and `headers` text"))?;
    Ok(Credential {
        identity: Identity {
            subject: described.subject,
            org: described.org,
        },
        headers: described
            .headers
            .into_iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value.into_bytes()))
            .collect(),
    })
}

fn unreadable(why: &str) -> ConnectError {
    ConnectError::failed_precondition(format!("the host's credential {why}"))
}

/// The host's `credential()` threw: the source could not answer. The call
/// fails closed with nothing sent.
#[cfg(any(test, all(feature = "wasm", target_arch = "wasm32")))]
fn unanswered() -> ConnectError {
    ConnectError::unavailable("the host's credential source could not answer")
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod browser {
    use std::sync::Arc;

    use api_bones_batch::{Anonymous, Credential, CredentialSource};
    use api_bones_page::RevisionCache;
    use connectrpc::ConnectError;
    use js_sys::{Function, Reflect};
    use wasm_bindgen::{JsCast as _, JsValue};

    use super::{credential_of, unanswered, unreadable};
    use crate::call::CallEnv;
    use crate::span::{JsSink, Tracer};

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

    /// The credential of a host, asked on every read.
    struct HostCredential(JsValue);

    impl CredentialSource for HostCredential {
        fn current(&self) -> Result<Credential, ConnectError> {
            let read =
                function(&self.0, "credential").ok_or_else(|| unreadable("is not a function"))?;
            let answer = read.call0(&self.0).map_err(|_| unanswered())?;
            credential_of(
                &answer
                    .as_string()
                    .ok_or_else(|| unreadable("is not text"))?,
            )
        }
    }

    impl CallEnv {
        /// The env a browser host describes: its `credential()` is read
        /// before every send and a throw or unreadable answer fails the call
        /// closed; `anonymous === true` presents no credential;
        /// `unauthenticated()` is told of every unauthenticated refusal;
        /// `start` and `end` receive the spans. Every call of the env shares
        /// the instance's one revision cache, in the caller's own slot. Bind
        /// the env to its server with [`CallEnv::at`] before listing.
        #[must_use]
        pub fn from_host(host: JsValue) -> Self {
            let anonymous = Reflect::get(&host, &JsValue::from_str("anonymous"))
                .is_ok_and(|value| value.as_bool() == Some(true));
            let env = if anonymous {
                Self::new(Anonymous)
            } else {
                Self::new(HostCredential(host.clone()))
            };
            let told = host.clone();
            env.with_tracer(Tracer::new(Arc::new(JsSink::new(host))))
                .with_revision_cache(REVISIONS.with(RevisionCache::clone))
                .on_unauthenticated(Arc::new(move || {
                    if let Some(notify) = function(&told, "unauthenticated") {
                        let _ = notify.call0(&told);
                    }
                }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_names_its_identity_and_headers() {
        let credential =
            credential_of(r#"{"subject":"u1","org":"o1","headers":{"Authorization":"Bearer t"}}"#)
                .expect("a credential");
        assert_eq!(credential.identity.subject, "u1");
        assert_eq!(credential.identity.org, "o1");
        assert_eq!(
            credential.headers,
            vec![("authorization".to_owned(), b"Bearer t".to_vec())]
        );
    }

    #[test]
    fn an_empty_header_object_is_a_credential_without_headers() {
        let credential =
            credential_of(r#"{"subject":"","org":"","headers":{}}"#).expect("a credential");
        assert_eq!(credential.headers, Vec::<(String, Vec<u8>)>::new());
    }

    #[test]
    fn a_throwing_credential_source_is_unavailable() {
        assert_eq!(unanswered().code, connectrpc::ErrorCode::Unavailable);
    }

    #[test]
    fn an_unreadable_answer_is_a_failed_precondition() {
        assert_eq!(
            unreadable("is not text").code,
            connectrpc::ErrorCode::FailedPrecondition
        );
    }

    #[test]
    fn anything_else_fails_closed() {
        for text in [
            "not json",
            "null",
            r#"{"org":"o","headers":{}}"#,
            r#"{"subject":"s","headers":{}}"#,
            r#"{"subject":"s","org":"o"}"#,
            r#"{"subject":"s","org":"o","headers":{"authorization":7}}"#,
            r#"{"subject":1,"org":"o","headers":{}}"#,
        ] {
            let error = credential_of(text).expect_err(text);
            assert_eq!(
                error.code,
                connectrpc::ErrorCode::FailedPrecondition,
                "{text}"
            );
        }
    }
}
