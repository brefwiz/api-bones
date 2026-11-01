// SPDX-License-Identifier: MIT
//! Stream rotation: a long-lived server stream is replaced before the server
//! cuts it, so the consumer sees one logical stream with no gap.
//!
//! A server about to cut a stream sends a drain notice in-band, and sends
//! checkpoints, one per domain, each reporting how far the stream has
//! delivered. On a drain the client opens a replacement and switches only on
//! proof that nothing falls between the two streams: in every domain either
//! stream has checkpointed, the old stream's latest `delivered_through` is at
//! or above the `live_from` of the replacement's FIRST checkpoint, both in the
//! same epoch. Any other outcome keeps the old stream until the server cuts
//! it, and the consumer's own loop takes its usual path (reopen and report a
//! gap).
//!
//! [`RotationMachine`] is the pure decision logic: frames, lifecycle events
//! and the current time in, actions out. It owns no stream and no timer, and
//! the TypeScript package ships the same machine, action for action.

use std::collections::{HashMap, HashSet};

use crate::StreamControl;

/// The time the client keeps in hand before the cut: the replacement must
/// prove continuity this long before the server drops the old stream, leaving
/// time to finish the switch. Fixed; a consumer cannot move it.
const SWITCH_MARGIN_MS: u64 = 2000;

/// The longest cut a server announces. A drain naming a longer cut is read as
/// this one, so the rotation deadline never lies beyond it. Fixed.
const MAX_CUT_MS: u64 = 60_000;

/// The most frames held back from the replacement. Reaching it abandons the
/// replacement and keeps the old stream. Fixed.
const MAX_HELD_FRAMES: usize = 10_000;

/// One control signal, as the machine reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlSignal {
    /// The stream will be cut `cut_in_ms` milliseconds after the frame is read.
    Drain {
        /// Milliseconds from receipt until the cut.
        cut_in_ms: u32,
    },
    /// The position the stream has reached in one domain.
    Checkpoint {
        /// The domain the positions belong to.
        domain: String,
        /// The first position a stream opened now would deliver.
        live_from: u64,
        /// The contiguous position this stream has delivered.
        delivered_through: u64,
        /// The producer's sequence epoch.
        epoch: u64,
    },
    /// A control the codec could not read.
    Unreadable,
}

impl From<StreamControl> for ControlSignal {
    fn from(control: StreamControl) -> Self {
        match control {
            StreamControl::Drain { cut_in_ms } => Self::Drain { cut_in_ms },
            StreamControl::Checkpoint {
                domain,
                live_from,
                delivered_through,
                epoch,
            } => Self::Checkpoint {
                domain,
                live_from,
                delivered_through,
                epoch,
            },
        }
    }
}

/// Application data off a stream, with the key it is deduplicated on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationEvent<T> {
    /// The event the consumer receives.
    pub value: T,
    /// The key the event is deduplicated on; an event without one is never dropped.
    pub key: Option<String>,
}

/// One frame off a stream: its control, if any, and its application data, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotationFrame<T> {
    /// The control the frame carried.
    pub control: Option<ControlSignal>,
    /// The application data the frame carried.
    pub data: Option<RotationEvent<T>>,
}

impl<T> RotationFrame<T> {
    /// A frame that carries only a control.
    #[must_use]
    pub const fn from_control(control: ControlSignal) -> Self {
        Self {
            control: Some(control),
            data: None,
        }
    }

    /// A frame that carries only application data.
    #[must_use]
    pub const fn from_data(value: T, key: Option<String>) -> Self {
        Self {
            control: None,
            data: Some(RotationEvent { value, key }),
        }
    }
}

/// Which of the two streams an event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamRole {
    /// The stream the consumer is reading.
    Old,
    /// The stream opened to take over.
    Replacement,
}

/// How a stream stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndCause {
    /// The stream finished.
    Ended,
    /// The stream failed, including failing to open.
    Failed,
}

