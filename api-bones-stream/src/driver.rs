// SPDX-License-Identifier: MIT
//! The async driver: [`RotationMachine`] over real streams and a tokio timer.

use std::collections::VecDeque;
use std::future::{Future, pending};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use futures_core::Stream;
use futures_util::{FutureExt, StreamExt, stream};
use tokio::time::{Instant, sleep_until};

use crate::{
    ControlSignal, EndCause, RotationAction, RotationEvent, RotationFrame, RotationMachine,
    STREAM_CONTROL_HEADER, STREAM_CONTROL_HEADER_VALUE, StreamControlled, StreamRole,
};

/// What [`rotating_stream`] hands to its `open` function each time it needs a
/// stream.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OpenRequest {
    /// The request header that declares the client reads control frames.
    pub header_name: &'static str,
    /// The value of that header.
    pub header_value: &'static str,
}

impl OpenRequest {
    const fn declaring() -> Self {
        Self {
            header_name: STREAM_CONTROL_HEADER,
            header_value: STREAM_CONTROL_HEADER_VALUE,
        }
    }
}

enum Leg<Fut, S> {
    Opening(Pin<Box<Fut>>),
    Streaming(S),
}

enum Event<M, E> {
    Message(M),
    Failed(E),
    Ended,
}

impl<Fut, S> Leg<Fut, S> {
    fn opening(fut: Fut) -> Self {
        Self::Opening(Box::pin(fut))
    }

    /// The next thing the leg produced. Cancel-safe: dropping the future loses
    /// nothing, and the leg resumes where it was.
    async fn next<M, E>(&mut self) -> Event<M, E>
    where
        Fut: Future<Output = Result<S, E>>,
        S: Stream<Item = Result<M, E>> + Unpin,
    {
        loop {
            match self {
                Self::Opening(open) => match open.as_mut().await {
                    Ok(stream) => *self = Self::Streaming(stream),
                    Err(error) => return Event::Failed(error),
                },
                Self::Streaming(stream) => {
                    return match stream.next().await {
                        Some(Ok(message)) => Event::Message(message),
                        Some(Err(error)) => Event::Failed(error),
                        None => Event::Ended,
                    };
                }
            }
        }
    }
}

async fn next_of<Fut, S, M, E>(leg: Option<&mut Leg<Fut, S>>) -> Event<M, E>
where
    Fut: Future<Output = Result<S, E>>,
    S: Stream<Item = Result<M, E>> + Unpin,
{
    match leg {
        Some(leg) => leg.next().await,
        None => pending().await,
    }
}

async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => sleep_until(at).await,
        None => pending().await,
    }
}

type Unseen<M> = Arc<Mutex<VecDeque<RotationFrame<M>>>>;

fn lock<M>(unseen: &Unseen<M>) -> MutexGuard<'_, VecDeque<RotationFrame<M>>> {
    unseen.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Split a message into its control and its application data. A message that
/// carried only a control has no data; one that carried both keeps its data
/// with the control removed. A control that cannot be read is reported as
/// such, and the message keeps whatever data it holds.
fn to_frame<M, K>(dedupe_key: &K, mut message: M) -> RotationFrame<M>
where
    M: StreamControlled,
    K: Fn(&M) -> Option<String>,
{
    let control = match message.take_control() {
        Ok(None) => None,
        Ok(Some(control)) => Some(ControlSignal::from(control)),
        Err(_) => Some(ControlSignal::Unreadable),
    };
    let data = if control.is_some() && message.is_empty() {
        None
    } else {
        let key = dedupe_key(&message);
        Some(RotationEvent {
            value: message,
            key,
        })
    };
    RotationFrame { control, data }
}

/// How the old stream ended while frames read ahead were still unseen.
enum OldEnd<E> {
    /// The old stream ended cleanly.
    Clean,
    /// The old stream ended with this failure.
    Failed(E),
}

