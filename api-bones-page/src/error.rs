// SPDX-License-Identifier: MIT
//! The error a page stream ends with.

use api_bones::connect::CarriesErrorInfo as _;
use connectrpc::ConnectError;

use crate::reason::{CURSOR_EXPIRED, CURSOR_INVALID, CURSOR_QUERY_MISMATCH};

/// Why a page fetch failed.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum PageErrorKind {
    /// The server could not read the cursor.
    #[error("cursor invalid")]
    CursorInvalid,
    /// The cursor is authentic but older than the server accepts.
    #[error("cursor expired")]
    CursorExpired,
    /// The cursor was issued for a different query.
    #[error("cursor query mismatch")]
    CursorQueryMismatch,
    /// Any other failure of the call.
    #[error(transparent)]
    Rpc(ConnectError),
}

/// The single error a page stream yields before it ends.
///
/// Items delivered before the failure stay delivered; the counts say how many.
#[derive(Debug, thiserror::Error)]
#[error("{kind} after {delivered_pages} pages / {delivered_items} items")]
pub struct PageError {
    /// What failed.
    pub kind: PageErrorKind,
    /// Pages handed to the consumer before the failure.
    pub delivered_pages: u32,
    /// Items handed to the consumer before the failure.
    pub delivered_items: u64,
}

impl PageError {
    /// Classify a Connect failure using its canonical `ErrorInfo` detail.
    ///
    /// The delivery counts describe pages and items already handed to the
    /// caller before this failure; classification preserves that context.
    #[must_use]
    pub fn from_connect(error: ConnectError, delivered_pages: u32, delivered_items: u64) -> Self {
        let kind = match error.code().as_deref() {
            Some(CURSOR_INVALID) => PageErrorKind::CursorInvalid,
            Some(CURSOR_EXPIRED) => PageErrorKind::CursorExpired,
            Some(CURSOR_QUERY_MISMATCH) => PageErrorKind::CursorQueryMismatch,
            _ => PageErrorKind::Rpc(error),
        };
        Self {
            kind,
            delivered_pages,
            delivered_items,
        }
    }

    /// Whether the server refused the cursor, so the list must restart from
    /// its first page.
    #[must_use]
    pub fn is_cursor_rejected(&self) -> bool {
        matches!(
            self.kind,
            PageErrorKind::CursorInvalid
                | PageErrorKind::CursorExpired
                | PageErrorKind::CursorQueryMismatch
        )
    }
}
