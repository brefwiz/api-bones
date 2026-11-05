// SPDX-License-Identifier: MIT
//! Admission of wire requests: at most [`MAX_WIRE_IN_FLIGHT`] in flight per
//! client, granted round-robin across identities, and at most
//! [`MAX_QUEUED_CHUNKS`] waiting per identity.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use connectrpc::ConnectError;
use tokio::sync::oneshot;

use crate::item_error;
use crate::key::QueueKey;

/// Wire batch requests in flight per client instance.
pub const MAX_WIRE_IN_FLIGHT: usize = 4;
/// Unsent chunks queued per key identity.
pub const MAX_QUEUED_CHUNKS: usize = 64;

#[derive(Default)]
struct State {
    in_flight: usize,
    queues: HashMap<QueueKey, VecDeque<oneshot::Sender<Permit>>>,
    /// Identities with a waiting chunk, in the order they are served next.
    ring: VecDeque<QueueKey>,
}

/// Grants wire slots.
#[derive(Clone, Default)]
pub struct Admission {
    state: Arc<Mutex<State>>,
}

/// One wire slot, released on drop.
pub struct Permit {
    state: Option<Arc<Mutex<State>>>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            let mut guard = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.in_flight = guard.in_flight.saturating_sub(1);
            dispatch(&state, &mut guard);
        }
    }
}

/// Hand free slots to waiting identities, one at a time round-robin.
fn dispatch(shared: &Arc<Mutex<State>>, state: &mut State) {
    while state.in_flight < MAX_WIRE_IN_FLIGHT {
        let Some(key) = state.ring.pop_front() else {
            return;
        };
        let Some(queue) = state.queues.get_mut(&key) else {
            continue;
        };
        while let Some(waiter) = queue.pop_front() {
            if waiter.is_closed() {
                continue;
            }
            let permit = Permit {
                state: Some(shared.clone()),
            };
            state.in_flight += 1;
            if let Err(mut returned) = waiter.send(permit) {
                // The waiter went away between the check and the send: take
                // the slot back without running the release path under the lock.
                returned.state = None;
                state.in_flight -= 1;
                continue;
            }
            break;
        }
        if queue.is_empty() {
            state.queues.remove(&key);
        } else {
            state.ring.push_back(key);
        }
    }
}

impl Admission {
    /// Wait for a wire slot for `key`.
    ///
    /// # Errors
    /// `resource_exhausted` when `key` already has [`MAX_QUEUED_CHUNKS`]
    /// chunks waiting. Only that identity is refused.
    pub async fn acquire(&self, key: &QueueKey) -> Result<Permit, ConnectError> {
        let receiver = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state.in_flight < MAX_WIRE_IN_FLIGHT && state.ring.is_empty() {
                state.in_flight += 1;
                return Ok(Permit {
                    state: Some(self.state.clone()),
                });
            }
            let queue = state.queues.entry(key.clone()).or_default();
            queue.retain(|waiter| !waiter.is_closed());
            if queue.len() >= MAX_QUEUED_CHUNKS {
                return Err(item_error::queue_full());
            }
            let (sender, receiver) = oneshot::channel();
            queue.push_back(sender);
            if !state.ring.contains(key) {
                state.ring.push_back(key.clone());
            }
            receiver
        };
        receiver.await.map_err(|_| item_error::abandoned())
    }

    /// Wire requests in flight now.
    #[must_use]
    pub fn in_flight(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .in_flight
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::{Pin, pin};
    use std::task::{Context, Poll, Waker};

    use super::*;
    use crate::key::{Credential, Identity};

    fn key(subject: &str) -> QueueKey {
        QueueKey::of(&Credential {
            identity: Identity {
                subject: subject.to_owned(),
                org: "o".to_owned(),
            },
            headers: Vec::new(),
        })
    }

    fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }

    fn granted(result: Poll<Result<Permit, ConnectError>>) -> Permit {
        match result {
            Poll::Ready(Ok(permit)) => permit,
            _ => panic!("slot must be ready and granted"),
        }
    }

    fn fill_slots(admission: &Admission, identity: &QueueKey) -> Vec<Permit> {
        let mut held = Vec::new();
        for _ in 0..MAX_WIRE_IN_FLIGHT {
            held.push(granted(poll_once(pin!(admission.acquire(identity)))));
        }
        held
    }

    #[test]
    fn four_slots_then_a_queue() {
        let admission = Admission::default();
        let a = key("a");
        let mut held = fill_slots(&admission, &a);
        assert_eq!(admission.in_flight(), MAX_WIRE_IN_FLIGHT);
        let mut waiting = pin!(admission.acquire(&a));
        assert!(poll_once(waiting.as_mut()).is_pending());
        held.pop();
        let _permit = granted(poll_once(waiting.as_mut()));
    }

    #[test]
    fn overflow_refuses_only_the_identity_that_overflowed() {
        let admission = Admission::default();
        let (a, b) = (key("a"), key("b"));
        let held = fill_slots(&admission, &a);
        let mut queued = Vec::new();
        for _ in 0..MAX_QUEUED_CHUNKS {
            let mut waiting = Box::pin(admission.acquire(&a));
            assert!(poll_once(waiting.as_mut()).is_pending());
            queued.push(waiting);
        }
        let Poll::Ready(Err(overflow)) = poll_once(pin!(admission.acquire(&a))) else {
            panic!("overflow must be refused immediately");
        };
        assert_eq!(overflow.code, connectrpc::ErrorCode::ResourceExhausted);
        let mut other = pin!(admission.acquire(&b));
        assert!(
            poll_once(other.as_mut()).is_pending(),
            "b queues; it is not refused"
        );
        drop(held);
        let _permit = granted(poll_once(other.as_mut()));
    }

    #[test]
    fn slots_alternate_between_identities() {
        let admission = Admission::default();
        let (a, b) = (key("a"), key("b"));
        let mut held = fill_slots(&admission, &a);
        let mut waiting = Vec::new();
        for k in [&a, &a, &b, &b] {
            let mut future = Box::pin(admission.acquire(k));
            assert!(poll_once(future.as_mut()).is_pending());
            waiting.push(Some(future));
        }
        let mut granted_permits = Vec::new();
        for expected in [0, 2, 1, 3] {
            held.pop();
            for (index, future) in waiting.iter_mut().enumerate() {
                if let Some(future) = future {
                    let result = poll_once(future.as_mut());
                    if index == expected {
                        granted_permits.push(granted(result));
                    } else {
                        assert!(result.is_pending(), "unexpected grant at index {index}");
                    }
                }
            }
            waiting[expected] = None;
        }
    }
}
