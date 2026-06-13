//! Streamable-HTTP MCP transport.
//!
//! Implements the non-streaming request/response path against an MCP server's
//! HTTP endpoint (e.g. AIRP-MCP-Server's `/mcp/v1`). Session-id negotiation and
//! SSE-framed streaming responses are scaffolded as TODOs.

use async_trait::async_trait;

use crate::error::{GatewayError, Result};
use crate::mcp::transport::McpTransport;
use crate::mcp::types::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

pub struct HttpTransport {
    client: reqwest::Client,
    url: String,
    auth_token: Option<String>,
    // TODO(session): MCP streamable-HTTP returns an `Mcp-Session-Id` header on
    // initialize that must be echoed on subsequent requests. Store it here.
}

impl HttpTransport {
    pub fn new(url: &str, auth_token: Option<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| GatewayError::Transport(e.to_string()))?;
        Ok(Self {
            client,
            url: url.to_string(),
            auth_token,
        })
    }

    fn apply_auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth_token {
            Some(t) => rb.bearer_auth(t),
            None => rb,
        }
    }
}

#[async_trait]
impl McpTransport for HttpTransport {
    async fn request(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let rb = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            // MCP streamable-HTTP clients advertise they accept both framings.
            .header("accept", "application/json, text/event-stream")
            .json(&req);

        let resp = self
            .apply_auth(rb)
            .send()
            .await
            .map_err(|e| GatewayError::Transport(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(GatewayError::Transport(format!(
                "upstream HTTP {}",
                resp.status()
            )));
        }

        // TODO(streaming): if Content-Type is text/event-stream, parse the SSE
        // frames instead of decoding a single JSON body.
        let parsed = resp
            .json::<JsonRpcResponse>()
            .await
            .map_err(|e| GatewayError::Transport(e.to_string()))?;
        Ok(parsed)
    }

    async fn notify(&self, note: JsonRpcNotification) -> Result<()> {
        let rb = self
            .client
            .post(&self.url)
            .header("content-type", "application/json")
            .json(&note);
        self.apply_auth(rb)
            .send()
            .await
            .map_err(|e| GatewayError::Transport(e.to_string()))?;
        Ok(())
    }
}