/// Why the machine kept the old stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// The deadline, the cut less the margin, passed before the proof held.
    Deadline,
    /// The drain came too close to its cut to open a replacement.
    TooClose,
    /// A stream's epoch changed within a domain.
    EpochChange,
    /// The two streams checkpointed a domain in different epochs.
    EpochMismatch,
    /// A stream's positions moved back within one epoch.
    Backward,
    /// The replacement sent a drain of its own.
    ReplacementDrained,
    /// A stream sent a control that could not be read.
    UnreadableControl,
    /// The replacement ended before the switch.
    ReplacementEnded,
    /// The replacement failed, including failing to open.
    ReplacementFailed,
    /// The replacement delivered more frames before the switch than the
    /// machine holds.
    HeldOverflow,
}

/// What the caller must do next, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationAction<T> {
    /// Hand the event to the consumer.
    Deliver(T),
    /// The replacement's event is held back until the switch.
    Hold(T),
    /// Open a replacement stream.
    Open,
    /// Close a stream.
    Close(StreamRole),
    /// The replacement is now the stream the consumer reads.
    Switch,
    /// Keep the old stream; the replacement, if any, has been closed.
    Fallback(FallbackReason),
    /// The consumer's stream ends as the old stream did.
    End,
}

#[derive(Debug, Clone, Copy)]
struct Mark {
    live_from: u64,
    delivered_through: u64,
    epoch: u64,
}

#[derive(Debug, Default)]
struct Track {
    first: HashMap<String, Mark>,
    latest: HashMap<String, Mark>,
}

impl Track {
    fn record(&mut self, domain: String, mark: Mark) {
        self.latest.insert(domain.clone(), mark);
        self.first.entry(domain).or_insert(mark);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Steady,
    Rotating,
    Kept,
    Ended,
}

type TakeBufferedOld<T> = Box<dyn FnMut() -> Vec<RotationFrame<T>> + Send>;

/// The rotation decision logic. It owns no stream and no timer: every call
/// carries the time in milliseconds, so a test drives it with a virtual clock.
///
/// Old-stream data is delivered as it arrives; replacement data is held until
/// the switch. The function given to [`RotationMachine::with_buffered_old`]
/// returns the frames the old stream has already received but the machine has
/// not yet been shown; the machine delivers them before it closes the old
/// stream.
pub struct RotationMachine<T> {
    phase: Phase,
    drained: bool,
    cut_at: u64,
    old_track: Track,
    replacement_track: Track,
    held: Vec<RotationEvent<T>>,
    current: HashSet<String>,
    previous: HashSet<String>,
    take_buffered_old: TakeBufferedOld<T>,
}

impl<T: 'static> Default for RotationMachine<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> RotationMachine<T> {
    /// A machine over a stream that buffers nothing ahead of the machine.
    #[must_use]
    pub fn new() -> Self {
        Self::with_buffered_old(Vec::new)
    }

    /// A machine whose old stream may hold frames the machine has not been shown.
    #[must_use]
    pub fn with_buffered_old(
        take_buffered_old: impl FnMut() -> Vec<RotationFrame<T>> + Send + 'static,
    ) -> Self {
        Self {
            phase: Phase::Steady,
            drained: false,
            cut_at: 0,
            old_track: Track::default(),
            replacement_track: Track::default(),
            held: Vec::new(),
            current: HashSet::new(),
            previous: HashSet::new(),
            take_buffered_old: Box::new(take_buffered_old),
        }
    }

    /// When the current rotation gives up and keeps the old stream, if one is under way.
    #[must_use]
    pub const fn deadline(&self) -> Option<u64> {
        match self.phase {
            Phase::Rotating => Some(self.cut_at),
            _ => None,
        }
    }

    /// A frame arrived on `stream` at `now`.
    pub fn on_frame(
        &mut self,
        stream: StreamRole,
        frame: RotationFrame<T>,
        now: u64,
    ) -> Vec<RotationAction<T>>
    where
        T: Clone,
    {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        let mut out = self.expire(now);
        let RotationFrame { control, data } = frame;
        match stream {
            StreamRole::Replacement => {
                if self.phase != Phase::Rotating {
                    return out;
                }
                if let Some(event) = data {
                    self.held.push(event);
                    if self.held.len() >= MAX_HELD_FRAMES {
                        out.extend(self.keep_old(FallbackReason::HeldOverflow));
                        return out;
                    }
                    if let Some(held) = self.held.last() {
                        out.push(RotationAction::Hold(held.value.clone()));
                    }
                }
                if let Some(control) = control {
                    out.extend(self.replacement_control(control));
                }
            }
            StreamRole::Old => {
                if let Some(event) = data {
                    out.extend(self.deliver(event));
                }
                if let Some(control) = control {
                    out.extend(self.old_control(control, now));
                }
            }
        }
        out
    }

