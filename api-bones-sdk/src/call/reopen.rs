// SPDX-License-Identifier: MIT
//! A server stream's open, kept so that a refusal in the stream's first frame
//! is the open's refusal: renewed and replayed once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use api_bones_batch::Credential;
use api_bones_rt::{BoxStream, MaybeSend};
use connectrpc::ConnectError;
use futures_util::{Stream as FutureStream, StreamExt as _, stream};

use super::abort::AbortFlag;
use super::attempt::Attempt;
use super::identity::{Refreshed, WalkGuard};
use super::run::{Plan, Route, guarded, is_refused_credential};
use super::{CALL_BUDGET, CallEnv};
use crate::ctx::CallCtx;
use crate::error::Error;

/// What a stream's open ran under, and the open itself.
pub(super) struct Reopen<F> {
    pub(super) env: CallEnv,
    pub(super) route: Route,
    pub(super) ctx: CallCtx,
    pub(super) deadline: api_bones_rt::Instant,
    pub(super) retries: u8,
    pub(super) record: bool,
    pub(super) abort: Option<AbortFlag>,
    pub(super) walk: Arc<WalkGuard>,
    pub(super) open: F,
    /// The credential the latest send of the open carried.
    pub(super) carried: Mutex<Option<Credential>>,
    /// Whether the call's one renewal is used, by the open's own replay or by
    /// the first frame's.
    pub(super) spent: AtomicBool,
}

/// The frames of an open stream that are still to come.
type Frames<Item> = BoxStream<'static, Result<Item, ConnectError>>;

/// `error` as the caller's, after it told the host when it is a refusal.
fn told(env: &CallEnv, walk: &WalkGuard, error: ConnectError) -> Error {
    env.refused(&error, Some(walk), true);
    Error::from(error)
}

impl<F> Reopen<F> {
    /// One open, as the call's `Plan` runs it. A `renewed` credential is what
    /// the open carries, with the renewal already spent; its budget starts
    /// again, for the first frame arrives after the first open's has ended.
    pub(super) async fn open<S, Fut>(
        &mut self,
        renewed: Option<Credential>,
    ) -> Result<S, ConnectError>
    where
        F: FnMut(Attempt) -> Fut,
        Fut: Future<Output = Result<S, ConnectError>>,
    {
        let deadline = if renewed.is_some() {
            api_bones_rt::Instant::now() + CALL_BUDGET
        } else {
            self.deadline
        };
        Plan {
            env: &self.env,
            route: self.route,
            parent: &self.ctx,
            deadline,
            retries: self.retries,
            batch: false,
            stream: true,
            record: self.record,
            abort: self.abort.as_ref(),
            walk: Some(self.walk.as_ref()),
        }
        .run_with(&mut self.open, &self.carried, &self.spent, renewed)
        .await
    }

    /// The stream `items` as the caller sees it. Nothing is awaited until its
    /// first poll, so a quiet stream stalls only the `next()` that waits for
    /// its first item.
    ///
    /// A refusal as unauthenticated before the first item is the open's: the
    /// credential is renewed, together with every other call refused on it,
    /// and the open is replayed once. Anything that fails after the replay,
    /// or after an item, tells the host and is never replayed.
    pub(super) fn frames<Item, S, Fut>(self, items: S) -> BoxStream<'static, Result<Item, Error>>
    where
        Item: MaybeSend + 'static,
        S: FutureStream<Item = Result<Item, ConnectError>> + MaybeSend + 'static,
        F: FnMut(Attempt) -> Fut + MaybeSend + 'static,
        Fut: Future<Output = Result<S, ConnectError>> + MaybeSend + 'static,
    {
        Box::pin(
            stream::once(async move {
                let mut this = self;
                let (first, rest) = this.first_frame(items).await;
                let (env, walk) = (this.env, this.walk);
                let rest = rest.map(move |item| item.map_err(|error| told(&env, &walk, error)));
                stream::iter(first).chain(rest)
            })
            .flatten(),
        )
    }

    /// The first frame of the stream, replayed once when it refuses the
    /// credential, and the frames after it.
    async fn first_frame<Item, S, Fut>(
        &mut self,
        items: S,
    ) -> (Option<Result<Item, Error>>, Frames<Item>)
    where
        Item: MaybeSend + 'static,
        S: FutureStream<Item = Result<Item, ConnectError>> + MaybeSend + 'static,
        F: FnMut(Attempt) -> Fut + MaybeSend + 'static,
        Fut: Future<Output = Result<S, ConnectError>> + MaybeSend + 'static,
    {
        let mut items: Frames<Item> = Box::pin(items);
        let mut first = items.next().await;
        if let Some(Err(refused)) =
            first.take_if(|frame| frame.as_ref().err().is_some_and(is_refused_credential))
        {
            match self.renewed(refused).await {
                Ok(reopened) => {
                    items = Box::pin(reopened);
                    first = items.next().await;
                }
                Err(error) => {
                    let ended: Frames<Item> = Box::pin(stream::empty());
                    return (Some(Err(error)), ended);
                }
            }
        }
        let first = first.map(|frame| frame.map_err(|error| told(&self.env, &self.walk, error)));
        (first, items)
    }

    /// The open replayed with the credential the server's refusal asked to
    /// have renewed.
    ///
    /// # Errors
    /// The refusal that ended the call, already told to the host as far as
    /// the core tells it: a failed renewal is the source's own to report, a
    /// renewal with nothing newer is told once for every call refused on the
    /// credential, and an open the server refuses again tells the host itself.
    /// A renewal the open's own replay already used is not made again: the
    /// refusal is told once and ends the call. Reading the source and the
    /// renewal are bound by a fresh budget and the abort.
    async fn renewed<S, Fut>(&mut self, refused: ConnectError) -> Result<S, Error>
    where
        F: FnMut(Attempt) -> Fut,
        Fut: Future<Output = Result<S, ConnectError>>,
    {
        if self.spent.swap(true, Ordering::SeqCst) {
            return Err(told(&self.env, &self.walk, refused));
        }
        let budget = api_bones_rt::Instant::now() + CALL_BUDGET;
        let (env, walk, carried) = (&self.env, &*self.walk, &self.carried);
        let renewal = guarded(budget, self.abort.as_ref(), async {
            Ok(renewal(env, walk, carried, refused).await)
        })
        .await
        .map_err(|error| told(&self.env, &self.walk, error))?;
        self.open(Some(renewal?)).await.map_err(Error::from)
    }
}

/// The credential that replaces the refused one. A frame arrives only from an
/// open that was sent, so a send carried a credential and the walk captured
/// the identity it was sent as. It borrows the open's state and not the open,
/// so the future it returns asks nothing of the open's closure.
async fn renewal(
    env: &CallEnv,
    walk: &WalkGuard,
    carried: &Mutex<Option<Credential>>,
    refused: ConnectError,
) -> Result<Credential, Error> {
    let held = carried
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
        .expect("an open that answered carried a credential");
    let source = &*env.credentials;
    let identity =
        (walk.get(source, false).await).expect("an open that answered captured the identity");
    match env
        .flights
        .refresh(&identity, &env.credentials, &held)
        .await
    {
        Err(error) => {
            env.clear_slot(walk);
            Err(Error::from(error))
        }
        Ok(Refreshed::Ended { error, first }) => {
            let error = error.unwrap_or(refused);
            env.clear_slot(walk);
            env.refused(&error, Some(walk), first);
            Err(Error::from(error))
        }
        Ok(Refreshed::Replay(credential)) => Ok(credential),
    }
}
