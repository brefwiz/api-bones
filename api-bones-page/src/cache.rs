// SPDX-License-Identifier: MIT
//! Caller-scoped revision chains containing weak item references only.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use api_bones_rt::{MaybeSend, MaybeSync};

use crate::{Page, Total};

#[cfg(not(target_arch = "wasm32"))]
type Chain = Box<dyn Any + Send + Sync>;
#[cfg(target_arch = "wasm32")]
type Chain = Box<dyn Any>;

type Entries = HashMap<(&'static str, [u8; 32]), Chain>;

/// What one identity has cached. Its generation names the slot's life: a
/// walk that started under another generation has outlived its cache.
struct Slot {
    generation: u64,
    entries: Entries,
}

#[derive(Default)]
struct State {
    /// Never reused, so a dropped slot's walks can never match a new one.
    next_generation: u64,
    slots: HashMap<String, Slot>,
}

/// Shared by all generated List methods of one caller env. One slot per
/// identity, so callers sharing the cache never invalidate each other. Item
/// data is retained only while a caller holds the delivered page's item
/// allocation.
#[derive(Clone, Default)]
pub struct RevisionCache(Arc<Mutex<State>>);

pub(crate) struct Cached<T> {
    items: Weak<[T]>,
    has_next: bool,
    revision: String,
    total: Option<Total>,
}

impl<T> Cached<T> {
    pub(crate) fn of(page: &Page<T>) -> Self {
        Self {
            items: Arc::downgrade(&page.items),
            has_next: page.has_next,
            revision: page.revision.clone(),
            total: page.total,
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
                    entries: Entries::new(),
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
        let state = self.0.lock().expect("revision cache lock");
        let chain = state
            .slots
            .get(identity)?
            .entries
            .get(&(method, key))?
            .downcast_ref::<Vec<Cached<T>>>()?;
        let revision = &chain.first()?.revision;
        if revision.is_empty() {
            return None;
        }
        chain
            .iter()
            .map(|entry| {
                if entry.revision != *revision {
                    return None;
                }
                Some(Page {
                    items: entry.items.upgrade()?,
                    revision: entry.revision.clone(),
                    total: entry.total,
                    from_cache: true,
                    has_next: entry.has_next,
                })
            })
            .collect()
    }

    pub(crate) fn store<T: MaybeSend + MaybeSync + 'static>(
        &self,
        identity: &str,
        generation: Option<u64>,
        method: &'static str,
        key: [u8; 32],
        chain: Vec<Cached<T>>,
    ) {
        let mut state = self.0.lock().expect("revision cache lock");
        if let Some(slot) = state.slots.get_mut(identity)
            && generation == Some(slot.generation)
        {
            slot.entries.insert((method, key), Box::new(chain));
        }
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

    #[test]
    fn weak_chain_requires_live_holders_and_one_revision() {
        let cache = RevisionCache::default();
        cache.identity("alice:org-a");
        let first = page("r1");
        let second = page("r1");
        cache.store(
            "alice:org-a",
            cache.generation("alice:org-a"),
            "p.S/List",
            [1; 32],
            vec![Cached::of(&first), Cached::of(&second)],
        );
        let replay = cache
            .replay::<u32>("alice:org-a", "p.S/List", [1; 32])
            .expect("complete live chain");
        assert_eq!(replay.len(), 2);
        assert!(replay.iter().all(|page| page.from_cache));
        assert!(Arc::ptr_eq(&replay[0].items, &first.items));
        drop(replay);
        drop(second);
        assert!(
            cache
                .replay::<u32>("alice:org-a", "p.S/List", [1; 32])
                .is_none()
        );
        let second = page("r2");
        cache.store(
            "alice:org-a",
            cache.generation("alice:org-a"),
            "p.S/List",
            [1; 32],
            vec![Cached::of(&first), Cached::of(&second)],
        );
        assert!(
            cache
                .replay::<u32>("alice:org-a", "p.S/List", [1; 32])
                .is_none()
        );
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
            vec![Cached::of(&alice_page)],
        );
        cache.store(
            "bob",
            cache.generation("bob"),
            "p.S/List",
            [1; 32],
            vec![Cached::of(&bob_page)],
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
            cache.identity(who);
            cache.store(
                who,
                cache.generation(who),
                "p.S/List",
                [1; 32],
                vec![Cached::of(&held)],
            );
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
            vec![Cached::of(&held)],
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
            vec![Cached::of(&held)],
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
            vec![Cached::of(&held)],
        );
        assert!(cache.is_empty());
        assert!(cache.replay::<u32>("", "p.S/List", [1; 32]).is_none());
    }

    #[test]
    fn method_request_and_item_type_partition_chains() {
        let cache = RevisionCache::default();
        cache.identity("alice:org-a");
        let held = page("r1");
        cache.store(
            "alice:org-a",
            cache.generation("alice:org-a"),
            "p.S/List",
            [1; 32],
            vec![Cached::of(&held)],
        );
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
}