    /// A stream ended or failed, including failing to open.
    pub fn on_end(
        &mut self,
        stream: StreamRole,
        now: u64,
        cause: EndCause,
    ) -> Vec<RotationAction<T>> {
        if self.phase == Phase::Ended {
            return Vec::new();
        }
        let mut out = self.expire(now);
        if stream == StreamRole::Replacement {
            if self.phase != Phase::Rotating {
                return out;
            }
            let reason = match cause {
                EndCause::Failed => FallbackReason::ReplacementFailed,
                EndCause::Ended => FallbackReason::ReplacementEnded,
            };
            out.extend(self.keep_old(reason));
            return out;
        }
        if self.phase == Phase::Rotating {
            out.push(RotationAction::Close(StreamRole::Replacement));
        }
        self.phase = Phase::Ended;
        self.held.clear();
        out.push(RotationAction::End);
        out
    }

    /// Time passed with no event.
    pub fn on_tick(&mut self, now: u64) -> Vec<RotationAction<T>> {
        if self.phase == Phase::Ended {
            Vec::new()
        } else {
            self.expire(now)
        }
    }

    fn expire(&mut self, now: u64) -> Vec<RotationAction<T>> {
        if self.phase == Phase::Rotating && now >= self.cut_at {
            return self.keep_old(FallbackReason::Deadline);
        }
        Vec::new()
    }

    fn keep_old(&mut self, reason: FallbackReason) -> Vec<RotationAction<T>> {
        let was_rotating = self.phase == Phase::Rotating;
        self.phase = Phase::Kept;
        self.held.clear();
        self.replacement_track = Track::default();
        let mut out = Vec::new();
        if was_rotating {
            out.push(RotationAction::Close(StreamRole::Replacement));
        }
        out.push(RotationAction::Fallback(reason));
        out
    }

