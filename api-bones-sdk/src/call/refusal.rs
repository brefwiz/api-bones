// SPDX-License-Identifier: MIT
//! The refusals the call helper makes itself, as `ConnectError`s.
//!
//! The call helper's inner layers (retry, page fetch) speak `ConnectError`.
//! A local refusal travels through them as one carrying a `bones.v1.ErrorInfo`
//! of emitter `bones`, so the single conversion at the edge
//! (`Error::from(ConnectError)`) gives the same error the vocabulary's own
//! constructors do.

use api_bones::connect::{ErrorInfo, error_info, with_error_info};
use api_bones_batch::BATCH_IDENTITY_CHANGED;
use connectrpc::ConnectError;

use crate::error::{CALL_BUDGET_ELAPSED, CANCELED, EMITTER, IDENTITY_CHANGED};

fn refusal(code: &str, error: ConnectError) -> ConnectError {
    let mut info = ErrorInfo::from_code(code);
    EMITTER.clone_into(&mut info.emitter);
    with_error_info(error, &info)
}

/// The call did not finish within its call budget.
pub(crate) fn budget_elapsed() -> ConnectError {
    refusal(
        CALL_BUDGET_ELAPSED,
        ConnectError::deadline_exceeded("the call did not finish within its call budget"),
    )
}

/// The call was aborted.
pub(crate) fn canceled() -> ConnectError {
    refusal(CANCELED, ConnectError::canceled("the call was canceled"))
}

/// The caller's identity changed; a batch keeps its own code for it.
pub(crate) fn identity_changed(batch: bool) -> ConnectError {
    if batch {
        refusal(
            BATCH_IDENTITY_CHANGED,
            ConnectError::unauthenticated("the caller identity changed during the batch"),
        )
    } else {
        refusal(
            IDENTITY_CHANGED,
            ConnectError::unauthenticated("the caller identity changed during the call"),
        )
    }
}

/// Whether `error` is the core's own refusal with `code`.
pub(crate) fn is(error: &ConnectError, code: &str) -> bool {
    error_info(error).is_some_and(|info| info.code == code && info.emitter == EMITTER)
}

/// Whether `error` is the core's own refusal for a changed identity.
pub(crate) fn is_identity_changed(error: &ConnectError) -> bool {
    is(error, IDENTITY_CHANGED) || is(error, BATCH_IDENTITY_CHANGED)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn a_refusal_converts_to_the_vocabulary_error_of_the_same_name() {
        let cases = [
            (budget_elapsed(), Error::call_budget_elapsed()),
            (canceled(), Error::canceled()),
            (identity_changed(false), Error::identity_changed()),
        ];
        for (refusal, expected) in cases {
            assert_eq!(Error::from(refusal), expected);
        }
    }

    #[test]
    fn a_batch_keeps_its_own_identity_code() {
        let error = Error::from(identity_changed(true));
        assert_eq!(error.code, BATCH_IDENTITY_CHANGED);
        assert_eq!(error.emitter, EMITTER);
        assert_eq!(error.platform, "UNAUTHORIZED");
    }

    #[test]
    fn only_the_cores_own_refusal_matches_its_code() {
        assert!(is(&canceled(), CANCELED));
        assert!(!is(&canceled(), IDENTITY_CHANGED));
        assert!(!is(&ConnectError::canceled("a server said so"), CANCELED));
        assert!(is_identity_changed(&identity_changed(false)));
        assert!(is_identity_changed(&identity_changed(true)));
        assert!(!is_identity_changed(&budget_elapsed()));
    }
}
