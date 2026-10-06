// SPDX-License-Identifier: MIT
//! Header names owned by the request identity protocol.

#[cfg(any(feature = "connect", feature = "opentelemetry"))]
pub(crate) const AUTHORIZATION: &str = "authorization";
pub(crate) const ORG_ID: &str = "x-org-id";
pub(crate) const ORG_PATH: &str = "x-org-path";
#[cfg(any(feature = "connect", feature = "opentelemetry"))]
pub(crate) const SUBJECT_ID: &str = "x-subject-id";

#[cfg(feature = "opentelemetry")]
pub(crate) fn is_identity_header(name: &str) -> bool {
    [AUTHORIZATION, ORG_ID, ORG_PATH, SUBJECT_ID]
        .iter()
        .any(|reserved| name.eq_ignore_ascii_case(reserved))
}