    fn old_control(&mut self, control: ControlSignal, now: u64) -> Vec<RotationAction<T>> {
        match control {
            ControlSignal::Drain { cut_in_ms } => {
                if self.drained {
                    return Vec::new();
                }
                self.drained = true;
                let cut_in = u64::from(cut_in_ms).min(MAX_CUT_MS);
                if cut_in <= SWITCH_MARGIN_MS {
                    self.phase = Phase::Kept;
                    return vec![RotationAction::Fallback(FallbackReason::TooClose)];
                }
                self.phase = Phase::Rotating;
                self.cut_at = now.saturating_add(cut_in - SWITCH_MARGIN_MS);
                self.previous = HashSet::new();
                vec![RotationAction::Open]
            }
            ControlSignal::Checkpoint {
                domain,
                live_from,
                delivered_through,
                epoch,
            } => self.checkpoint(
                StreamRole::Old,
                domain,
                Mark {
                    live_from,
                    delivered_through,
                    epoch,
                },
            ),
            ControlSignal::Unreadable => {
                if self.phase == Phase::Rotating {
                    self.keep_old(FallbackReason::UnreadableControl)
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn replacement_control(&mut self, control: ControlSignal) -> Vec<RotationAction<T>> {
        match control {
            ControlSignal::Drain { .. } => self.keep_old(FallbackReason::ReplacementDrained),
            ControlSignal::Checkpoint {
                domain,
                live_from,
                delivered_through,
                epoch,
            } => self.checkpoint(
                StreamRole::Replacement,
                domain,
                Mark {
                    live_from,
                    delivered_through,
                    epoch,
                },
            ),
            ControlSignal::Unreadable => self.keep_old(FallbackReason::UnreadableControl),
        }
    }

    /// The reason a checkpoint ends the rotation at once, if it does.
    fn conflict(&self, stream: StreamRole, domain: &str, mark: Mark) -> Option<FallbackReason> {
        let (own, other) = match stream {
            StreamRole::Old => (&self.old_track, &self.replacement_track),
            StreamRole::Replacement => (&self.replacement_track, &self.old_track),
        };
        if let Some(before) = own.latest.get(domain) {
            if before.epoch != mark.epoch {
                return Some(FallbackReason::EpochChange);
            }
            if mark.live_from < before.live_from
                || mark.delivered_through < before.delivered_through
            {
                return Some(FallbackReason::Backward);
            }
        }
        // Compared against the other stream's mark the proof reads: the old
        // stream's latest, the replacement's first.
        let peer = match stream {
            StreamRole::Old => other.first.get(domain),
            StreamRole::Replacement => other.latest.get(domain),
        };
        match peer {
            Some(peer) if peer.epoch != mark.epoch => Some(FallbackReason::EpochMismatch),
            _ => None,
        }
    }

    fn checkpoint(
        &mut self,
        stream: StreamRole,
        domain: String,
        mark: Mark,
    ) -> Vec<RotationAction<T>> {
        if self.phase == Phase::Rotating {
            if let Some(reason) = self.conflict(stream, &domain, mark) {
                return self.keep_old(reason);
            }
        }
        match stream {
            StreamRole::Old => self.old_track.record(domain, mark),
            StreamRole::Replacement => self.replacement_track.record(domain, mark),
        }
        if self.phase == Phase::Rotating && self.proven() {
            return self.switch_over();
        }
        Vec::new()
    }

    fn proven(&self) -> bool {
        let domains: HashSet<&String> = self
            .old_track
            .latest
            .keys()
            .chain(self.replacement_track.latest.keys())
            .collect();
        if domains.is_empty() {
            return false;
        }
        domains.into_iter().all(|domain| {
            match (
                self.old_track.latest.get(domain),
                self.replacement_track.first.get(domain),
            ) {
                (Some(old), Some(first)) => {
                    old.epoch == first.epoch && old.delivered_through >= first.live_from
                }
                _ => false,
            }
        })
    }

    fn switch_over(&mut self) -> Vec<RotationAction<T>> {
        let mut out = Vec::new();
        for frame in (self.take_buffered_old)() {
            if let Some(event) = frame.data {
                out.extend(self.deliver(event));
            }
        }
        out.push(RotationAction::Close(StreamRole::Old));
        self.previous = std::mem::take(&mut self.current);
        for event in std::mem::take(&mut self.held) {
            out.extend(self.deliver(event));
        }
        self.old_track = std::mem::take(&mut self.replacement_track);
        self.drained = false;
        self.phase = Phase::Steady;
        out.push(RotationAction::Switch);
        out
    }

    /// Deliver `event` unless its key was already delivered by this stream or the previous one.
    fn deliver(&mut self, event: RotationEvent<T>) -> Option<RotationAction<T>> {
        if let Some(key) = event.key {
            if self.current.contains(&key) || self.previous.contains(&key) {
                return None;
            }
            self.current.insert(key);
        }
        Some(RotationAction::Deliver(event.value))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    type Buffered = Arc<Mutex<Vec<RotationFrame<String>>>>;
    type Action = RotationAction<String>;

    const OLD: StreamRole = StreamRole::Old;
    const REPLACEMENT: StreamRole = StreamRole::Replacement;

    fn drain(cut_in_ms: u32) -> ControlSignal {
        ControlSignal::Drain { cut_in_ms }
    }

    fn cp(domain: &str, live_from: u64, through: u64, epoch: u64) -> ControlSignal {
        ControlSignal::Checkpoint {
            domain: domain.to_owned(),
            live_from,
            delivered_through: through,
            epoch,
        }
    }

    fn ev(id: &str) -> RotationFrame<String> {
        RotationFrame::from_data(id.to_owned(), Some(id.to_owned()))
    }

    fn ctl(control: ControlSignal) -> RotationFrame<String> {
        RotationFrame::from_control(control)
    }

    fn deliver(value: &str) -> Action {
        RotationAction::Deliver(value.to_owned())
    }

    fn hold(value: &str) -> Action {
        RotationAction::Hold(value.to_owned())
    }

    fn switched(actions: &[Action]) -> bool {
        actions.contains(&RotationAction::Switch)
    }

    fn fell_back(actions: &[Action]) -> Option<FallbackReason> {
        actions.iter().find_map(|a| match a {
            RotationAction::Fallback(reason) => Some(*reason),
            _ => None,
        })
    }

    fn fallback(reason: FallbackReason) -> Action {
        RotationAction::Fallback(reason)
    }

    fn close_replacement() -> Action {
        RotationAction::Close(REPLACEMENT)
    }

    fn machine_over(buffered: &Buffered) -> RotationMachine<String> {
        let buffered = Arc::clone(buffered);
        RotationMachine::with_buffered_old(move || {
            std::mem::take(&mut *buffered.lock().expect("buffer"))
        })
    }

    /// A machine already drained at 1000 ms with a 30000 ms cut, so its deadline is 29000.
    fn rotating() -> (RotationMachine<String>, Buffered) {
        let buffered: Buffered = Arc::default();
        let mut machine = machine_over(&buffered);
        machine.on_frame(OLD, ctl(cp("a", 90, 99, 1)), 500);
        assert_eq!(
            machine.on_frame(OLD, ctl(drain(30000)), 1000),
            vec![RotationAction::Open]
        );
        (machine, buffered)
    }

    #[test]
    fn switches_delivers_buffered_frames_before_closing_old_and_never_repeats_a_key() {
        let (mut machine, buffered) = rotating();

        assert_eq!(
            machine.on_frame(REPLACEMENT, ev("e-100"), 1100),
            vec![hold("e-100")]
        );
        machine.on_frame(REPLACEMENT, ev("e-101"), 1110);
        machine.on_frame(REPLACEMENT, ev("e-102"), 1120);
        assert_eq!(
            machine.on_frame(REPLACEMENT, ctl(cp("a", 100, 102, 1)), 1130),
            vec![]
        );
        assert_eq!(
            machine.on_frame(OLD, ev("e-100"), 1200),
            vec![deliver("e-100")]
        );

        buffered.lock().expect("buffer").push(ev("e-101"));
        let actions = machine.on_frame(OLD, ctl(cp("a", 90, 100, 1)), 1300);

        assert_eq!(
            actions,
            vec![
                deliver("e-101"),
                RotationAction::Close(OLD),
                deliver("e-102"),
                RotationAction::Switch,
            ]
        );
    }

    #[test]
    fn proof_uses_the_replacements_first_checkpoint_not_its_moving_head() {
        let (mut machine, _) = rotating();
        machine.on_frame(REPLACEMENT, ctl(cp("a", 100, 100, 1)), 1100);
        // The replacement's head moves on; the proof is still judged against 100.
        assert_eq!(
            machine.on_frame(REPLACEMENT, ctl(cp("a", 500, 500, 1)), 1150),
            vec![]
        );
        assert!(switched(&machine.on_frame(
            OLD,
            ctl(cp("a", 90, 100, 1)),
            1200
        )));
    }

    #[test]
    fn drops_a_key_delivered_by_the_previous_stream_and_keeps_keyless_events() {
        let (mut machine, _) = rotating();
        machine.on_frame(OLD, ev("e-99"), 1050);
        machine.on_frame(REPLACEMENT, ctl(cp("a", 100, 100, 1)), 1100);
        machine.on_frame(OLD, ctl(cp("a", 90, 100, 1)), 1200);

        assert_eq!(machine.on_frame(OLD, ev("e-99"), 1300), vec![]);
        let keyless = RotationFrame::from_data("k".to_owned(), None);
        assert_eq!(
            machine.on_frame(OLD, keyless.clone(), 1310),
            vec![deliver("k")]
        );
        assert_eq!(machine.on_frame(OLD, keyless, 1320), vec![deliver("k")]);
    }

    #[test]
    fn rotates_again_after_a_switch() {
        let (mut machine, _) = rotating();
        machine.on_frame(REPLACEMENT, ctl(cp("a", 100, 100, 1)), 1100);
        machine.on_frame(OLD, ctl(cp("a", 90, 100, 1)), 1200);
        assert_eq!(
            machine.on_frame(OLD, ctl(drain(30000)), 5000),
            vec![RotationAction::Open]
        );
    }

    #[test]
    fn keeps_the_old_stream_until_the_deadline_less_the_margin_then_falls_back() {
        let (mut machine, _) = rotating();
        machine.on_frame(REPLACEMENT, ctl(cp("a", 120, 130, 1)), 1100);
        machine.on_frame(OLD, ctl(cp("a", 90, 100, 1)), 1200);

        assert_eq!(machine.deadline(), Some(29000));
        assert_eq!(machine.on_tick(28999), vec![]);
        assert_eq!(
            machine.on_tick(29000),
            vec![close_replacement(), fallback(FallbackReason::Deadline)]
        );
        assert_eq!(machine.deadline(), None);
        // The old stream carries on until the server cuts it, then the consumer's stream ends.
        assert_eq!(
            machine.on_frame(OLD, ev("e-1"), 29500),
            vec![deliver("e-1")]
        );
        assert_eq!(
            machine.on_end(OLD, 32000, EndCause::Ended),
            vec![RotationAction::End]
        );
    }

    #[test]
    fn discards_what_the_replacement_held_when_it_falls_back() {
        let (mut machine, _) = rotating();
        machine.on_frame(REPLACEMENT, ev("e-100"), 1100);
        machine.on_tick(29000);
        assert_eq!(machine.on_frame(REPLACEMENT, ev("e-101"), 29100), vec![]);
        assert_eq!(
            machine.on_frame(OLD, ctl(cp("a", 90, 500, 1)), 29200),
            vec![]
        );
    }

    #[test]
    fn does_not_switch_for_a_proof_completed_after_the_deadline_less_the_margin() {
        let (mut machine, _) = rotating();
        machine.on_frame(REPLACEMENT, ctl(cp("a", 100, 100, 1)), 1100);
        let actions = machine.on_frame(OLD, ctl(cp("a", 90, 100, 1)), 29000);
        assert!(!switched(&actions));
        assert_eq!(fell_back(&actions), Some(FallbackReason::Deadline));
    }

    #[test]
    fn does_not_switch_with_no_checkpoint_on_either_stream() {
        let mut machine = RotationMachine::<String>::new();
        machine.on_frame(OLD, ctl(drain(30000)), 1000);
        assert_eq!(
            machine.on_frame(REPLACEMENT, ev("e-1"), 1100),
            vec![hold("e-1")]
        );
        assert!(
            machine
                .on_tick(29000)
                .contains(&fallback(FallbackReason::Deadline))
        );
    }

    #[test]
    fn falls_back_at_once_on_an_epoch_change_an_epoch_mismatch_or_a_backward_move() {
        use FallbackReason::{Backward, EpochChange, EpochMismatch};
        type Case = (
            &'static str,
            Vec<(StreamRole, ControlSignal)>,
            FallbackReason,
        );
        let cases: Vec<Case> = vec![
            (
                "the replacement changes epoch",
                vec![
                    (REPLACEMENT, cp("a", 100, 100, 1)),
                    (REPLACEMENT, cp("a", 101, 101, 2)),
                ],
                EpochChange,
            ),
            (
                "the old stream changes epoch",
                vec![(OLD, cp("a", 90, 100, 2))],
                EpochChange,
            ),
            (
                "the epochs differ",
                vec![(REPLACEMENT, cp("a", 100, 100, 2))],
                EpochMismatch,
            ),
            (
                "the old stream moves back in delivered_through",
                vec![(OLD, cp("a", 90, 100, 1)), (OLD, cp("a", 90, 98, 1))],
                Backward,
            ),
            (
                "the old stream moves back in live_from",
                vec![(OLD, cp("a", 90, 100, 1)), (OLD, cp("a", 80, 100, 1))],
                Backward,
            ),
            (
                "the replacement moves back in live_from",
                vec![
                    (REPLACEMENT, cp("a", 100, 100, 1)),
                    (REPLACEMENT, cp("a", 99, 100, 1)),
                ],
                Backward,
            ),
            (
                "the replacement moves back in delivered_through",
                vec![
                    (REPLACEMENT, cp("a", 100, 100, 1)),
                    (REPLACEMENT, cp("a", 100, 99, 1)),
                ],
                Backward,
            ),
        ];
        for (label, frames, reason) in cases {
            let (mut machine, _) = rotating();
            let mut last = Vec::new();
            for (stream, control) in frames {
                last = machine.on_frame(stream, ctl(control), 1100);
            }
            assert_eq!(last, vec![close_replacement(), fallback(reason)], "{label}");
            // A checkpoint that would now cover the replacement does not revive the rotation.
            assert!(
                !switched(&machine.on_frame(OLD, ctl(cp("a", 90, 900, 1)), 1200)),
                "{label}"
            );
        }
    }

    #[test]
    fn falls_back_when_either_stream_sends_a_control_that_cannot_be_read() {
        for stream in [OLD, REPLACEMENT] {
            let (mut machine, _) = rotating();
            assert_eq!(
                machine.on_frame(stream, ctl(ControlSignal::Unreadable), 1100),
                vec![
                    close_replacement(),
                    fallback(FallbackReason::UnreadableControl)
                ]
            );
        }
    }

    /// Two domains checkpointed on the old stream, then a drain, and the
    /// replacement covering domain `a` only.
    fn two_domains_announcing_a() -> RotationMachine<String> {
        let mut machine = RotationMachine::<String>::new();
        machine.on_frame(OLD, ctl(cp("a", 90, 99, 1)), 500);
        machine.on_frame(OLD, ctl(cp("b", 40, 49, 1)), 510);
        machine.on_frame(OLD, ctl(drain(30000)), 1000);
        machine.on_frame(REPLACEMENT, ctl(cp("a", 100, 100, 1)), 1100);
        machine
    }

    fn two_domains() -> RotationMachine<String> {
        let mut machine = two_domains_announcing_a();
        machine.on_frame(REPLACEMENT, ctl(cp("b", 50, 50, 1)), 1110);
        machine
    }

    #[test]
    fn two_domains_switch_only_once_both_are_covered() {
        let mut machine = two_domains();
        assert!(!switched(&machine.on_frame(
            OLD,
            ctl(cp("a", 90, 100, 1)),
            1200
        )));
        assert!(switched(&machine.on_frame(
            OLD,
            ctl(cp("b", 40, 50, 1)),
            1210
        )));
    }

    #[test]
    fn two_domains_fall_back_when_one_stays_uncovered() {
        let mut machine = two_domains();
        machine.on_frame(OLD, ctl(cp("a", 90, 100, 1)), 1200);
        machine.on_frame(OLD, ctl(cp("b", 40, 49, 1)), 1210);
        assert!(
            machine
                .on_tick(29000)
                .contains(&fallback(FallbackReason::Deadline))
        );
    }

    #[test]
    fn two_domains_fall_back_when_the_replacement_never_announces_one() {
        let mut machine = two_domains_announcing_a();
        assert!(!switched(&machine.on_frame(
            OLD,
            ctl(cp("a", 90, 100, 1)),
            1200
        )));
        assert!(
            machine
                .on_tick(29000)
                .contains(&fallback(FallbackReason::Deadline))
        );
    }

    #[test]
    fn keeps_the_old_stream_when_the_replacement_fails_ends_drains_or_is_unreadable() {
        type Act = fn(&mut RotationMachine<String>) -> Vec<Action>;
        let cases: [(&str, Act, FallbackReason); 3] = [
            (
                "fails to open",
                |m| m.on_end(REPLACEMENT, 1100, EndCause::Failed),
                FallbackReason::ReplacementFailed,
            ),
            (
                "ends",
                |m| m.on_end(REPLACEMENT, 1100, EndCause::Ended),
                FallbackReason::ReplacementEnded,
            ),
            (
                "sends a drain",
                |m| m.on_frame(REPLACEMENT, ctl(drain(30000)), 1100),
                FallbackReason::ReplacementDrained,
            ),
        ];
        for (label, act, reason) in cases {
            let (mut machine, _) = rotating();
            assert_eq!(
                act(&mut machine),
                vec![close_replacement(), fallback(reason)],
                "{label}"
            );
            assert_eq!(
                machine.on_frame(OLD, ev("e-1"), 1200),
                vec![deliver("e-1")],
                "{label}"
            );
        }
    }

    #[test]
    fn old_stream_ending_ends_the_consumers_stream_and_closes_the_replacement() {
        let (mut machine, _) = rotating();
        machine.on_frame(REPLACEMENT, ev("e-100"), 1100);
        assert_eq!(
            machine.on_end(OLD, 1200, EndCause::Ended),
            vec![close_replacement(), RotationAction::End]
        );
        assert_eq!(machine.on_frame(REPLACEMENT, ev("e-101"), 1300), vec![]);
    }

    #[test]
    fn ignores_a_second_drain_on_the_old_stream() {
        let (mut machine, _) = rotating();
        assert_eq!(machine.on_frame(OLD, ctl(drain(500)), 1100), vec![]);
        assert_eq!(machine.deadline(), Some(29000));
    }

    #[test]
    fn ignores_a_drain_after_falling_back() {
        let (mut machine, _) = rotating();
        machine.on_tick(29000);
        assert_eq!(machine.on_frame(OLD, ctl(drain(30000)), 29100), vec![]);
    }

    #[test]
    fn opens_nothing_for_a_drain_inside_the_margin() {
        for cut_in_ms in [2000, 1500, 0] {
            let mut machine = RotationMachine::<String>::new();
            assert_eq!(
                machine.on_frame(OLD, ctl(drain(cut_in_ms)), 1000),
                vec![fallback(FallbackReason::TooClose)],
                "{cut_in_ms}"
            );
            assert_eq!(machine.deadline(), None, "{cut_in_ms}");
        }
    }

    #[test]
    fn opens_a_replacement_for_a_drain_just_outside_the_margin() {
        let mut machine = RotationMachine::<String>::new();
        assert_eq!(
            machine.on_frame(OLD, ctl(drain(2001)), 1000),
            vec![RotationAction::Open]
        );
    }

    #[test]
    fn reads_a_cut_longer_than_a_minute_as_a_minute() {
        let mut machine = RotationMachine::<String>::new();
        assert_eq!(
            machine.on_frame(OLD, ctl(drain(u32::MAX)), 1000),
            vec![RotationAction::Open]
        );
        assert_eq!(machine.deadline(), Some(1000 + 60_000 - SWITCH_MARGIN_MS));
        assert_eq!(machine.on_tick(58_999), vec![]);
        assert_eq!(
            machine.on_tick(59_000),
            vec![close_replacement(), fallback(FallbackReason::Deadline)]
        );
    }

    #[test]
    fn keeps_a_cut_of_exactly_a_minute_as_announced() {
        let mut machine = RotationMachine::<String>::new();
        machine.on_frame(OLD, ctl(drain(60_000)), 0);
        assert_eq!(machine.deadline(), Some(58_000));
    }

    #[test]
    fn abandons_the_replacement_when_it_fills_the_hold() {
        let (mut machine, _) = rotating();
        for n in 0..MAX_HELD_FRAMES - 1 {
            let actions = machine.on_frame(REPLACEMENT, ev(&format!("r-{n}")), 1100);
            assert_eq!(actions, vec![hold(&format!("r-{n}"))]);
        }
        let actions = machine.on_frame(REPLACEMENT, ev("one-too-many"), 1100);
        assert_eq!(
            actions,
            vec![close_replacement(), fallback(FallbackReason::HeldOverflow)]
        );
        // The old stream carries on and the consumer's stream ends as it does.
        assert_eq!(machine.on_frame(OLD, ev("o-1"), 1200), vec![deliver("o-1")]);
        assert_eq!(
            machine.on_end(OLD, 1300, EndCause::Ended),
            vec![RotationAction::End]
        );
    }

    #[test]
    fn reads_a_decoded_stream_control_into_a_signal() {
        assert_eq!(
            ControlSignal::from(StreamControl::Drain { cut_in_ms: 7 }),
            drain(7)
        );
        assert_eq!(
            ControlSignal::from(StreamControl::Checkpoint {
                domain: "a".to_owned(),
                live_from: 1,
                delivered_through: 2,
                epoch: 3,
            }),
            cp("a", 1, 2, 3)
        );
    }
}
