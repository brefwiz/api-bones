// SPDX-License-Identifier: MIT
//! One span and completion event for each list iteration.

use std::time::Instant;

pub(crate) struct Measurement {
    span: tracing::Span,
    method: &'static str,
    start: Instant,
    first: Option<u64>,
    pub(crate) pages: u32,
    pub(crate) prefetch_hits: u32,
    pub(crate) not_modified_hits: u32,
    ended: bool,
}

impl Measurement {
    pub(crate) fn new(method: &'static str) -> Self {
        Self {
            span: tracing::info_span!(
                "bones.list",
                rpc.method = method,
                bones.list.time_to_first_item_ms = tracing::field::Empty,
                bones.list.time_to_full_list_ms = tracing::field::Empty,
                bones.list.pages = tracing::field::Empty,
                bones.list.prefetch_hits = tracing::field::Empty,
                bones.list.not_modified_hits = tracing::field::Empty,
                bones.list.cancelled = tracing::field::Empty
            ),
            method,
            start: Instant::now(),
            first: None,
            pages: 0,
            prefetch_hits: 0,
            not_modified_hits: 0,
            ended: false,
        }
    }

    pub(crate) fn page(&mut self, has_items: bool) {
        self.pages += 1;
        if has_items && self.first.is_none() {
            self.first = Some(self.elapsed());
        }
    }

    fn elapsed(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    pub(crate) fn finish(&mut self, complete: bool, cancelled: bool) {
        if self.ended {
            return;
        }
        self.ended = true;
        let full = complete.then(|| self.elapsed());
        if let Some(first) = self.first {
            self.span.record("bones.list.time_to_first_item_ms", first);
        }
        if let Some(full) = full {
            self.span.record("bones.list.time_to_full_list_ms", full);
        }
        self.span.record("bones.list.pages", self.pages);
        self.span
            .record("bones.list.prefetch_hits", self.prefetch_hits);
        self.span
            .record("bones.list.not_modified_hits", self.not_modified_hits);
        self.span.record("bones.list.cancelled", cancelled);
        tracing::event!(target: "bones.list", tracing::Level::INFO,
            rpc.method = self.method,
            bones.list.time_to_first_item_ms = self.first,
            bones.list.time_to_full_list_ms = full,
            bones.list.pages = self.pages,
            bones.list.prefetch_hits = self.prefetch_hits,
            bones.list.not_modified_hits = self.not_modified_hits,
            bones.list.cancelled = cancelled,
            "list iteration ended");
    }
}

impl Drop for Measurement {
    fn drop(&mut self) {
        self.finish(false, true);
    }
}
