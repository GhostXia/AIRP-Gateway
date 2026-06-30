//! Broadcast bus: connects dispatch results to SSE subscriber streams.
//!
//! Each SSE connection holds a receiver. The dispatcher sends downstream
//! [`Envelope`]s via the sender. Connections filter by their subscribed scopes
//! (set via the `subscribe` intent); if a connection has no scope filter, it
//! receives everything.

use std::sync::Arc;

use tokio::sync::{broadcast, Mutex};

use super::envelope::Envelope;

/// How many envelopes to buffer per broadcast channel. Old envelopes are
/// dropped when the buffer is full (slow consumers lose messages — acceptable
/// for a real-time UI where stale state is worse than a gap).
const BROADCAST_CAPACITY: usize = 256;

/// Shared between the dispatch handler (sender) and every SSE stream (receiver).
#[derive(Debug, Clone)]
pub struct Bus {
    tx: broadcast::Sender<Envelope>,
    /// Per-connection scope subscriptions. Keyed by a connection id (the SSE
    /// stream's unique id, assigned at connect time).
    subscriptions: Arc<Mutex<std::collections::HashMap<u64, Vec<String>>>>,
}

impl Bus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            tx,
            subscriptions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Register a new connection. Returns (conn_id, receiver).
    pub fn subscribe(&self) -> (u64, broadcast::Receiver<Envelope>) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_CONN: AtomicU64 = AtomicU64::new(1);
        let conn_id = NEXT_CONN.fetch_add(1, Ordering::Relaxed);
        let rx = self.tx.subscribe();
        (conn_id, rx)
    }

    /// Set the scope filter for a connection. Replaces the previous set.
    /// Empty vector = receive all scopes (no filter).
    pub async fn set_scopes(&self, conn_id: u64, scopes: Vec<String>) {
        let mut map = self.subscriptions.lock().await;
        if scopes.is_empty() {
            map.remove(&conn_id);
        } else {
            map.insert(conn_id, scopes);
        }
    }

    /// Remove a connection when its SSE stream ends.
    pub async fn disconnect(&self, conn_id: u64) {
        let mut map = self.subscriptions.lock().await;
        map.remove(&conn_id);
    }

    /// Broadcast an envelope to all connections.
    ///
    /// `broadcast::Sender` does not support per-receiver filtering, so the
    /// envelope is sent to every subscriber and each SSE stream filters on
    /// its own side via [`wants_scope`]. The channel is bounded; slow
    /// consumers lag (envelopes are dropped, not blocked).
    pub async fn send(&self, envelope: Envelope) {
        let _ = self.tx.send(envelope);
    }

    /// Check whether a connection wants envelopes for a given scope.
    /// Returns true if the connection has no filter or the scope is in its list.
    pub async fn wants_scope(&self, conn_id: u64, scope: Option<&str>) -> bool {
        let map = self.subscriptions.lock().await;
        match map.get(&conn_id) {
            None => true, // no filter = receive all
            Some(scopes) => match scope {
                None => true, // non-scope envelope (blueprint/manifest/error) = always
                Some(s) => scopes.iter().any(|fs| fs == s),
            },
        }
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}
