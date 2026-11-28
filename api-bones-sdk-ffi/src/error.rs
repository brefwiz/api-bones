// SPDX-License-Identifier: MIT
//! The one failure that crosses to a host.

use api_bones_sdk::__private::encode_call_error;
use api_bones_sdk::Error;
use api_bones_sdk::call::host::unanswered;

/// A failed call: the `bones.sdk.v1.CallError` bytes, the same value the
/// browser binding rejects with. The host decodes them, and renders text for a
/// person through `format_error`, never from the diagnostic message.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    /// The encoded `bones.sdk.v1.CallError`.
    #[error("the call failed")]
    Call {
        /// The `CallError` message.
        bytes: Vec<u8>,
    },
}

impl From<Error> for FfiError {
    fn from(error: Error) -> Self {
        Self::Call {
            bytes: encode_call_error(&error),
        }
    }
}

/// A host callback that failed in the foreign runtime is the fixed
/// "could not answer" refusal; the foreign exception text never crosses.
impl From<uniffi::UnexpectedUniFFICallbackError> for FfiError {
    fn from(_: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Error::from(unanswered()).into()
    }
}

#[cfg(test)]
mod tests {
    use api_bones_sdk::__private::decode_call_error;

    use super::*;

    #[test]
    fn a_core_error_crosses_as_its_call_error_bytes() {
        let FfiError::Call { bytes } = FfiError::from(Error::canceled());
        let back = decode_call_error(&bytes).expect("decodes");
        assert_eq!(back.code, Error::canceled().code);
    }

    #[test]
    fn a_failed_foreign_callback_is_the_fixed_unanswered_refusal() {
        let error = FfiError::from(uniffi::UnexpectedUniFFICallbackError::new(
            "sentinel-secret".to_owned(),
        ));
        let FfiError::Call { bytes } = error;
        let back = decode_call_error(&bytes).expect("decodes");
        assert_eq!(
            back.connect_code,
            api_bones::connect::ConnectCode::Unavailable
        );
        assert!(!back.message.contains("sentinel-secret"));
        assert!(back.message.contains("could not answer"));
    }

    #[test]
    fn the_error_names_no_detail() {
        let error = FfiError::Call { bytes: vec![1] };
        assert_eq!(error.to_string(), "the call failed");
    }
}
