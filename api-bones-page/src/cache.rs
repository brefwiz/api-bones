// SPDX-License-Identifier: MIT
//! Caller-scoped revision chains, held within a byte budget.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use api_bones_rt::{MaybeSend, MaybeSync};

use crate::{Page, Total};

/// The most one (identity, method, request) chain holds: the encoded protobuf
/// length of its items plus [`PAGE_OVERHEAD`] and the revision's length for
/// every page.
pub(crate) const HOLD_CHAIN_BYTES: usize = 1 << 20;

/// What one held page costs besides its items, charged to the budgets so a
/// chain of item-less pages still counts: the page's record, its item slice
/// and its share of the chain's bookkeeping.
pub(crate) const PAGE_OVERHEAD: usize = 128;

/// The most the whole cache holds; the least recently used chain goes first.
pub(crate) const HOLD_TOTAL_BYTES: usize = 4 << 20;

#[cfg(not(target_arch = "wasm32"))]
type Chain = Box<dyn Any + Send + Sync>;
#[cfg(target_arch = "wasm32")]
type Chain = Box<dyn Any>;

type Key = (&'static str, [u8; 32]);

/// A stored chain with its encoded size and the tick of its last use.
struct Entry {
    chain: Chain,
    bytes: usize,
    used: u64,
}

/// What one identity has cached. Its generation names the slot's life: a
/// walk that started under another generation has outlived its cache.
struct Slot {
    generation: u64,
    entries: HashMap<Key, Entry>,
}

#[derive(Default)]
struct State {
    /// Never reused, so a dropped slot's walks can never match a new one.
    next_generation: u64,
    /// Advances on every store and replay; orders the entries by use.
    clock: u64,
    slots: HashMap<String, Slot>,
}

impl State {
    fn total_bytes(&self) -> usize {
        self.slots
            .values()
            .flat_map(|slot| slot.entries.values())
            .map(|entry| entry.bytes)
            .sum()
    }

    /// Drops the least recently used chain of any identity; whether one went.
    fn evict_oldest(&mut self) -> bool {
        let oldest = self
            .slots
            .iter()
            .flat_map(|(identity, slot)| {
                slot.entries
                    .iter()
                    .map(move |(key, entry)| (entry.used, identity.clone(), *key))
            })
            .min_by_key(|(used, ..)| *used);
        let Some((_, identity, key)) = oldest else {
            return false;
        };
        if let Some(slot) = self.slots.get_mut(&identity) {
            slot.entries.remove(&key);
        }
        true
    }
}

/// Shared by all generated List methods of one caller env. One slot per
/// identity, so callers sharing the cache never invalidate each other. A chain
/// holds its pages' items for as long as it is cached, within 1 MiB per
/// chain and 4 MiB overall.
#[derive(Clone, Default)]
pub struct RevisionCache(Arc<Mutex<State>>);

/// One delivered page of a chain.
pub(crate) struct Cached<T> {
    items: Arc<[T]>,
    has_next: bool,
    revision: String,
    total: Option<Total>,
}

impl<T> Cached<T> {
    fn of(page: &Page<T>) -> Self {
        Self {
            items: Arc::clone(&page.items),
            has_next: page.has_next,
            revision: page.revision.clone(),
            total: page.total,
        }
    }
}

/// The pages of a walk in progress. A chain over [`HOLD_CHAIN_BYTES`] is
/// dropped as it grows, so a long walk never holds more than the budget. A
/// page without a revision makes the chain one that can never replay, so it
/// is not held either.
pub(crate) struct ChainBuilder<T> {
    pages: Vec<Cached<T>>,
    bytes: usize,
    unheld: bool,
}

impl<T> Default for ChainBuilder<T> {
    fn default() -> Self {
        Self {
            pages: Vec::new(),
            bytes: 0,
            unheld: false,
        }
    }
}

impl<T> ChainBuilder<T> {
    /// Adds `page`, whose items encode to `bytes`. The page is charged those
    /// bytes, [`PAGE_OVERHEAD`] and its revision's length.
    pub(crate) fn push(&mut self, page: &Page<T>, bytes: usize) {
        if self.unheld {
            return;
        }
        self.bytes = self
            .bytes
            .saturating_add(bytes)
            .saturating_add(PAGE_OVERHEAD)
            .saturating_add(page.revision.len());
        if page.revision.is_empty() || self.bytes > HOLD_CHAIN_BYTES {
            self.unheld = true;
            self.pages = Vec::new();
        } else {
            self.pages.push(Cached::of(page));
        }
    }
}

impl RevisionCache {
    /// Release every chain of every identity, for example when the host
    /// session ends. Walks in flight end as unauthenticated.
    ///
    /// # Panics
    /// Panics if the revision-cache mutex is poisoned.
    pub fn clear(&self) {
        self.0.lock().expect("revision cache lock").slots.clear();
    }

    /// Release the chains of one identity only; other identities keep theirs.
    ///
    /// # Panics
    /// Panics if the revision-cache mutex is poisoned.
    pub fn clear_identity(&self, identity: &str) {
        self.0
            .lock()
            .expect("revision cache lock")
            .slots
            .remove(identity);
    }

    /// Whether no complete replayable chain is registered.
    ///
    /// # Panics
    /// Panics if the revision-cache mutex is poisoned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0
            .lock()
            .expect("revision cache lock")
            .slots
            .values()
            .all(|slot| slot.entries.is_empty())
    }

    /// Opens the identity's slot when it has none. The empty identity names a
    /// walk whose credential could not be read: it never gets a slot, so it
    /// neither reads nor stores.
    pub(crate) fn identity(&self, identity: &str) {
        if identity.is_empty() {
            return;
        }
        let mut state = self.0.lock().expect("revision cache lock");
        if !state.slots.contains_key(identity) {
            let generation = state.next_generation;
            state.next_generation = generation.wrapping_add(1);
            state.slots.insert(
                identity.to_owned(),
                Slot {
                    generation,
                    entries: HashMap::new(),
                },
            );
        }
    }

    /// The generation of the identity's slot; `None` without one.
    pub(crate) fn generation(&self, identity: &str) -> Option<u64> {
        self.0
            .lock()
            .expect("revision cache lock")
            .slots
            .get(identity)
            .map(|slot| slot.generation)
    }

    pub(crate) fn replay<T: MaybeSend + MaybeSync + 'static>(
        &self,
        identity: &str,
        method: &'static str,
        key: [u8; 32],
    ) -> Option<Vec<Page<T>>> {
        let mut state = self.0.lock().expect("revision cache lock");
        state.clock = state.clock.wrapping_add(1);
        let used = state.clock;
        let entry = state
            .slots
            .get_mut(identity)?
            .entries
            .get_mut(&(method, key))?;
        let chain = entry.chain.downcast_ref::<Vec<Cached<T>>>()?;
        let revision = &chain.first()?.revision;
        if revision.is_empty() {
            return None;
        }
        let pages = chain
            .iter()
            .map(|cached| {
                (cached.revision == *revision).then(|| Page {
                    items: Arc::clone(&cached.items),
                    revision: cached.revision.clone(),
                    total: cached.total,
                    from_cache: true,
                    has_next: cached.has_next,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        entry.used = used;
        Some(pages)
    }

    /// Stores the finished `chain` under the walk's (identity, method, key),
    /// replacing what was there. A chain over the per-chain budget, or with a
    /// page that has no revision, is not stored and takes the key's previous
    /// chain with it, since that one
    /// belongs to a revision the walk has just seen replaced.
    pub(crate) fn store<T: MaybeSend + MaybeSync + 'static>(
        &self,
        identity: &str,
        generation: Option<u64>,
        method: &'static str,
        key: [u8; 32],
        chain: ChainBuilder<T>,
    ) {
        let mut state = self.0.lock().expect("revision cache lock");
        state.clock = state.clock.wrapping_add(1);
        let used = state.clock;
        let Some(slot) = state.slots.get_mut(identity) else {
            return;
        };
        if generation != Some(slot.generation) {
            return;
        }
        if chain.unheld {
            slot.entries.remove(&(method, key));
            return;
        }
        slot.entries.insert(
            (method, key),
            Entry {
                chain: Box::new(chain.pages),
                bytes: chain.bytes,
                used,
            },
        );
        while state.total_bytes() > HOLD_TOTAL_BYTES && state.evict_oldest() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(revision: &str) -> Page<u32> {
        Page {
            items: vec![1, 2].into(),
            revision: revision.into(),
            total: None,
            from_cache: false,
            has_next: false,
        }
    }

    /// A chain of `pages`, each encoding to one byte.
    fn chain(pages: &[&Page<u32>]) -> ChainBuilder<u32> {
        sized(pages, 1)
    }

    fn sized(pages: &[&Page<u32>], bytes: usize) -> ChainBuilder<u32> {
        let mut chain = ChainBuilder::default();
        for page in pages {
            chain.push(page, bytes);
        }
        chain
    }

    fn put(cache: &RevisionCache, identity: &str, method: &'static str, built: ChainBuilder<u32>) {
        cache.identity(identity);
        cache.store(identity, cache.generation(identity), method, [1; 32], built);
    }

    #[test]
    fn a_chain_outlives_its_callers_and_needs_one_revision() {
        let cache = RevisionCache::default();
        cache.identity("alice:org-a");
        let first = page("r1");
        let second = page("r1");
        cache.store(
            "alice:org-a",
            cache.generation("alice:org-a"),
            "p.S/List",
            [1; 32],
            chain(&[&first, &second]),
        );
        drop(second);
        let replay = cache
            .replay::<u32>("alice:org-a", "p.S/List", [1; 32])
            .expect("complete chain");
        assert_eq!(replay.len(), 2);
        assert!(replay.iter().all(|page| page.from_cache));
        assert!(Arc::ptr_eq(&replay[0].items, &first.items));
        assert_eq!(&*replay[1].items, &[1, 2]);
        let second = page("r2");
        cache.store(
            "alice:org-a",
            cache.generation("alice:org-a"),
            "p.S/List",
            [1; 32],
            chain(&[&first, &second]),
        );
        assert!(
            cache
                .replay::<u32>("alice:org-a", "p.S/List", [1; 32])
                .is_none()
        );
    }

    #[test]
    fn a_chain_without_a_revision_or_pages_never_replays() {
        let cache = RevisionCache::default();
        put(&cache, "alice", "p.S/List", chain(&[&page("")]));
        assert!(cache.replay::<u32>("alice", "p.S/List", [1; 32]).is_none());
        put(&cache, "alice", "p.S/List", ChainBuilder::default());
        assert!(cache.replay::<u32>("alice", "p.S/List", [1; 32]).is_none());
    }

    #[test]
    fn two_identities_interleave_and_neither_invalidates_the_other() {
        let cache = RevisionCache::default();
        cache.identity("alice");
        let alice_generation = cache.generation("alice");
        let alice_page = page("r1");
        cache.identity("bob");
        let bob_page = page("r9");
        cache.store(
            "alice",
            cache.generation("alice"),
            "p.S/List",
            [1; 32],
            chain(&[&alice_page]),
        );
        cache.store(
            "bob",
            cache.generation("bob"),
            "p.S/List",
            [1; 32],
            chain(&[&bob_page]),
        );
        cache.identity("alice");
        assert_eq!(cache.generation("alice"), alice_generation);
        assert_ne!(cache.generation("bob"), alice_generation);
        let alice = cache
            .replay::<u32>("alice", "p.S/List", [1; 32])
            .expect("alice's chain");
        let bob = cache
            .replay::<u32>("bob", "p.S/List", [1; 32])
            .expect("bob's chain");
        assert_eq!(alice[0].revision, "r1");
        assert_eq!(bob[0].revision, "r9");
    }

    #[test]
    fn clear_identity_drops_one_slot_and_clear_drops_all() {
        let cache = RevisionCache::default();
        let held = page("r1");
        for who in ["alice", "bob"] {
            put(&cache, who, "p.S/List", chain(&[&held]));
        }
        let bob_generation = cache.generation("bob");
        let alice_generation = cache.generation("alice");
        cache.clear_identity("alice");
        assert_eq!(cache.generation("alice"), None);
        assert!(cache.replay::<u32>("alice", "p.S/List", [1; 32]).is_none());
        assert_eq!(cache.generation("bob"), bob_generation);
        assert!(cache.replay::<u32>("bob", "p.S/List", [1; 32]).is_some());
        assert!(!cache.is_empty());
        cache.identity("alice");
        assert_ne!(
            cache.generation("alice"),
            alice_generation,
            "a dropped slot's generation is never reused"
        );
        cache.clear();
        assert_eq!(cache.generation("bob"), None);
        assert!(cache.is_empty());
    }

    #[test]
    fn a_dropped_slot_cannot_be_repopulated_by_a_stale_store() {
        let cache = RevisionCache::default();
        cache.identity("alice");
        let held = page("r1");
        cache.clear_identity("alice");
        cache.store(
            "alice",
            cache.generation("alice"),
            "p.S/List",
            [1; 32],
            chain(&[&held]),
        );
        assert!(cache.is_empty());
    }

    #[test]
    fn a_slot_recreated_mid_walk_ignores_the_stale_store() {
        let cache = RevisionCache::default();
        cache.identity("alice");
        let walk_generation = cache.generation("alice");
        cache.clear_identity("alice");
        cache.identity("alice");
        let held = page("r1");
        cache.store(
            "alice",
            walk_generation,
            "p.S/List",
            [1; 32],
            chain(&[&held]),
        );
        assert!(cache.replay::<u32>("alice", "p.S/List", [1; 32]).is_none());
        assert!(cache.is_empty());
    }

    #[test]
    fn the_unreadable_identity_never_gets_a_slot() {
        let cache = RevisionCache::default();
        cache.identity("");
        assert_eq!(cache.generation(""), None);
        let held = page("r1");
        cache.store(
            "",
            cache.generation(""),
            "p.S/List",
            [1; 32],
            chain(&[&held]),
        );
        assert!(cache.is_empty());
        assert!(cache.replay::<u32>("", "p.S/List", [1; 32]).is_none());
    }

    #[test]
    fn method_request_and_item_type_partition_chains() {
        let cache = RevisionCache::default();
        let held = page("r1");
        put(&cache, "alice:org-a", "p.S/List", chain(&[&held]));
        assert!(
            cache
                .replay::<String>("alice:org-a", "p.S/List", [1; 32])
                .is_none()
        );
        assert!(
            cache
                .replay::<u32>("alice:org-a", "p.S/Other", [1; 32])
                .is_none()
        );
        assert!(
            cache
                .replay::<u32>("alice:org-a", "p.S/List", [2; 32])
                .is_none()
        );
    }

    #[test]
    fn a_new_walk_replaces_the_chain_of_its_key() {
        let cache = RevisionCache::default();
        put(&cache, "alice", "p.S/List", chain(&[&page("r1")]));
        put(&cache, "alice", "p.S/List", chain(&[&page("r2")]));
        let replay = cache
            .replay::<u32>("alice", "p.S/List", [1; 32])
            .expect("the newer chain");
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].revision, "r2");
    }

    #[test]
    fn a_chain_at_the_budget_is_held_and_one_byte_over_is_not() {
        let cache = RevisionCache::default();
        put(
            &cache,
            "alice",
            "p.S/List",
            sized(&[&page("r1")], HOLD_CHAIN_BYTES - PAGE_OVERHEAD - 2),
        );
        assert!(cache.replay::<u32>("alice", "p.S/List", [1; 32]).is_some());
        put(
            &cache,
            "alice",
            "p.S/Edge",
            sized(&[&page("r1")], HOLD_CHAIN_BYTES - PAGE_OVERHEAD - 1),
        );
        assert!(cache.replay::<u32>("alice", "p.S/Edge", [1; 32]).is_none());
        put(
            &cache,
            "alice",
            "p.S/Other",
            sized(&[&page("r1"), &page("r1")], HOLD_CHAIN_BYTES / 2 + 1),
        );
        assert!(cache.replay::<u32>("alice", "p.S/Other", [1; 32]).is_none());
    }

    #[test]
    fn an_overflowing_walk_stops_holding_pages_and_evicts_the_older_revision() {
        let mut builder = ChainBuilder::default();
        let held = page("r2");
        builder.push(&held, HOLD_CHAIN_BYTES - PAGE_OVERHEAD - 2);
        assert_eq!(Arc::strong_count(&held.items), 2);
        builder.push(&held, 1);
        assert_eq!(Arc::strong_count(&held.items), 1, "no page is retained");
        builder.push(&held, 1);
        assert_eq!(
            builder.bytes,
            HOLD_CHAIN_BYTES + 1 + PAGE_OVERHEAD + 2,
            "an overflowed chain stops counting"
        );

        let cache = RevisionCache::default();
        put(&cache, "alice", "p.S/List", chain(&[&page("r1")]));
        put(&cache, "alice", "p.S/List", builder);
        assert!(
            cache.replay::<u32>("alice", "p.S/List", [1; 32]).is_none(),
            "the superseded revision does not outlive the walk that replaced it"
        );
        assert!(cache.is_empty());
    }

    #[test]
    fn the_least_recently_used_chain_goes_when_the_total_is_over_budget() {
        let cache = RevisionCache::default();
        // Each chain costs a quarter of the total with its page's overhead
        // and the two bytes of its revision.
        let quarter = HOLD_TOTAL_BYTES / 4 - PAGE_OVERHEAD - 2;
        for method in ["p.S/A", "p.S/B", "p.S/C"] {
            put(&cache, "alice", method, sized(&[&page("r1")], quarter));
        }
        put(&cache, "bob", "p.S/A", sized(&[&page("r1")], quarter));
        // Reading A refreshes it, so the next eviction takes B.
        assert!(cache.replay::<u32>("alice", "p.S/A", [1; 32]).is_some());
        put(&cache, "bob", "p.S/B", sized(&[&page("r1")], 1));
        assert!(cache.replay::<u32>("alice", "p.S/B", [1; 32]).is_none());
        for kept in [
            ("alice", "p.S/A"),
            ("alice", "p.S/C"),
            ("bob", "p.S/A"),
            ("bob", "p.S/B"),
        ] {
            assert!(
                cache.replay::<u32>(kept.0, kept.1, [1; 32]).is_some(),
                "{kept:?}"
            );
        }
        assert!(cache.0.lock().expect("lock").total_bytes() <= HOLD_TOTAL_BYTES);
    }

    #[test]
    fn a_chain_of_item_less_pages_is_charged_for_every_page() {
        let empty = Page::<u32> {
            items: Vec::new().into(),
            ..page("r1")
        };
        let mut builder = ChainBuilder::default();
        let fits = HOLD_CHAIN_BYTES / (PAGE_OVERHEAD + 2);
        for _ in 0..fits {
            builder.push(&empty, 0);
        }
        assert!(!builder.unheld, "{fits} item-less pages fit the budget");
        builder.push(&empty, 0);
        assert!(builder.unheld, "one more page is over it");
        assert!(builder.pages.is_empty(), "nothing is retained");
    }

    #[test]
    fn a_revision_is_charged_by_its_length_and_item_less_chains_are_evicted() {
        let cache = RevisionCache::default();
        let revision = "r".repeat(1 << 16);
        let empty = Page::<u32> {
            items: Vec::new().into(),
            ..page(&revision)
        };
        let stored = 2 * HOLD_TOTAL_BYTES / (1 << 16);
        cache.identity("alice");
        for n in 0..stored {
            let mut built = ChainBuilder::default();
            built.push(&empty, 0);
            let key = [u8::try_from(n).expect("a byte"); 32];
            cache.store("alice", cache.generation("alice"), "p.S/List", key, built);
        }
        let held = (0..stored)
            .filter(|n| {
                let key = [u8::try_from(*n).expect("a byte"); 32];
                cache.replay::<u32>("alice", "p.S/List", key).is_some()
            })
            .count();
        assert!(held > 0 && held < stored, "{held} of {stored} chains held");
        assert!(cache.0.lock().expect("lock").total_bytes() <= HOLD_TOTAL_BYTES);
    }

    #[test]
    fn a_chain_with_a_page_without_a_revision_is_not_held_and_replaces_nothing_older() {
        let mut builder = ChainBuilder::default();
        builder.push(&page("r1"), 1);
        builder.push(&page(""), 1);
        assert!(builder.unheld);
        assert!(builder.pages.is_empty());
        let cache = RevisionCache::default();
        put(&cache, "alice", "p.S/List", chain(&[&page("r1")]));
        put(&cache, "alice", "p.S/List", builder);
        assert!(cache.is_empty(), "the superseded chain is dropped with it");
    }

    #[test]
    fn nothing_is_evicted_from_an_empty_cache() {
        assert!(!State::default().evict_oldest());
    }
}
