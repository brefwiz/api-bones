// SPDX-License-Identifier: MIT
//! Same-tick coalescing of single Gets into one batch request.
//!
//! A call opens a window keyed on the single method, the request with its id
//! cleared, every effective header and the caller identity. Calls with the
//! same key join it until a caller observes no growth across polls, when it
//! flushes: one distinct id calls the single RPC, several call the batch RPC.
//! There is no timer. The flush runs on a spawned task, so dropping a caller
//! never cancels the request the others wait on.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use connectrpc::ConnectError;
use connectrpc::client::CallOptions;
use futures_util::future::join_all;
use tokio::sync::oneshot;

use crate::chunk::{MAX_ITEMS, MAX_REQUEST_BYTES, element_bytes};
use crate::item_error;
use crate::key::{CallHeaders, CoalesceKey, CredentialGuard, QueueKey, digest, header_hash};
use crate::results::BatchResults;
use crate::wire::{BatchFn, Boxed, ChunkCall, Explicit, Wire};

/// Sends one single Get.
pub type SingleFn<Get, T> =
    Arc<dyn Fn(Get, CallOptions) -> Boxed<Result<T, ConnectError>> + Send + Sync>;

/// How one `Get<Resource>` and its `BatchGet<Resources>` sibling are called.
pub struct GetPlan<Get, Tmpl, T> {
    /// The single method's full name.
    pub single_method: &'static str,
    /// The batch method's full name.
    pub batch_method: &'static str,
    /// Calls the single RPC.
    pub single: SingleFn<Get, T>,
    /// Calls the batch RPC.
    pub batch: BatchFn<Tmpl, T>,
    /// The id a Get asks for.
    pub id_of: fn(&Get) -> String,
    /// Set the id of a Get.
    pub set_id: fn(&mut Get, &str),
    /// The binary encoding of a Get with its id cleared.
    pub encode_cleared: fn(&Get) -> Vec<u8>,
    /// The batch request, with no ids, that carries a Get's other fields.
    pub to_template: fn(&Get) -> Tmpl,
    /// The encoded size of a batch request template with no ids.
    pub template_bytes: fn(&Tmpl) -> usize,
}

struct Window<Get, T> {
    serial: u64,
    turn: Arc<Turn>,
    template: Get,
    headers: CallHeaders,
    queue: QueueKey,
    guard: CredentialGuard,
    bytes: usize,
    ids: Vec<String>,
    waiters: HashMap<String, Vec<oneshot::Sender<Result<T, ConnectError>>>>,
    calls: usize,
    opened: Instant,
}

