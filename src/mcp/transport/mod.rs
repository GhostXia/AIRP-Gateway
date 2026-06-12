//! Transport abstraction for talking to an upstream MCP server.
//!
//! A single trait, [`McpTransport`], hides whether the server is reached over
//! a stdio child process or streamable HTTP. The bridge and client never
//! branch on transport kind — add a new transport by implementing this trait.

use async_trait::async_trait;

use crate::config::TransportConfig;
use crate::error::Result;
use crate::mcp::types::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

pub mod http;
pub mod stdio;

/// A bidirectional JSON-RPC channel to one MCP server.
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Send a request and await its matching response.
    async fn request(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse>;

    /// Fire a notification (no response expected).
    async fn notify(&self, note: JsonRpcNotification) -> Result<()>;

    // TODO(streaming): a `request_stream` returning a `Stream<Item = ServerEvent>`
    // for tools whose results arrive incrementally (mapped to frontend SSE).
}

/// Construct a transport from its declarative config.
pub async fn connect(cfg: &TransportConfig) -> Result<Box<dyn McpTransport>> {
    match cfg {
        TransportConfig::Stdio { command, args, cwd } => {
            let t = stdio::StdioTransport::connect(command, args, cwd.as_deref()).await?;
            Ok(Box::new(t))
        }
        TransportConfig::Http { url, auth_token } => {
            let t = http::HttpTransport::new(url, auth_token.clone())?;
            Ok(Box::new(t))
        }
    }
}
