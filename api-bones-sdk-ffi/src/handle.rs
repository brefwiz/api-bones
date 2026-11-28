// SPDX-License-Identifier: MIT
//! The handle a call returns when its result arrives over time.

use std::sync::Arc;

use api_bones_rt::BoxFuture;
use api_bones_sdk::{Error, Handle};

use crate::FfiError;

/// What a host does to a handle, whatever its item type.
trait Source: Send + Sync {
    fn next(&self) -> BoxFuture<'_, Result<Option<Vec<u8>>, Error>>;
    fn cancel(&self);
    fn span(&self) -> Option<String>;
}

/// A [`Handle`] with the encoding of its items.
struct Encoded<T, F> {
    handle: Handle<T>,
    encode: F,
}

impl<T, F> Source for Encoded<T, F>
where
    T: Send + 'static,
    F: Fn(&T) -> Vec<u8> + Send + Sync,
{
    fn next(&self) -> BoxFuture<'_, Result<Option<Vec<u8>>, Error>> {
        Box::pin(async move { Ok(self.handle.next().await?.map(|item| (self.encode)(&item))) })
    }

    fn cancel(&self) {
        self.handle.cancel();
    }

    fn span(&self) -> Option<String> {
        self.handle.span()
    }
}

/// A list, a stream or a batch, read with `next`, ended with `cancel`.
///
/// `next` gives the encoded `bones.sdk.v1.Page`, item message or
/// `bones.sdk.v1.BatchChunk`, `None` once the source ran out, and fails with
/// the `bones.sdk.v1.CallError` bytes of an [`FfiError`].
#[derive(uniffi::Object)]
pub struct FfiHandle {
    source: Arc<dyn Source>,
}

impl FfiHandle {
    /// A handle over `handle`, whose results cross as `encode` writes them.
    pub fn new<T, F>(handle: Handle<T>, encode: F) -> Arc<Self>
    where
        T: Send + 'static,
        F: Fn(&T) -> Vec<u8> + Send + Sync + 'static,
    {
        Arc::new(Self {
            source: Arc::new(Encoded { handle, encode }),
        })
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl FfiHandle {
    /// The next result; `None` once the source ran out or the handle was
    /// canceled.
    ///
    /// # Errors
    /// `HANDLE_BUSY` while another `next` is pending, `canceled` when this one
    /// was abandoned by `cancel`, or the source's own failure.
    pub async fn next(&self) -> Result<Option<Vec<u8>>, FfiError> {
        Ok(self.source.next().await?)
    }

    /// Ends the handle; idempotent.
    pub fn cancel(&self) {
        self.source.cancel();
    }

    /// The `traceparent` of the span covering the walk; none when it is not
    /// recording.
    #[must_use]
    pub fn span(&self) -> Option<String> {
        self.source.span()
    }
}

#[cfg(test)]
mod tests {
    use api_bones_sdk::__private::decode_call_error;
    use api_bones_sdk::{Attributes, CallCtx, SpanKind, Tracer};
    use futures_util::stream;

    use super::*;

    fn handle(items: Vec<Result<u8, Error>>) -> Arc<FfiHandle> {
        let span = Tracer::default().start(
            "walk",
            SpanKind::Internal,
            &CallCtx::default(),
            Attributes::new(),
        );
        FfiHandle::new(
            Handle::new(Box::pin(stream::iter(items)), span),
            |item: &u8| vec![*item, *item],
        )
    }

    #[tokio::test]
    async fn items_cross_encoded_and_the_end_is_none_as_often_as_asked() {
        let handle = handle(vec![Ok(1), Ok(2)]);
        assert_eq!(handle.next().await.expect("one"), Some(vec![1, 1]));
        assert_eq!(handle.next().await.expect("two"), Some(vec![2, 2]));
        assert_eq!(handle.next().await.expect("end"), None);
        assert_eq!(handle.next().await.expect("still the end"), None);
    }

    #[tokio::test]
    async fn a_source_failure_crosses_as_call_error_bytes() {
        let handle = handle(vec![Err(Error::handle_busy())]);
        let Err(FfiError::Call { bytes }) = handle.next().await else {
            panic!("the source failed");
        };
        let error = decode_call_error(&bytes).expect("decodes");
        assert_eq!(error.code, Error::handle_busy().code);
    }

    #[tokio::test]
    async fn a_canceled_handle_gives_none_and_cancel_is_idempotent() {
        let handle = handle(vec![Ok(1)]);
        handle.cancel();
        handle.cancel();
        assert_eq!(handle.next().await.expect("canceled"), None);
    }

    #[test]
    fn a_handle_that_is_not_recording_has_no_span() {
        assert_eq!(handle(Vec::new()).span(), None);
    }
}
