//! Per-upstream MCP client: owns a transport, performs the `initialize`
//! handshake, and exposes the small set of MCP calls the bridge needs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::OnceCell;

use crate::error::{GatewayError, Result};
use crate::mcp::transport::McpTransport;
use crate::mcp::types::{
    resource_read_params, tool_call_params, JsonRpcNotification, JsonRpcRequest,
    MCP_PROTOCOL_VERSION,
};

pub struct McpClient {
    pub name: String,
    transport: Arc<dyn McpTransport>,
    next_id: AtomicU64,
    /// `initialize` is run once, lazily, on first use. Holds the *negotiated*
    /// protocol version the server replied with.
    negotiated: OnceCell<String>,
}

impl McpClient {
    pub fn new(name: impl Into<String>, transport: Arc<dyn McpTransport>) -> Self {
        Self {
            name: name.into(),
            transport,
            next_id: AtomicU64::new(1),
            negotiated: OnceCell::new(),
        }
    }

    fn next_id(&self) -> Value {
        Value::from(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Run the MCP `initialize` handshake exactly once.
    pub async fn ensure_initialized(&self) -> Result<()> {
        self.negotiated
            .get_or_try_init(|| async {
                let params = json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "airp-gateway", "version": env!("CARGO_PKG_VERSION") }
                });
                let req = JsonRpcRequest::new(self.next_id(), "initialize", Some(params));
                let resp = self.transport.request(req).await?;
                if let Some(err) = resp.error {
                    return Err(GatewayError::Upstream {
                        code: err.code,
                        message: err.message,
                    });
                }
                // Capture the *negotiated* protocol version. The server MAY answer
                // with a different version than we advertised (spec allows this);
                // the HTTP transport must echo it in the `MCP-Protocol-Version`
                // header on subsequent requests.
                let version = resp
                    .result
                    .as_ref()
                    .and_then(|r| r.get("protocolVersion"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(MCP_PROTOCOL_VERSION)
                    .to_string();
                // Per spec, follow up with the `initialized` notification.
                let note = JsonRpcNotification::new("notifications/initialized", None);
                self.transport.notify(note).await?;
                Ok(version)
            })
            .await
            .map(|_| ())
    }

    /// The protocol version negotiated with the upstream during `initialize`,
    /// or `None` if it has not run yet. Use this (not the advertised constant)
    /// for the HTTP `MCP-Protocol-Version` header.
    pub fn protocol_version(&self) -> Option<String> {
        self.negotiated.get().cloned()
    }

    /// Call an MCP tool, returning its `result` payload.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        self.ensure_initialized().await?;
        let params = tool_call_params(name, arguments);
        self.invoke("tools/call", params).await
    }

    /// Read an MCP resource, returning its `result` payload.
    pub async fn read_resource(&self, uri: &str) -> Result<Value> {
        self.ensure_initialized().await?;
        let params = resource_read_params(uri);
        self.invoke("resources/read", params).await
    }

    async fn invoke(&self, method: &str, params: Value) -> Result<Value> {
        let req = JsonRpcRequest::new(self.next_id(), method, Some(params));
        let resp = self.transport.request(req).await?;
        if let Some(err) = resp.error {
            return Err(GatewayError::Upstream {
                code: err.code,
                message: err.message,
            });
        }
        Ok(resp.result.unwrap_or(Value::Null))
    }
}
