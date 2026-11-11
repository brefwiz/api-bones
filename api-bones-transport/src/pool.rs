// SPDX-License-Identifier: MIT
//! One connection per peer, opened once however many requests ask at once.

use std::collections::HashMap;
use std::error::Error as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use http::Uri;
use tokio::sync::Mutex as AsyncMutex;

use crate::classify::TransportError;
use crate::conn::Conn;
use crate::connector::Connector;
use crate::peer;

/// A peer's connection, and the failure of the dial that last failed.
#[derive(Default)]
struct Slot {
    /// Counts failed dials. A caller reads it before waiting for `state`, so
    /// it can tell a failure that happened while it waited from an older one.
    epoch: AtomicU64,
    state: AsyncMutex<SlotState>,
}

#[derive(Default)]
struct SlotState {
    conn: Option<Arc<Conn>>,
    /// The epoch and message of the last failed dial.
    failed: Option<(u64, String)>,
}

/// The connections of a transport, by peer (scheme, host and port).
pub(crate) struct Pool {
    connector: Connector,
    peers: Mutex<HashMap<String, Arc<Slot>>>,
}

impl Pool {
    pub(crate) fn new(connector: Connector) -> Self {
        Self {
            connector,
            peers: Mutex::default(),
        }
    }

    /// The open connection to the peer `uri` names. A burst of callers to a
    /// peer with no connection waits on one connect: the peer's slot is held
    /// across it, and when that connect fails every caller that queued before
    /// it failed gets its failure instead of dialing in turn. A connection
    /// flagged closed (it received a GOAWAY, failed its keepalive, idled out,
    /// or a replay moved off it) is replaced.
    pub(crate) async fn acquire(&self, uri: &Uri) -> Result<Arc<Conn>, TransportError> {
        let slot = {
            // The map only gains entries, so a panic while holding the lock
            // cannot leave it half-written.
            let mut peers = self.peers.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(peers.entry(key(uri)).or_default())
        };
        let ticket = slot.epoch.load(Ordering::SeqCst);
        let mut state = slot.state.lock().await;
        if let Some(conn) = state.conn.as_ref().filter(|conn| !conn.is_closed()) {
            return Ok(Arc::clone(conn));
        }
        if let Some((_, message)) = state.failed.as_ref().filter(|(epoch, _)| *epoch > ticket) {
            return Err(TransportError::connect(message.clone()));
        }
        match Conn::dial(&self.connector, uri).await {
            Ok(conn) => {
                state.conn = Some(Arc::clone(&conn));
                Ok(conn)
            }
            Err(error) => {
                let message = error
                    .source()
                    .map_or_else(|| error.to_string(), ToString::to_string);
                let epoch = slot.epoch.fetch_add(1, Ordering::SeqCst) + 1;
                state.failed = Some((epoch, message));
                Err(error)
            }
        }
    }
}

impl Drop for Pool {
    /// Connections do not outlive the transport that owns them: each is
    /// flagged closed, finishes the requests it has, and ends. No request
    /// holds the pool while it is dropped, so no slot is locked.
    fn drop(&mut self) {
        let peers = self.peers.get_mut().unwrap_or_else(PoisonError::into_inner);
        for slot in peers.values() {
            let pooled = slot
                .state
                .try_lock()
                .ok()
                .and_then(|state| state.conn.clone());
            if let Some(conn) = pooled {
                conn.close();
            }
        }
    }
}

fn key(uri: &Uri) -> String {
    format!("{}://{}", uri.scheme_str().unwrap_or(""), peer::name(uri))
}

#[cfg(test)]
mod tests {
    #[test]
    fn peers_differ_by_scheme_host_and_port() {
        let key = |uri: &str| super::key(&uri.parse().expect("uri"));
        assert_eq!(key("http://a:80/x"), key("http://a:80/y"));
        assert_ne!(key("http://a:80/x"), key("https://a:80/x"));
        assert_ne!(key("http://a:80/x"), key("http://a:81/x"));
        assert_ne!(key("http://a:80/x"), key("http://b:80/x"));
        assert_eq!(key("http://u:p@a:80/x"), key("http://a:80/x"));
    }
}
