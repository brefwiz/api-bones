// SPDX-License-Identifier: MIT
//! The SDK batch runtime.
//!
//! [`Batched`] wraps a generated Connect client. Per RPC pair, `batch_method!`
//! adds:
//!
//! - an explicit batch call with any number of ids: deduplicated, split into
//!   chunks of at most [`MAX_ITEMS`] ids and [`MAX_REQUEST_BYTES`] bytes, one
//!   outcome per input id in input order;
//! - a single Get whose same-tick siblings are merged into one batch request;
//! - a grouped batch list whose groups continue through the page iterator.
//!
//! Every chunk and every merged window sends a credential re-checked against
//! the identity it was keyed under, at most [`MAX_WIRE_IN_FLIGHT`] requests are
//! in flight per client instance, and at most [`MAX_QUEUED_CHUNKS`] chunks wait
//! per identity. These are platform constants with no knob.

mod admit;
mod chunk;
mod coalesce;
mod descriptor;
mod grouped;
mod item_error;
mod key;
mod macros;
mod paged;
mod results;
mod span;
mod wire;

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

pub use admit::{Admission, MAX_QUEUED_CHUNKS, MAX_WIRE_IN_FLIGHT, Permit};
pub use chunk::{MAX_ITEMS, MAX_REQUEST_BYTES};
pub use coalesce::{Coalescer, GetPlan, SingleFn};
pub use grouped::{
    BatchGroups, FirstPage, Group, GroupError, GroupedSpec, ListRequestFor, SingleList, run_grouped,
};
pub use item_error::{
    BATCH_IDENTITY_CHANGED, BATCH_ITEM_TOO_LARGE, BATCH_RESPONSE_MISMATCH, BATCH_TOO_LARGE,
    ItemErrorParts, is_never_retried, item_error_to_connect,
};
pub use key::{
    Anonymous, CallHeaders, CoalesceKey, Credential, CredentialSource, HeaderPair, Identity,
    QueueKey, TIMEOUT_HEADER, digest, header_hash,
};
pub use paged::{BONES_LIST_CACHE, BONES_NOT_MODIFIED, ListFn, not_modified};
pub use results::{BatchResults, ItemResult};
pub use span::{BatchRecord, Observer, Outcome};
pub use wire::{BatchFn, Boxed, Wire, WireResults};

/// The producer's generated client over a credential-neutral HTTP transport.
pub trait CredentialNeutralClient {
    /// The generated client's transport configuration.
    fn batch_client_config(&self) -> &connectrpc::client::ClientConfig;
    /// The generated file descriptor set, including imported method options.
    fn batch_client_descriptor(&self) -> &[u8];
    /// The fully qualified proto service of this generated client.
    fn batch_client_service(&self) -> &str;
}

/// Binds a generated client's HTTP transport and generated descriptor for batch dispatch.
/// `descriptor` is the generated file descriptor set, including imports.
#[macro_export]
macro_rules! batch_client {
    (client: $client:ident<$transport:ty>, service: $service:literal, descriptor: $descriptor:expr $(,)?) => {
        const _: fn(
            $client<$transport>,
        ) -> $client<$crate::__private::connectrpc::client::HttpClient> = |client| client;
        impl $crate::CredentialNeutralClient
            for $client<$crate::__private::connectrpc::client::HttpClient>
        {
            fn batch_client_config(&self) -> &$crate::__private::connectrpc::client::ClientConfig {
                self.config()
            }
            fn batch_client_descriptor(&self) -> &[u8] {
                $descriptor
            }
            fn batch_client_service(&self) -> &str {
                $service
            }
        }
    };
}

/// What `batch_method!` expands against. Not a stable surface.
#[doc(hidden)]
pub mod __private {
    pub use crate::macros::{tag_len, template_bytes};
    pub use crate::paged::CanonicalPagedRequest;
    pub use api_bones::connect::Idempotency;
    pub use api_bones_page::{
        ListCall, ListResponse, PageInfo, PagedRequest, PagedResponse, Total, TotalKind,
    };
    pub use buffa;
    pub use connectrpc;
    pub use connectrpc::ConnectError;
    pub use connectrpc::client::CallOptions;
}

type Coalescers = Mutex<HashMap<&'static str, Arc<dyn Any + Send + Sync>>>;

/// A generated Connect client with batch and paging runtime: one wire, one
/// revision cache and one set of coalescers per instance.
///
/// Clones share them, so the in-flight bound and the coalescing windows hold
/// across clones.
pub struct Batched<C> {
    inner: Arc<C>,
    wire: Arc<Wire>,
    coalescers: Arc<Coalescers>,
    methods: Arc<descriptor::Methods>,
    revisions: api_bones_page::RevisionCache,
}

impl<C> Clone for Batched<C> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            wire: Arc::clone(&self.wire),
            coalescers: Arc::clone(&self.coalescers),
            methods: Arc::clone(&self.methods),
            revisions: self.revisions.clone(),
        }
    }
}