struct Rotation<M, K, Open, Fut, S, E> {
    open: Open,
    dedupe_key: K,
    start: Instant,
    started: bool,
    machine: RotationMachine<M>,
    /// Old-stream frames read ahead of the machine, shared with the machine so
    /// it can deliver them before it closes the old stream.
    unseen: Unseen<M>,
    old: Option<Leg<Fut, S>>,
    replacement: Option<Leg<Fut, S>>,
    /// The old stream ended while frames read ahead were still unseen; the end
    /// waits until they have been shown.
    old_ended: Option<OldEnd<E>>,
    failure: Option<E>,
    out: VecDeque<Result<M, E>>,
    done: bool,
}

impl<M, K, Open, Fut, S, E> Rotation<M, K, Open, Fut, S, E>
where
    M: StreamControlled + Clone + Send + 'static,
    K: Fn(&M) -> Option<String>,
    Open: FnMut(OpenRequest) -> Fut,
    Fut: Future<Output = Result<S, E>>,
    S: Stream<Item = Result<M, E>> + Unpin,
{
    fn new(open: Open, dedupe_key: K) -> Self {
        let unseen: Unseen<M> = Arc::new(Mutex::new(VecDeque::new()));
        let shared = Arc::clone(&unseen);
        Self {
            open,
            dedupe_key,
            start: Instant::now(),
            started: false,
            machine: RotationMachine::with_buffered_old(move || lock(&shared).drain(..).collect()),
            unseen,
            old: None,
            replacement: None,
            old_ended: None,
            failure: None,
            out: VecDeque::new(),
            done: false,
        }
    }

    fn now(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    async fn next(&mut self) -> Option<Result<M, E>> {
        if !self.started {
            self.started = true;
            self.old = Some(Leg::opening((self.open)(OpenRequest::declaring())));
        }
        loop {
            if let Some(item) = self.out.pop_front() {
                return Some(item);
            }
            if self.done {
                return None;
            }
            let actions = self.step().await;
            self.apply(actions);
        }
    }

    async fn step(&mut self) -> Vec<RotationAction<M>> {
        let queued = lock(&self.unseen).pop_front();
        if let Some(frame) = queued {
            let now = self.now();
            return self.machine.on_frame(StreamRole::Old, frame, now);
        }
        if let Some(ended) = self.old_ended.take() {
            return self.end_old(match ended {
                OldEnd::Clean => None,
                OldEnd::Failed(error) => Some(error),
            });
        }
        let deadline = self
            .machine
            .deadline()
            .map(|ms| self.start + Duration::from_millis(ms));
        // Replacement first: its frames are only held, and reading them first
        // is what makes the old stream's read-ahead matter to the switch.
        let woke = tokio::select! {
            biased;
            event = next_of::<Fut, S, M, E>(self.replacement.as_mut()) => Woke::Replacement(event),
            event = next_of::<Fut, S, M, E>(self.old.as_mut()) => Woke::Old(event),
            () = until(deadline) => Woke::Deadline,
        };
        let now = self.now();
        match woke {
            Woke::Old(Event::Message(message)) => {
                let frame = to_frame(&self.dedupe_key, message);
                self.machine.on_frame(StreamRole::Old, frame, now)
            }
            Woke::Old(Event::Failed(error)) => self.end_old(Some(error)),
            Woke::Old(Event::Ended) => self.end_old(None),
            Woke::Replacement(Event::Message(message)) => {
                self.read_ahead_old();
                let frame = to_frame(&self.dedupe_key, message);
                self.machine.on_frame(StreamRole::Replacement, frame, now)
            }
            Woke::Replacement(Event::Failed(_)) => {
                self.replacement = None;
                self.machine
                    .on_end(StreamRole::Replacement, now, EndCause::Failed)
            }
            Woke::Replacement(Event::Ended) => {
                self.replacement = None;
                self.machine
                    .on_end(StreamRole::Replacement, now, EndCause::Ended)
            }
            Woke::Deadline => self.machine.on_tick(now),
        }
    }

    /// Move every frame the old stream can hand over without waiting into the
    /// unseen queue, so a switch delivers them before it drops the old stream.
    fn read_ahead_old(&mut self) {
        while self.old_ended.is_none() {
            let Some(old) = self.old.as_mut() else {
                return;
            };
            match old.next::<M, E>().now_or_never() {
                Some(Event::Message(message)) => {
                    let frame = to_frame(&self.dedupe_key, message);
                    lock(&self.unseen).push_back(frame);
                }
                Some(Event::Failed(error)) => {
                    self.old = None;
                    self.old_ended = Some(OldEnd::Failed(error));
                }
                Some(Event::Ended) => {
                    self.old = None;
                    self.old_ended = Some(OldEnd::Clean);
                }
                None => return,
            }
        }
    }

    fn end_old(&mut self, failure: Option<E>) -> Vec<RotationAction<M>> {
        self.old = None;
        let cause = if failure.is_some() {
            EndCause::Failed
        } else {
            EndCause::Ended
        };
        self.failure = failure;
        let now = self.now();
        self.machine.on_end(StreamRole::Old, now, cause)
    }

    fn apply(&mut self, actions: Vec<RotationAction<M>>) {
        for action in actions {
            match action {
                RotationAction::Deliver(message) => self.out.push_back(Ok(message)),
                RotationAction::Open => {
                    let leg = Leg::opening((self.open)(OpenRequest::declaring()));
                    self.replacement = Some(leg);
                }
                RotationAction::Close(StreamRole::Old) => {
                    self.old = None;
                    self.old_ended = None;
                    self.failure = None;
                }
                RotationAction::Close(StreamRole::Replacement) => self.replacement = None,
                RotationAction::Switch => self.old = self.replacement.take(),
                RotationAction::End => {
                    if let Some(error) = self.failure.take() {
                        self.out.push_back(Err(error));
                    }
                    self.old = None;
                    self.replacement = None;
                    self.done = true;
                }
                RotationAction::Hold(_) | RotationAction::Fallback(_) => {}
            }
        }
    }
}

enum Woke<M, E> {
    Old(Event<M, E>),
    Replacement(Event<M, E>),
    Deadline,
}

/// Open a server stream that replaces itself before the server cuts it.
///
/// The caller reads one logical stream of messages. A control never reaches
/// it: a message that carried only a control is not yielded, and one that
/// carried data as well is yielded with the control removed. On a drain
/// notice the driver opens a replacement and switches to it only on proof that
/// nothing falls between the two streams; it then drops the old stream, with
/// every message the old stream had already received delivered first and no
/// message repeated past `dedupe_key`. Any other outcome keeps the old stream,
/// and the logical stream ends or fails exactly as the old stream does. The
/// time allowed for a rotation is fixed; there is nothing to configure.
///
/// `open` is called once for each stream, first for the stream the caller
/// reads and again for each replacement. It must build a fresh client (a new
/// `HttpClient`, for instance) inside the call, so the replacement rides a new
/// connection rather than the one about to be cut. The request it receives
/// carries the header that declares the client reads controls; the
/// [`StreamControlled`] bound is what permits sending it.
///
/// `dedupe_key` names the key a message is deduplicated on across a switch; a
/// message with no key is never dropped.
///
/// Time is read from tokio's clock, so the stream must be polled inside a
/// tokio runtime.
///
/// A response type opens once it implements [`StreamControlled`]:
///
/// ```
/// use api_bones_stream::{
///     MalformedControl, OpenRequest, StreamControl, StreamControlled, rotating_stream,
/// };
/// use futures_util::stream;
///
/// #[derive(Clone, Default, PartialEq)]
/// struct Event;
///
/// impl StreamControlled for Event {
///     fn take_control(&mut self) -> Result<Option<StreamControl>, MalformedControl> {
///         Ok(None)
///     }
/// }
///
/// let open = |_: OpenRequest| async { Ok::<_, String>(stream::empty::<Result<Event, String>>()) };
/// let _events = rotating_stream(open, |_: &Event| None);
/// ```
///
/// One that does not is refused when the program compiles, so no request
/// ever declares stream control for it:
///
/// ```compile_fail,E0277
/// use api_bones_stream::{OpenRequest, rotating_stream};
/// use futures_util::stream;
///
/// #[derive(Clone, Default, PartialEq)]
/// struct Event;
///
/// let open = |_: OpenRequest| async { Ok::<_, String>(stream::empty::<Result<Event, String>>()) };
/// let _events = rotating_stream(open, |_: &Event| None);
/// ```
pub fn rotating_stream<M, K, Open, Fut, S, E>(
    open: Open,
    dedupe_key: K,
) -> impl Stream<Item = Result<M, E>>
where
    M: StreamControlled + Clone + Send + 'static,
    K: Fn(&M) -> Option<String>,
    Open: FnMut(OpenRequest) -> Fut,
    Fut: Future<Output = Result<S, E>>,
    S: Stream<Item = Result<M, E>> + Unpin,
{
    stream::unfold(Rotation::new(open, dedupe_key), |mut rotation| async move {
        rotation.next().await.map(|item| (item, rotation))
    })
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;
    use crate::{MalformedControl, StreamControl};

    #[derive(Debug, Clone, Default, PartialEq)]
    struct Msg {
        id: String,
        control: Option<Vec<u8>>,
    }

    impl StreamControlled for Msg {
        fn take_control(&mut self) -> Result<Option<StreamControl>, MalformedControl> {
            self.control
                .take()
                .map(|bytes| StreamControl::from_control_message(&bytes))
                .transpose()
        }
    }

    type Item = Result<Msg, String>;
    type Tx = mpsc::UnboundedSender<Item>;
    type Rx = mpsc::UnboundedReceiver<Item>;
    type Requests = Arc<Mutex<Vec<(String, String)>>>;

    fn pipe() -> (Tx, Rx) {
        mpsc::unbounded_channel()
    }

    fn feed(mut rx: Rx) -> impl Stream<Item = Item> + Unpin {
        stream::poll_fn(move |cx| rx.poll_recv(cx))
    }

    fn bare(control: &StreamControl) -> Vec<u8> {
        // Two tag bytes and a one-byte length precede the body.
        control.to_proto_frame()[3..].to_vec()
    }

    fn data(id: &str) -> Msg {
        Msg {
            id: id.to_owned(),
            control: None,
        }
    }

    fn control(control: &StreamControl) -> Msg {
        Msg {
            id: String::new(),
            control: Some(bare(control)),
        }
    }

    fn data_with_control(id: &str, with: &StreamControl) -> Msg {
        Msg {
            id: id.to_owned(),
            control: Some(bare(with)),
        }
    }

    fn drain(cut_in_ms: u32) -> StreamControl {
        StreamControl::Drain { cut_in_ms }
    }

    fn checkpoint(live_from: u64, delivered_through: u64) -> StreamControl {
        StreamControl::Checkpoint {
            domain: "d".to_owned(),
            live_from,
            delivered_through,
            epoch: 1,
        }
    }

    /// A rotating stream whose successive `open` calls take `legs` in order;
    /// a `None` leg fails to open. Each request is recorded.
    fn rotating(legs: Vec<Option<Rx>>, requests: &Requests) -> Pin<Box<dyn Stream<Item = Item>>> {
        let mut legs: VecDeque<Option<Rx>> = legs.into();
        let requests = Arc::clone(requests);
        Box::pin(rotating_stream(
            move |request: OpenRequest| {
                requests.lock().expect("requests").push((
                    request.header_name.to_owned(),
                    request.header_value.to_owned(),
                ));
                let leg = legs.pop_front().expect("more opens than legs");
                async move { leg.map(feed).ok_or_else(|| "open failed".to_owned()) }
            },
            |message: &Msg| (!message.id.is_empty()).then(|| message.id.clone()),
        ))
    }

    async fn next(stream: &mut Pin<Box<dyn Stream<Item = Item>>>) -> Option<Item> {
        stream.next().await
    }

    async fn assert_a_then_b_then_end(stream: &mut Pin<Box<dyn Stream<Item = Item>>>) {
        assert_eq!(next(stream).await, Some(Ok(data("a"))));
        assert_eq!(next(stream).await, Some(Ok(data("b"))));
        assert_eq!(next(stream).await, None);
    }

    fn send(tx: &Tx, items: Vec<Item>) {
        for item in items {
            tx.send(item).expect("receiver open");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn declares_the_capability_on_every_open() {
        let (old_tx, old_rx) = pipe();
        let (_new_tx, new_rx) = pipe();
        let requests = Requests::default();
        let mut stream = rotating(vec![Some(old_rx), Some(new_rx)], &requests);
        send(&old_tx, vec![Ok(data("a")), Ok(control(&drain(10_000)))]);
        assert_eq!(next(&mut stream).await, Some(Ok(data("a"))));
        let _ = tokio::time::timeout(Duration::from_millis(1), next(&mut stream)).await;
        let expected = (
            STREAM_CONTROL_HEADER.to_owned(),
            STREAM_CONTROL_HEADER_VALUE.to_owned(),
        );
        assert_eq!(*requests.lock().expect("requests"), vec![expected; 2]);
    }

    /// A rotating stream whose old leg has sent `a`, a checkpoint, a drain and
    /// then `b` and `c`, with the replacement leg still silent.
    fn draining_stream() -> (Tx, Tx, Pin<Box<dyn Stream<Item = Item>>>) {
        let (old_tx, old_rx) = pipe();
        let (new_tx, new_rx) = pipe();
        let stream = rotating(vec![Some(old_rx), Some(new_rx)], &Requests::default());
        send(
            &old_tx,
            vec![
                Ok(data("a")),
                Ok(control(&checkpoint(3, 2))),
                Ok(control(&drain(10_000))),
                Ok(data("b")),
                Ok(data("c")),
            ],
        );
        (old_tx, new_tx, stream)
    }

    #[tokio::test(start_paused = true)]
    async fn switches_on_proof_delivering_the_old_streams_buffered_frames_first() {
        let (old_tx, new_tx, mut stream) = draining_stream();
        send(
            &new_tx,
            vec![Ok(control(&checkpoint(2, 1))), Ok(data("c")), Ok(data("d"))],
        );
        for id in ["a", "b", "c", "d"] {
            assert_eq!(next(&mut stream).await, Some(Ok(data(id))), "{id}");
        }
        assert!(old_tx.is_closed(), "the retired stream is dropped");
        send(&new_tx, vec![Ok(data("e"))]);
        assert_eq!(next(&mut stream).await, Some(Ok(data("e"))));
        drop(new_tx);
        assert_eq!(next(&mut stream).await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn keeps_the_old_stream_until_it_ends_when_the_proof_is_missed() {
        let (old_tx, new_tx, mut stream) = draining_stream();
        send(&new_tx, vec![Ok(control(&checkpoint(9, 8))), Ok(data("x"))]);
        drop(old_tx);
        for id in ["a", "b", "c"] {
            assert_eq!(next(&mut stream).await, Some(Ok(data(id))), "{id}");
        }
        assert_eq!(next(&mut stream).await, None);
        assert!(new_tx.is_closed(), "the replacement is dropped");
    }

    #[tokio::test(start_paused = true)]
    async fn falls_back_at_the_deadline_less_the_margin() {
        let (old_tx, old_rx) = pipe();
        let (new_tx, new_rx) = pipe();
        let mut stream = rotating(vec![Some(old_rx), Some(new_rx)], &Requests::default());
        send(&old_tx, vec![Ok(control(&drain(10_000)))]);
        send(&new_tx, vec![Ok(data("x"))]);
        // The deadline is 8000 ms out; nothing is yielded before then or after.
        let quiet = tokio::time::timeout(Duration::from_secs(7), next(&mut stream)).await;
        assert!(quiet.is_err());
        assert!(!new_tx.is_closed(), "the rotation is still under way");
        let quiet = tokio::time::timeout(Duration::from_secs(2), next(&mut stream)).await;
        assert!(quiet.is_err());
        assert!(
            new_tx.is_closed(),
            "the replacement is dropped at the deadline"
        );
        send(&old_tx, vec![Ok(data("late"))]);
        assert_eq!(next(&mut stream).await, Some(Ok(data("late"))));
        drop(old_tx);
        assert_eq!(next(&mut stream).await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn keeps_the_old_stream_when_the_replacement_fails_to_open() {
        let (old_tx, old_rx) = pipe();
        let mut stream = rotating(vec![Some(old_rx), None], &Requests::default());
        send(
            &old_tx,
            vec![Ok(data("a")), Ok(control(&drain(10_000))), Ok(data("b"))],
        );
        drop(old_tx);
        assert_a_then_b_then_end(&mut stream).await;
    }

    #[tokio::test(start_paused = true)]
    async fn keeps_the_old_stream_when_the_replacement_errors() {
        let (old_tx, old_rx) = pipe();
        let (new_tx, new_rx) = pipe();
        let mut stream = rotating(vec![Some(old_rx), Some(new_rx)], &Requests::default());
        send(&old_tx, vec![Ok(control(&drain(10_000)))]);
        send(&new_tx, vec![Err("boom".to_owned())]);
        let quiet = tokio::time::timeout(Duration::from_millis(10), next(&mut stream)).await;
        assert!(quiet.is_err());
        send(&old_tx, vec![Ok(data("b"))]);
        drop(old_tx);
        assert_eq!(next(&mut stream).await, Some(Ok(data("b"))));
        assert_eq!(next(&mut stream).await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn fails_exactly_as_the_old_stream_fails() {
        let (old_tx, old_rx) = pipe();
        let mut stream = rotating(vec![Some(old_rx)], &Requests::default());
        send(&old_tx, vec![Ok(data("a")), Err("cut".to_owned())]);
        assert_eq!(next(&mut stream).await, Some(Ok(data("a"))));
        assert_eq!(next(&mut stream).await, Some(Err("cut".to_owned())));
        assert_eq!(next(&mut stream).await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn fails_when_the_first_stream_cannot_be_opened() {
        let mut stream = rotating(vec![None], &Requests::default());
        assert_eq!(next(&mut stream).await, Some(Err("open failed".to_owned())));
        assert_eq!(next(&mut stream).await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn strips_controls_from_what_the_caller_reads() {
        let (old_tx, old_rx) = pipe();
        let requests = Requests::default();
        let mut stream = rotating(vec![Some(old_rx)], &requests);
        send(
            &old_tx,
            vec![
                Ok(control(&checkpoint(1, 0))),
                Ok(data_with_control("a", &checkpoint(2, 1))),
                Ok(data("b")),
            ],
        );
        drop(old_tx);
        assert_a_then_b_then_end(&mut stream).await;
        assert_eq!(requests.lock().expect("requests").len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_control_that_cannot_be_read_keeps_the_old_stream() {
        let (old_tx, old_rx) = pipe();
        let (new_tx, new_rx) = pipe();
        let mut stream = rotating(vec![Some(old_rx), Some(new_rx)], &Requests::default());
        send(&old_tx, vec![Ok(control(&drain(10_000))), Ok(data("b"))]);
        send(
            &new_tx,
            vec![
                Ok(Msg {
                    id: String::new(),
                    control: Some(vec![0x0a]),
                }),
                Ok(data("x")),
            ],
        );
        drop(old_tx);
        assert_eq!(next(&mut stream).await, Some(Ok(data("b"))));
        assert_eq!(next(&mut stream).await, None);
        assert!(new_tx.is_closed());
    }
}
