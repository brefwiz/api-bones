// SPDX-License-Identifier: MIT
//! Client-scoped revision chains containing weak item references only.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use crate::{Page, Total};

type Entries = HashMap<(&'static str, [u8; 32]), Box<dyn Any + Send + Sync>>;

#[derive(Default)]
struct State {
    generation: u64,
    identity: Option<String>,
    entries: Entries,
}

/// Shared by all generated List methods of one client. Item data is retained
/// only while a caller holds the delivered page's item allocation.
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
    /// Release every chain, for example when the client session ends.
    ///
    /// # Panics
    /// Panics if the revision-cache mutex is poisoned.
    pub fn clear(&self) {
        let mut state = self.0.lock().expect("revision cache lock");
        state.entries.clear();
        state.generation = state.generation.wrapping_add(1);
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
            .entries
            .is_empty()
    }

    pub(crate) fn identity(&self, identity: &str) {
        let mut state = self.0.lock().expect("revision cache lock");
        if state.identity.as_deref() != Some(identity) {
            state.entries.clear();
            state.generation = state.generation.wrapping_add(1);
            state.identity = Some(identity.to_owned());
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.0.lock().expect("revision cache lock").generation
    }

    pub(crate) fn replay<T: Send + Sync + 'static>(
        &self,
        method: &'static str,
        key: [u8; 32],
    ) -> Option<Vec<Page<T>>> {
        let state = self.0.lock().expect("revision cache lock");
        let chain = state
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

    pub(crate) fn store<T: Send + Sync + 'static>(
        &self,
        identity: &str,
        method: &'static str,
        key: [u8; 32],
        chain: Vec<Cached<T>>,
    ) {
        let mut state = self.0.lock().expect("revision cache lock");
        if state.identity.as_deref() != Some(identity) {
            return;
        }
        state.entries.insert((method, key), Box::new(chain));
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
            "p.S/List",
            [1; 32],
            vec![Cached::of(&first), Cached::of(&second)],
        );
        let replay = cache
            .replay::<u32>("p.S/List", [1; 32])
            .expect("complete live chain");
        assert_eq!(replay.len(), 2);
        assert!(replay.iter().all(|page| page.from_cache));
        assert!(Arc::ptr_eq(&replay[0].items, &first.items));
        drop(replay);
        drop(second);
        assert!(cache.replay::<u32>("p.S/List", [1; 32]).is_none());
        let second = page("r2");
        cache.store(
            "alice:org-a",
            "p.S/List",
            [1; 32],
            vec![Cached::of(&first), Cached::of(&second)],
        );
        assert!(cache.replay::<u32>("p.S/List", [1; 32]).is_none());
    }

    #[test]
    fn identity_and_clear_invalidate_in_flight_generations() {
        let cache = RevisionCache::default();
        cache.identity("alice:org-a");
        let held = page("r1");
        cache.store("alice:org-a", "p.S/List", [1; 32], vec![Cached::of(&held)]);
        let generation = cache.generation();
        cache.identity("alice:org-b");
        assert!(cache.is_empty());
        assert_ne!(cache.generation(), generation);
        cache.store("alice:org-a", "p.S/List", [1; 32], vec![Cached::of(&held)]);
        assert!(cache.is_empty(), "old identity cannot repopulate cache");
        let generation = cache.generation();
        cache.clear();
        assert_ne!(cache.generation(), generation);
    }

    #[test]
    fn method_request_and_item_type_partition_chains() {
        let cache = RevisionCache::default();
        cache.identity("alice:org-a");
        let held = page("r1");
        cache.store("alice:org-a", "p.S/List", [1; 32], vec![Cached::of(&held)]);
        assert!(cache.replay::<String>("p.S/List", [1; 32]).is_none());
        assert!(cache.replay::<u32>("p.S/Other", [1; 32]).is_none());
        assert!(cache.replay::<u32>("p.S/List", [2; 32]).is_none());
    }
}
