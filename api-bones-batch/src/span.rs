// SPDX-License-Identifier: MIT
//! One `bones.batch` span and one completion event per wire batch request.

use std::sync::Arc;

use tracing::Span;

/// How a wire batch request ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Answered.
    Ok,
    /// Failed as a whole.
    Error,
    /// Every caller went away and the request was aborted.
    Cancelled,
    /// The response did not answer the requested ids.
    Mismatch,
}

impl Outcome {
    /// The `bones.batch.outcome` value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
            Self::Mismatch => "mismatch",
        }
    }
}

/// What one wire batch request did, as the span records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchRecord {
    /// The batch method's full name.
    pub rpc_method: &'static str,
    /// The single method's full name.
    pub single_method: &'static str,
    /// Ids before dedup.
    pub requested: usize,
    /// Ids sent.
    pub items: usize,
    /// Callers merged: 1 for an explicit call.
    pub calls: usize,
    /// Position of this request among the call's requests.
    pub chunk_index: usize,
    /// Requests the call was split into.
    pub chunks: usize,
    /// Encoded request bytes.
    pub bytes: usize,
    /// Milliseconds from the oldest enqueue to the send.
    pub wait_ms: u64,
    /// Items answered with an error.
    pub item_errors: usize,
    /// How it ended.
    pub outcome: Outcome,
}

/// Receives each record when its request ends.
pub type Observer = Arc<dyn Fn(&BatchRecord) + Send + Sync>;

// Span and completion event carry the same immutable request dimensions.
macro_rules! request_fields {
    ($record:expr, $emit:ident) => {
        $emit! {
            $record;
            rpc.method = $record.rpc_method,
            bones.batch.single_method = $record.single_method,
            bones.batch.requested = $record.requested,
            bones.batch.items = $record.items,
            bones.batch.calls = $record.calls,
            bones.batch.chunk_index = $record.chunk_index,
            bones.batch.chunks = $record.chunks,
            bones.batch.bytes = $record.bytes,
            bones.batch.wait_ms = $record.wait_ms,
        }
    };
}

macro_rules! request_span {
    ($record:expr; $($fields:tt)*) => {
        tracing::info_span!(
            "bones.batch",
            $($fields)*
            bones.batch.item_errors = tracing::field::Empty,
            bones.batch.outcome = tracing::field::Empty,
        )
    };
}

macro_rules! completion_event {
    ($record:expr; $($fields:tt)*) => {
        tracing::event!(
            target: "bones.batch",
            tracing::Level::INFO,
            $($fields)*
            bones.batch.item_errors = $record.item_errors,
            bones.batch.outcome = $record.outcome.as_str(),
            "batch request ended"
        )
    };
}

/// The open span of one request.
pub(crate) fn open(record: &BatchRecord) -> Span {
    request_fields!(record, request_span)
}

/// Close `span` with the outcome, emit the unsampled completion event and
/// tell the observer.
pub(crate) fn finish(span: &Span, observer: Option<&Observer>, record: &BatchRecord) {
    span.record("bones.batch.item_errors", record.item_errors);
    span.record("bones.batch.outcome", record.outcome.as_str());
    request_fields!(record, completion_event);
    if let Some(observer) = observer {
        observer(record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcome_tokens() {
        let tokens = [
            Outcome::Ok,
            Outcome::Error,
            Outcome::Cancelled,
            Outcome::Mismatch,
        ]
        .map(Outcome::as_str);
        assert_eq!(tokens, ["ok", "error", "cancelled", "mismatch"]);
    }

    #[test]
    fn finish_tells_the_observer() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let observer: Observer = Arc::new(move |r| sink.lock().expect("sink").push(r.items));
        let record = BatchRecord {
            rpc_method: "p.S/B",
            single_method: "p.S/G",
            requested: 3,
            items: 2,
            calls: 3,
            chunk_index: 0,
            chunks: 1,
            bytes: 10,
            wait_ms: 0,
            item_errors: 0,
            outcome: Outcome::Ok,
        };
        finish(&open(&record), Some(&observer), &record);
        assert_eq!(*seen.lock().expect("sink"), [2]);
    }
}
