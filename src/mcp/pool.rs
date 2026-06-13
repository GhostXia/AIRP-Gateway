//! Registry of live MCP clients, keyed by upstream name.

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::UpstreamConfig;
use crate::error::Result;
use crate::mcp::{client::McpClient, transport};

#[derive(Clone, Default)]
pub struct UpstreamPool {
    clients: HashMap<String, Arc<McpClient>>,
}

impl UpstreamPool {
    /// Build a client per configured upstream. Transports connect here; the
    /// MCP `initialize` handshake is deferred to first use (see [`McpClient`]).
    pub async fn from_config(upstreams: &[UpstreamConfig]) -> Result<Self> {
        let mut clients = HashMap::new();
        for up in upstreams {
            let t = transport::connect(&up.transport).await?;
            let client = McpClient::new(up.name.clone(), Arc::from(t));
            clients.insert(up.name.clone(), Arc::new(client));
        }
        Ok(Self { clients })
    }

    /// Register a client directly. Useful for composing pools by hand (e.g.
    /// tests with a mock transport, or hosts wiring clients programmatically).
    pub fn insert(&mut self, name: impl Into<String>, client: Arc<McpClient>) {
        self.clients.insert(name.into(), client);
    }

    pub fn get(&self, name: &str) -> Option<Arc<McpClient>> {
        self.clients.get(name).cloned()
    }

    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.clients.keys()
    }
}
