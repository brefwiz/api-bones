// SPDX-License-Identifier: MIT
//! The per-call context a host passes with a call.

use api_bones_sdk::CallCtx;

/// What a host says about one call: the trace fields it continues and, for a
/// list, the reads it chooses.
///
/// The record belongs to the call, never to the client: two calls of one
/// client carry their own trace fields. Every async export, list, batch and
/// stream accepts one; `FfiCallCtx::default()` is a call that starts a new trace
/// and leaves the reads to the Layer 2 method.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct FfiCallCtx {
    /// The caller's W3C `traceparent`; none starts a new trace.
    #[uniffi(default = None)]
    pub traceparent: Option<String>,
    /// The caller's W3C `tracestate`.
    #[uniffi(default = None)]
    pub tracestate: Option<String>,
    /// The pages a list fetches ahead of its consumer: 0 reads none, any
    /// larger value reads one ahead; none leaves the Layer 2 method's choice.
    /// Read by a list only.
    #[uniffi(default = None)]
    pub prefetch: Option<u8>,
    /// A list delivers its first page only: one request, no page fetched
    /// ahead. Read by a list only.
    #[uniffi(default = false)]
    pub first_only: bool,
}

impl FfiCallCtx {
    /// The core's context for a call; the list reads only matter to a `list`.
    #[must_use]
    pub fn call_ctx(self, list: bool) -> CallCtx {
        CallCtx {
            traceparent: self.traceparent,
            tracestate: self.tracestate,
            prefetch: if list { self.prefetch } else { None },
            first_only: list && self.first_only,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chosen() -> FfiCallCtx {
        FfiCallCtx {
            traceparent: Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01".to_owned()),
            tracestate: Some("k=v".to_owned()),
            prefetch: Some(0),
            first_only: true,
        }
    }

    #[test]
    fn the_default_starts_a_new_trace_and_leaves_the_reads_alone() {
        assert_eq!(FfiCallCtx::default().call_ctx(true), CallCtx::default());
        assert_eq!(FfiCallCtx::default().call_ctx(false), CallCtx::default());
    }

    #[test]
    fn a_list_carries_the_trace_fields_and_the_reads_it_chose() {
        let ctx = chosen().call_ctx(true);
        assert_eq!(ctx.traceparent, chosen().traceparent);
        assert_eq!(ctx.tracestate, chosen().tracestate);
        assert_eq!(ctx.prefetch, Some(0));
        assert!(ctx.first_only);
    }

    #[test]
    fn any_other_call_carries_the_trace_fields_alone() {
        let ctx = chosen().call_ctx(false);
        assert_eq!(ctx.traceparent, chosen().traceparent);
        assert_eq!(ctx.tracestate, chosen().tracestate);
        assert_eq!(ctx.prefetch, None);
        assert!(!ctx.first_only);
    }
}