#[derive(Default)]
struct Turn {
    callers: AtomicUsize,
    #[cfg(test)]
    after_departure: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

struct Waiting<T, F: FnOnce()> {
    receiver: oneshot::Receiver<Result<T, ConnectError>>,
    close: Option<F>,
}

struct PendingGet<T> {
    key: CoalesceKey,
    serial: u64,
    observed_ids: usize,
    turn: Arc<Turn>,
    receiver: oneshot::Receiver<Result<T, ConnectError>>,
}

impl<T, F: FnOnce()> Drop for Waiting<T, F> {
    fn drop(&mut self) {
        self.receiver.close();
        if let Some(close) = self.close.take() {
            close();
        }
    }
}

/// The coalescer of one single method on one client.
pub struct Coalescer<Get, Tmpl, T> {
    plan: GetPlan<Get, Tmpl, T>,
    wire: Arc<Wire>,
    windows: Mutex<HashMap<CoalesceKey, Window<Get, T>>>,
    serial: AtomicU64,
}

impl<Get, Tmpl, T> Coalescer<Get, Tmpl, T>
where
    Get: Clone + Send + 'static,
    Tmpl: Clone + Send + 'static,
    T: Clone + Send + 'static,
{
    /// A coalescer over `plan`, sharing `wire` with the client's other methods.
    #[must_use]
    pub fn new(plan: GetPlan<Get, Tmpl, T>, wire: Arc<Wire>) -> Self {
        Self {
            plan,
            wire,
            windows: Mutex::new(HashMap::new()),
            serial: AtomicU64::new(0),
        }
    }

    /// One Get, merged with the same-key Gets of this tick.
    ///
    /// # Errors
    /// The error the single Get would raise, or the batch's whole-chunk error.
    pub async fn get(
        self: &Arc<Self>,
        request: Get,
        headers: CallHeaders,
    ) -> Result<T, ConnectError> {
        let PendingGet {
            key,
            serial,
            mut observed_ids,
            turn,
            receiver,
        } = self.enqueue(request, headers)?;
        let mut waiting = Waiting {
            receiver,
            close: Some(|| self.leave_window(&key, serial, &turn)),
        };
        loop {
            // Growth yields again so a parent's polling budget cannot close
            // the window before it registers the remaining siblings.
            tokio::task::yield_now().await;
            let mut grew = false;
            if let Some(window) = self.take_window(&key, serial, |window| {
                grew = observed_ids != window.ids.len();
                observed_ids = window.ids.len();
                !grew
            }) {
                tokio::spawn(Arc::clone(self).run_window(window));
            }
            if !grew {
                break;
            }
        }
        (&mut waiting.receiver)
            .await
            .unwrap_or_else(|_| Err(item_error::abandoned()))
    }

    fn enqueue(
        self: &Arc<Self>,
        request: Get,
        headers: CallHeaders,
    ) -> Result<PendingGet<T>, ConnectError> {
        let credential = self
            .wire
            .credentials
            .current()
            .map_err(|_| item_error::identity_changed())?;
        let guard = CredentialGuard::capture(&credential)?;
        headers.validate(&credential.headers)?;
        let id = (self.plan.id_of)(&request);
        let mut effective = credential.headers.clone();
        effective.extend(headers.effective());
        crate::key::validate_credentials(&effective)?;
        let key = CoalesceKey {
            method: self.plan.single_method,
            request: digest(&(self.plan.encode_cleared)(&request)),
            headers: header_hash(&effective),
            identity: credential.identity.clone(),
        };
        let template_bytes = (self.plan.template_bytes)(&(self.plan.to_template)(&request));
        let cost = element_bytes(&id);
        let (sender, receiver) = oneshot::channel();
        let mut flush = Vec::new();
        let (serial, observed_ids, turn) = {
            let mut windows = self
                .windows
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let full = windows.get(&key).is_some_and(|w| {
                !w.waiters.contains_key(&id)
                    && (w.ids.len() >= MAX_ITEMS || w.bytes + cost > MAX_REQUEST_BYTES)
            });
            if full {
                flush.extend(windows.remove(&key));
            }
            let opening = !windows.contains_key(&key);
            if opening {
                let serial = self.serial.fetch_add(1, Ordering::Relaxed);
                windows.insert(
                    key.clone(),
                    Window {
                        serial,
                        turn: Arc::default(),
                        template: request,
                        headers,
                        queue: QueueKey::of(&credential),
                        guard,
                        bytes: template_bytes,
                        ids: Vec::new(),
                        waiters: HashMap::new(),
                        calls: 0,
                        opened: Instant::now(),
                    },
                );
            }
            if let Some(window) = windows.get_mut(&key) {
                if !window.waiters.contains_key(&id) {
                    window.ids.push(id.clone());
                    window.bytes += cost;
                }
                window.waiters.entry(id).or_default().push(sender);
                window.calls += 1;
                window.turn.callers.fetch_add(1, Ordering::Relaxed);
                let registered = (window.serial, window.ids.len(), Arc::clone(&window.turn));
                if window.ids.len() >= MAX_ITEMS {
                    flush.extend(windows.remove(&key));
                }
                registered
            } else {
                return Err(item_error::abandoned());
            }
        };
        for window in flush {
            tokio::spawn(Arc::clone(self).run_window(window));
        }
        Ok(PendingGet {
            key,
            serial,
            observed_ids,
            turn,
            receiver,
        })
    }

    fn leave_window(&self, key: &CoalesceKey, serial: u64, turn: &Turn) {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Registration and the last departure share this lock: a joining
        // caller either keeps this window live or opens its replacement.
        let last = turn.callers.fetch_sub(1, Ordering::AcqRel) == 1;
        #[cfg(test)]
        {
            let hook = turn.after_departure.lock().expect("departure hook").take();
            if let Some(hook) = hook {
                hook();
            }
        }
        if last
            && windows
                .get(key)
                .is_some_and(|window| window.serial == serial)
        {
            windows.remove(key);
        }
    }

    fn take_window(
        &self,
        key: &CoalesceKey,
        serial: u64,
        ready: impl FnOnce(&Window<Get, T>) -> bool,
    ) -> Option<Window<Get, T>> {
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if windows
            .get(key)
            .is_some_and(|window| window.serial == serial && ready(window))
        {
            windows.remove(key)
        } else {
            None
        }
    }

    /// An explicit batch: one outcome per input id, in input order.
    pub async fn explicit(
        &self,
        mut template: Tmpl,
        ids: &[String],
        headers: &CallHeaders,
        template_bytes: usize,
    ) -> BatchResults<T> {
        let _ = &mut template;
        let explicit = Explicit {
            rpc: self.plan.batch_method,
            single: self.plan.single_method,
            batch: &self.plan.batch,
            template_bytes,
        };
        BatchResults::new(
            self.wire
                .run_explicit(&explicit, template, ids, headers)
                .await
                .0,
        )
    }

    async fn run_window(self: Arc<Self>, window: Window<Get, T>) {
        let Window {
            template,
            headers,
            queue,
            guard,
            bytes,
            ids,
            mut waiters,
            calls,
            opened,
            ..
        } = window;
        for senders in waiters.values_mut() {
            senders.retain(|sender| !sender.is_closed());
        }
        waiters.retain(|_, senders| !senders.is_empty());
        let ids: Vec<String> = ids
            .into_iter()
            .filter(|id| waiters.contains_key(id))
            .collect();
        if ids.is_empty() {
            return;
        }
        let call = ChunkCall {
            guard: &guard,
            queue: &queue,
            rpc: self.plan.batch_method,
            single: self.plan.single_method,
            requested: calls,
            calls,
            chunk_index: 0,
            chunks: 1,
            bytes,
            opened,
        };
        let work = self.send(&call, template, &headers, &ids);
        let everyone_left = async {
            let gone: Vec<_> = waiters
                .values_mut()
                .flatten()
                .map(oneshot::Sender::closed)
                .collect();
            join_all(gone).await;
        };
        let answers = tokio::select! {
            answers = work => answers,
            () = everyone_left => {
                return;
            }
        };
        for (id, answer) in ids.iter().zip(answers) {
            for sender in waiters.remove(id).unwrap_or_default() {
                let _ = sender.send(answer.clone());
            }
        }
    }

    /// The answer for each id: the single RPC for one id, the batch RPC for several.
    async fn send(
        &self,
        call: &ChunkCall<'_>,
        template: Get,
        headers: &CallHeaders,
        ids: &[String],
    ) -> Vec<Result<T, ConnectError>> {
        let fail = |error: ConnectError| ids.iter().map(|_| Err(error.clone())).collect();
        if let [only] = ids {
            let _slot = match self.wire.admission.acquire(call.queue).await {
                Ok(slot) => slot,
                Err(error) => return fail(error),
            };
            let credential = match self.wire.verify(call.guard) {
                Ok(credential) => credential,
                Err(error) => return fail(error),
            };
            let mut request = template;
            if let Err(error) = headers.validate(&credential.headers) {
                return fail(error);
            }
            (self.plan.set_id)(&mut request, only);
            let options = match headers.call_options(&credential.headers) {
                Ok(options) => options,
                Err(error) => return fail(error),
            };
            let answer = (self.plan.single)(request, options).await;
            if self.wire.verify(call.guard).is_err() {
                return fail(item_error::identity_changed());
            }
            return vec![answer];
        }
        let batch_template = (self.plan.to_template)(&template);
        match self
            .wire
            .send_chunk(
                call,
                &self.plan.batch,
                batch_template,
                ids.to_vec(),
                headers,
            )
            .await
        {
            Ok(answers) => answers,
            Err(error) => fail(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::sync::Barrier;
    use std::task::Context;
    use std::time::Duration;

    use super::*;
    use crate::key::Anonymous;
    use crate::span::BatchRecord;

    struct Harness {
        coalescer: Arc<Coalescer<String, (), String>>,
        singles: Arc<AtomicUsize>,
        batches: Arc<Mutex<Vec<Vec<String>>>>,
        records: Arc<Mutex<Vec<BatchRecord>>>,
    }

    impl Harness {
        fn new(fail_first: bool) -> Self {
            let singles = Arc::new(AtomicUsize::new(0));
            let single_calls = Arc::clone(&singles);
            let batches = Arc::new(Mutex::new(Vec::new()));
            let batch_calls = Arc::clone(&batches);
            let records = Arc::new(Mutex::new(Vec::new()));
            let observed = Arc::clone(&records);
            let wire = Wire::new(
                Arc::new(Anonymous),
                Some(Arc::new(move |record| {
                    observed.lock().expect("records").push(record.clone());
                })),
            );
            let plan = GetPlan {
                single_method: "/contract.Widgets/GetWidget",
                batch_method: "/contract.Widgets/BatchGetWidgets",
                single: Arc::new(move |id, _| {
                    single_calls.fetch_add(1, Ordering::SeqCst);
                    Box::pin(async move { Ok(id) })
                }),
                batch: Arc::new(move |(), ids, _| {
                    let failed = {
                        let mut calls = batch_calls.lock().expect("batch calls");
                        calls.push(ids.clone());
                        fail_first && calls.len() == 1
                    };
                    Box::pin(async move {
                        if failed {
                            Err(ConnectError::unavailable("first wire attempt failed"))
                        } else {
                            Ok(ids.into_iter().map(|id| (id.clone(), Ok(id))).collect())
                        }
                    })
                }),
                id_of: Clone::clone,
                set_id: |request, id| id.clone_into(request),
                encode_cleared: |_| Vec::new(),
                to_template: |_| (),
                template_bytes: |()| 0,
            };
            Self {
                coalescer: Arc::new(Coalescer::new(plan, Arc::new(wire))),
                singles,
                batches,
                records,
            }
        }

        fn assert_batches(&self, expected: &[&[&str]]) {
            assert_eq!(self.singles.load(Ordering::SeqCst), 0);
            assert_eq!(*self.batches.lock().expect("batch calls"), expected);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn same_tick_gets_merge_and_retry_only_the_flushed_batch() {
        let harness = Harness::new(true);
        let started = tokio::time::Instant::now();
        let answers = join_all(
            ["w1", "w2", "w1"].map(|id| harness.coalescer.get(id.into(), CallHeaders::new())),
        )
        .await;
        assert_eq!(
            answers
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .expect("retried batch"),
            ["w1", "w2", "w1"]
        );
        harness.assert_batches(&[&["w1", "w2"], &["w1", "w2"]]);
        assert_eq!(started.elapsed(), Duration::from_millis(50));
        assert_eq!(harness.records.lock().expect("records").len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn combinator_poll_budgets_preserve_full_and_partial_windows() {
        for count in [50, 250] {
            for lead_separate in [false, true] {
                let harness = Harness::new(false);
                let ids: Vec<_> = (0..count).map(|index| format!("w{index}")).collect();
                let mut gets = ids
                    .iter()
                    .map(|id| harness.coalescer.get(id.clone(), CallHeaders::new()));
                let answers = if lead_separate {
                    let lead = gets.next().expect("lead get");
                    let (lead, rest) = futures_util::future::join(lead, join_all(gets)).await;
                    std::iter::once(lead).chain(rest).collect::<Vec<_>>()
                } else {
                    join_all(gets).await
                };
                assert_eq!(
                    answers
                        .into_iter()
                        .collect::<Result<Vec<_>, _>>()
                        .expect("all gets"),
                    ids,
                    "count={count}, lead_separate={lead_separate}"
                );
                let mut sizes: Vec<_> = harness
                    .batches
                    .lock()
                    .expect("batch calls")
                    .iter()
                    .map(Vec::len)
                    .collect();
                sizes.sort_unstable();
                let expected = if count <= MAX_ITEMS {
                    vec![count]
                } else {
                    vec![50, 200]
                };
                assert_eq!(
                    sizes, expected,
                    "count={count}, lead_separate={lead_separate}"
                );
                assert_eq!(harness.singles.load(Ordering::SeqCst), 0);
            }
        }
    }

    #[test]
    fn first_poll_does_not_spawn_a_detached_flush() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_time()
            .build()
            .expect("runtime");
        let harness = Harness::new(false);
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = Arc::clone(&barrier);
        let worker = runtime.spawn(async move {
            worker_barrier.wait();
            worker_barrier.wait();
        });
        barrier.wait();
        let entered = runtime.enter();
        let mut first = Box::pin(harness.coalescer.get("w1".into(), CallHeaders::new()));
        let mut context = Context::from_waker(std::task::Waker::noop());
        assert!(first.as_mut().poll(&mut context).is_pending());
        // The only task is the blocked worker. A detached flush would be a
        // second task, able to race this parent's remaining registrations.
        let tasks = runtime.metrics().num_alive_tasks();
        barrier.wait();
        runtime.block_on(worker).expect("worker completed");
        assert_eq!(tasks, 1);
        let mut second = Box::pin(harness.coalescer.get("w2".into(), CallHeaders::new()));
        assert!(second.as_mut().poll(&mut context).is_pending());
        drop(entered);
        let (first, second) = runtime.block_on(futures_util::future::join(first, second));
        assert_eq!(first.expect("first"), "w1");
        assert_eq!(second.expect("second"), "w2");
        harness.assert_batches(&[&["w1", "w2"]]);
    }

    #[tokio::test]
    async fn dropping_the_opener_transfers_flush_to_a_surviving_callers_poll() {
        let harness = Harness::new(false);
        let mut first = Box::pin(
            harness
                .coalescer
                .get("cancelled".into(), CallHeaders::new()),
        );
        let mut second = Box::pin(harness.coalescer.get("w2".into(), CallHeaders::new()));
        let mut context = Context::from_waker(std::task::Waker::noop());
        assert!(first.as_mut().poll(&mut context).is_pending());
        assert!(second.as_mut().poll(&mut context).is_pending());
        drop(first);
        let third = harness.coalescer.get("w3".into(), CallHeaders::new());
        let mut third = Box::pin(third);
        assert!(third.as_mut().poll(&mut context).is_pending());
        assert!(second.as_mut().poll(&mut context).is_pending());
        let (second, third) = futures_util::future::join(second, third).await;
        assert_eq!(second.expect("second"), "w2");
        assert_eq!(third.expect("third"), "w3");
        harness.assert_batches(&[&["w2", "w3"]]);
    }

    #[tokio::test]
    async fn a_sibling_flushes_while_the_opener_stays_alive_and_unpolled() {
        let harness = Harness::new(false);
        let mut first = Box::pin(harness.coalescer.get("w1".into(), CallHeaders::new()));
        let mut second = Box::pin(harness.coalescer.get("w2".into(), CallHeaders::new()));
        let mut context = Context::from_waker(std::task::Waker::noop());
        assert!(first.as_mut().poll(&mut context).is_pending());
        assert!(second.as_mut().poll(&mut context).is_pending());
        assert_eq!(second.await.expect("sibling completed"), "w2");
        harness.assert_batches(&[&["w1", "w2"]]);
        assert_eq!(first.await.expect("opener completed"), "w1");
        harness.assert_batches(&[&["w1", "w2"]]);
    }

    #[tokio::test]
    async fn joining_during_last_departure_never_abandons_the_receiver() {
        let harness = Harness::new(false);
        let mut first = harness
            .coalescer
            .enqueue("gone".into(), CallHeaders::new())
            .expect("opening registration");
        first.receiver.close();
        let timeout = std::time::Duration::from_secs(10);
        let (start, started) = std::sync::mpsc::channel();
        let (progress, progressed) = std::sync::mpsc::channel();
        let (complete, completed) = std::sync::mpsc::channel();
        let coalescer = Arc::clone(&harness.coalescer);
        let joiner = std::thread::spawn(move || {
            started
                .recv_timeout(timeout)
                .expect("departure decremented within timeout");
            let unlocked = match coalescer.windows.try_lock() {
                Ok(windows) => {
                    drop(windows);
                    true
                }
                Err(std::sync::TryLockError::WouldBlock) => false,
                Err(std::sync::TryLockError::Poisoned(_)) => panic!("windows poisoned"),
            };
            progress.send(unlocked).expect("lock probe delivered");
            let pending = coalescer
                .enqueue("live".into(), CallHeaders::new())
                .expect("joining registration");
            if unlocked {
                progress.send(true).expect("registration completed");
            }
            complete.send(pending).expect("joiner result delivered");
        });
        *first.turn.after_departure.lock().expect("departure hook") = Some(Box::new(move || {
            start.send(()).expect("joiner started");
            // An unlocked window permits the joining registration to finish
            // before removal. A held lock defers it until departure returns.
            if progressed
                .recv_timeout(timeout)
                .expect("lock probe within timeout")
            {
                assert!(
                    progressed
                        .recv_timeout(timeout)
                        .expect("joining registration finished within timeout")
                );
            }
        }));
        harness
            .coalescer
            .leave_window(&first.key, first.serial, &first.turn);
        let second = completed
            .recv_timeout(timeout)
            .expect("joiner completed within timeout");
        joiner.join().expect("joiner completed");
        if let Some(window) = harness
            .coalescer
            .take_window(&second.key, second.serial, |_| true)
        {
            Arc::clone(&harness.coalescer).run_window(window).await;
        }
        assert_eq!(
            second
                .receiver
                .await
                .expect("joining receiver retained")
                .expect("joining result"),
            "live"
        );
        harness
            .coalescer
            .leave_window(&second.key, second.serial, &second.turn);
        assert_eq!(harness.singles.load(Ordering::SeqCst), 1);
        assert!(harness.batches.lock().expect("batches").is_empty());
    }

    #[tokio::test]
    async fn joining_and_last_departure_preserve_both_registration_orders() {
        for join_first in [false, true] {
            let harness = Harness::new(false);
            let mut first = harness
                .coalescer
                .enqueue("gone".into(), CallHeaders::new())
                .expect("opening registration");
            first.receiver.close();
            let second = if join_first {
                let second = harness
                    .coalescer
                    .enqueue("live".into(), CallHeaders::new())
                    .expect("joined registration");
                harness
                    .coalescer
                    .leave_window(&first.key, first.serial, &first.turn);
                assert_eq!(second.serial, first.serial);
                second
            } else {
                harness
                    .coalescer
                    .leave_window(&first.key, first.serial, &first.turn);
                let second = harness
                    .coalescer
                    .enqueue("live".into(), CallHeaders::new())
                    .expect("fresh registration");
                assert_ne!(second.serial, first.serial);
                second
            };
            let window = harness
                .coalescer
                .take_window(&second.key, second.serial, |_| true)
                .expect("live window retained");
            Arc::clone(&harness.coalescer).run_window(window).await;
            assert_eq!(
                second
                    .receiver
                    .await
                    .expect("answer delivered")
                    .expect("live result"),
                "live"
            );
            harness
                .coalescer
                .leave_window(&second.key, second.serial, &second.turn);
            assert_eq!(harness.singles.load(Ordering::SeqCst), 1);
            assert!(harness.batches.lock().expect("batches").is_empty());
        }
    }

    #[tokio::test]
    async fn dropping_every_caller_removes_the_unsent_window() {
        let harness = Harness::new(false);
        let mut first = Box::pin(harness.coalescer.get("w1".into(), CallHeaders::new()));
        let mut context = Context::from_waker(std::task::Waker::noop());
        assert!(first.as_mut().poll(&mut context).is_pending());
        drop(first);
        assert!(
            harness
                .coalescer
                .windows
                .lock()
                .expect("windows")
                .is_empty()
        );
        harness.assert_batches(&[]);
    }
}
