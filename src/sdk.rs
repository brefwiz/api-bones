// SPDX-License-Identifier: MIT
//! Helpers an SDK error type uses to expose the emitted error code.
//!
//! See [`ErrorInfo`] for the wire shape and the text token.

pub use crate::connect::{
    CarriesErrorInfo, ERROR_INFO_TYPE, ErrorInfo, MAX_ERROR_INFOS, error_info, error_infos,
    with_error_info,
};