impl<C> Batched<C> {
    /// Bind a credential-neutral generated client to its credential source.
    ///
    /// # Errors
    /// Refuses ambient credentials, invalid descriptors, and batch RPCs that
    /// do not declare `NO_SIDE_EFFECTS`.
    pub fn new(inner: C, source: impl CredentialSource) -> Result<Self, connectrpc::ConnectError>
    where
        C: CredentialNeutralClient,
    {
        let headers = inner.batch_client_config().default_headers();
        if headers.contains_key("authorization")
            || headers.contains_key("brefwiz-relayed-credential")
        {
            return Err(connectrpc::ConnectError::invalid_argument(
                "batch client credentials must come from its credential source",
            ));
        }
        let methods = descriptor::batch_methods(
            inner.batch_client_descriptor(),
            inner.batch_client_service(),
        )?;
        Ok(Self {
            inner: Arc::new(inner),
            wire: Arc::new(Wire::new(Arc::new(source), None)),
            coalescers: Arc::new(Mutex::new(HashMap::new())),
            methods: Arc::new(methods),
            revisions: api_bones_page::RevisionCache::default(),
        })
    }

    /// Construct a public-read client with no credential.
    ///
    /// # Errors
    /// Refuses ambient credentials, invalid descriptors, and batch RPCs that
    /// do not declare `NO_SIDE_EFFECTS`.
    pub fn anonymous(inner: C) -> Result<Self, connectrpc::ConnectError>
    where
        C: CredentialNeutralClient,
    {
        Self::new(inner, Anonymous)
    }

    /// Tell `observer` about every wire batch request when it ends.
    #[must_use]
    pub fn observed(self, observer: Observer) -> Self {
        let credentials = Arc::clone(&self.wire.credentials);
        self.rewired(Wire::new(credentials, Some(observer)))
    }

    /// The unwrapped client, for single calls the runtime does not touch.
    #[must_use]
    pub fn raw(&self) -> &C {
        &self.inner
    }

    fn rewired(self, wire: Wire) -> Self {
        Self {
            inner: self.inner,
            wire: Arc::new(wire),
            coalescers: Arc::new(Mutex::new(HashMap::new())),
            methods: self.methods,
            revisions: api_bones_page::RevisionCache::default(),
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub fn __inner(&self) -> Arc<C> {
        Arc::clone(&self.inner)
    }

    #[doc(hidden)]
    #[must_use]
    pub fn __wire(&self) -> Arc<Wire> {
        Arc::clone(&self.wire)
    }

    /// Check generated glue against the descriptor validated at construction.
    ///
    /// # Errors
    /// Refuses a method pair absent from the client's declared batch relations.
    #[doc(hidden)]
    pub fn __validate_method(
        &self,
        single: &str,
        batch: &str,
    ) -> Result<(), connectrpc::ConnectError> {
        if self
            .methods
            .get(batch.trim_start_matches('/'))
            .is_some_and(|declared| declared == single.trim_start_matches('/'))
        {
            Ok(())
        } else {
            Err(connectrpc::ConnectError::invalid_argument(
                "batch method does not match its declared batch relation",
            ))
        }
    }

    /// The coalescer registered under `method`, made by `make` on first use.
    #[doc(hidden)]
    pub fn __coalescer<T: Any + Send + Sync>(
        &self,
        method: &'static str,
        make: impl FnOnce(Arc<Wire>) -> T,
    ) -> Arc<T> {
        let mut map = self
            .coalescers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = map
            .get(method)
            .and_then(|existing| Arc::clone(existing).downcast::<T>().ok())
        {
            return found;
        }
        let coalescer = Arc::new(make(Arc::clone(&self.wire)));
        map.insert(method, Arc::clone(&coalescer) as Arc<dyn Any + Send + Sync>);
        coalescer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime<C>(inner: C) -> Batched<C> {
        Batched {
            inner: Arc::new(inner),
            wire: Arc::new(Wire::new(Arc::new(Anonymous), None)),
            coalescers: Arc::new(Mutex::new(HashMap::new())),
            methods: Arc::new(descriptor::Methods::new()),
            revisions: api_bones_page::RevisionCache::default(),
        }
    }

    #[test]
    fn a_coalescer_is_made_once_per_method_and_shared_by_clones() {
        let batched = runtime(());
        let first = batched.__coalescer("p.S/Get", |_| 7_u32);
        let clone = batched.clone();
        let again = clone.__coalescer("p.S/Get", |_| 9_u32);
        assert_eq!(*first, 7);
        assert!(Arc::ptr_eq(&first, &again));
        assert_eq!(*batched.__coalescer("p.S/Other", |_| 3_u32), 3);
    }

    #[test]
    fn new_wiring_starts_fresh_coalescers() {
        let batched = runtime(());
        let before = batched.__coalescer("p.S/Get", |_| 1_u32);
        let rewired = batched.observed(Arc::new(|_| {}));
        let after = rewired.__coalescer("p.S/Get", |_| 2_u32);
        assert_eq!((*before, *after), (1, 2));
    }

    #[test]
    fn the_raw_client_stays_reachable() {
        assert_eq!(*runtime(5_u8).raw(), 5);
    }

    #[test]
    fn observed_keeps_the_credential_source() {
        let seen = Arc::new(Mutex::new(0_usize));
        let sink = Arc::clone(&seen);
        let observer: Observer = Arc::new(move |_| *sink.lock().expect("sink") += 1);
        let batched = runtime(()).observed(observer);
        assert!(batched.__wire().observer.is_some());
        assert!(batched.__wire().credentials.current().is_ok());
    }
}
