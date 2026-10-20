// SPDX-License-Identifier: MIT
//! The BFF CSRF synchronizer's replay rule.
//!
//! A session BFF refuses a state-changing call that does not echo its current
//! synchronizer token with HTTP 403 and hands the fresh token back in
//! [`CSRF_HEADER`]. The call is safe to replay exactly once with that token.
//! [`csrf_replay_token`] is the one place that decides it, so no caller
//! restates the rule. The TypeScript package exports the same rule.

use std::sync::Mutex;

/// The response and request header that carries the synchronizer token.
pub const CSRF_HEADER: &str = "x-csrf-token";

/// The token to replay a refused call with, or `None` when it must not be
/// replayed.
///
/// Replay exactly when the refusal is a 403 carrying a token different from
/// the one sent (including when none was sent). A 403 carrying the token that
/// was sent, or none, is a refusal for another reason.
#[must_use]
pub fn csrf_replay_token(
    status: u16,
    refusal_token: Option<&str>,
    sent_token: Option<&str>,
) -> Option<String> {
    match refusal_token {
        Some(token) if status == 403 && !token.is_empty() && Some(token) != sent_token => {
            Some(token.to_owned())
        }
        _ => None,
    }
}

/// The latest synchronizer token a BFF handed out.
///
/// One instance per BFF origin; there is no process-wide state. Token values
/// are never logged.
#[derive(Default)]
pub struct CsrfSynchronizer {
    token: Mutex<Option<String>>,
}

impl CsrfSynchronizer {
    /// Keep `token` as the current one. An absent or empty token changes nothing.
    pub fn remember(&self, token: Option<&str>) {
        if let Some(token) = token.filter(|t| !t.is_empty()) {
            *self
                .token
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(token.to_owned());
        }
    }

    /// The token to send on the next call, if one has been handed out.
    #[must_use]
    pub fn current(&self) -> Option<String> {
        self.token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl std::fmt::Debug for CsrfSynchronizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CsrfSynchronizer").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Case<'a> = (u16, Option<&'a str>, Option<&'a str>, Option<&'a str>);

    #[test]
    fn replay_truth_table() {
        let cases: [Case<'_>; 8] = [
            (403, Some("fresh"), Some("stale"), Some("fresh")),
            (403, Some("same"), Some("same"), None),
            (403, None, Some("sent"), None),
            (403, Some(""), Some("sent"), None),
            (401, Some("fresh"), Some("stale"), None),
            (200, Some("fresh"), Some("stale"), None),
            (403, Some("fresh"), None, Some("fresh")),
            (403, None, None, None),
        ];
        for (status, refusal, sent, want) in cases {
            assert_eq!(
                csrf_replay_token(status, refusal, sent).as_deref(),
                want,
                "{status} {refusal:?} {sent:?}"
            );
        }
    }

    #[test]
    fn synchronizer_keeps_latest_and_ignores_absent() {
        let sync = CsrfSynchronizer::default();
        assert_eq!(sync.current(), None);
        sync.remember(Some("first"));
        sync.remember(None);
        sync.remember(Some(""));
        assert_eq!(sync.current().as_deref(), Some("first"));
        sync.remember(Some("second"));
        assert_eq!(sync.current().as_deref(), Some("second"));
        assert!(format!("{sync:?}").contains("CsrfSynchronizer"));
    }

    #[test]
    fn synchronizer_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CsrfSynchronizer>();
    }
}
