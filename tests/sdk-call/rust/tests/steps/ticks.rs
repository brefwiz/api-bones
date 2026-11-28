// SPDX-License-Identifier: LicenseRef-Brefwiz-Proprietary
//! The tick stream both lanes' `EventsService` servers answer `Watch` with.

use std::sync::Arc;
use std::time::Duration;

use api_bones_contract_rust::{EventsService, Tick as WireTick, WatchRequest, WatchResponse};
use connectrpc::{
    ConnectError, Encodable, RequestContext, Response, ServiceRequest, ServiceResult, ServiceStream,
};
use futures_util::stream;

/// The state a lane's server shares with its `Watch` handler: how a request is
/// admitted and how the ticks are paced.
pub trait Watching: Send + Sync + 'static {
    /// Records the request, or refuses it as the lane's script says.
    fn admit(&self, ctx: &RequestContext) -> Result<(), ConnectError>;

    /// The delay between ticks and whether the second tick is a refusal.
    fn pacing(&self) -> (Duration, bool);
}

/// The `EventsService` of a lane's server.
pub struct EventsServer<S>(pub Arc<S>);

impl<S> Clone for EventsServer<S> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<S: Watching> EventsService for EventsServer<S> {
    async fn watch(
        &self,
        ctx: RequestContext,
        request: ServiceRequest<'_, WatchRequest>,
    ) -> ServiceResult<ServiceStream<impl Encodable<WatchResponse> + Send + use<S>>> {
        let count = request.to_owned_message().count.unwrap_or_default();
        S::admit(&self.0, &ctx)?;
        let (every, refuse_after_first) = self.0.pacing();
        ticks(count, every, refuse_after_first)
    }
}

/// `count` ticks, one every `every`. With `refuse_after_first`, the second
/// tick is replaced by a refusal that ends the stream: nothing follows it.
pub fn ticks(
    count: u32,
    every: Duration,
    refuse_after_first: bool,
) -> ServiceResult<ServiceStream<impl Encodable<WatchResponse> + Send + use<>>> {
    Response::stream_ok(stream::unfold(0_u32, move |next| async move {
        if next >= count {
            return None;
        }
        tokio::time::sleep(every).await;
        if refuse_after_first && next == 1 {
            let refused: Result<WatchResponse, ConnectError> =
                Err(ConnectError::unauthenticated("the caller is not signed in"));
            return Some((refused, count));
        }
        let tick: Result<WatchResponse, ConnectError> = Ok(WatchResponse {
            tick: WireTick {
                sequence: next,
                ..Default::default()
            }
            .into(),
            ..Default::default()
        });
        Some((tick, next + 1))
    }))
}
