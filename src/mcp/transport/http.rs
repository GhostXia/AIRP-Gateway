//! Streamable-HTTP MCP transport.
//!
//! Implements the request/response path against an MCP server's HTTP endpoint
//! (e.g. an `/mcp/v1` mounted by rmcp's streamable-http server). Handles the
//! parts the spec requires of a client:
//! - captures the `Mcp-Session-Id` returned on `initialize` and echoes it on
//!   every subsequent request;
//! - captures the *negotiated* `protocolVersion` and sends it as the
//!   `MCP-Protocol-Version` header on subsequent requests;
//! - accepts either `application/json` or `text/event-stream` (SSE) responses.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::{GatewayError, Result};
use crate::mcp::transport::McpTransport;
use crate::mcp::types::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

pub struct HttpTransport {
    client: reqwest::Client,
    url: String,
    auth_token: Option<String>,
    /// Set from the `Mcp-Session-Id` response header on `initialize`.
    session_id: Mutex<Option<String>>,
    /// Negotiated protocol version (server's reply to `initialize`).
    protocol_version: Mutex<Option<String>>,
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
            session_id: Mutex::new(None),
            protocol_version: Mutex::new(None),
        })
    }

    /// Add the standard headers (auth, content negotiation, session, version).
    /// Guards are cloned out and dropped here — none are held across `.await`.
    fn decorate(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut rb = rb
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(t) = &self.auth_token {
            rb = rb.bearer_auth(t);
        }
        if let Some(sid) = self.session_id.lock().unwrap().clone() {
            rb = rb.header("Mcp-Session-Id", sid);
        }
        if let Some(ver) = self.protocol_version.lock().unwrap().clone() {
            rb = rb.header("MCP-Protocol-Version", ver);
        }
        rb
    }
}

#[async_trait]
impl McpTransport for HttpTransport {
    async fn request(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let rb = self.decorate(self.client.post(&self.url).json(&req));
        let resp = rb
            .send()
            .await
            .map_err(|e| GatewayError::Transport(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            return Err(GatewayError::Transport(format!("upstream HTTP {status}")));
        }

        // Capture session id (sent on the initialize response) before consuming body.
        if let Some(sid) = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
        {
            *self.session_id.lock().unwrap() = Some(sid);
        }
        let is_sse = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|c| c.contains("text/event-stream"))
            .unwrap_or(false);

        let text = resp
            .text()
            .await
            .map_err(|e| GatewayError::Transport(e.to_string()))?;

        let parsed = if is_sse {
            parse_sse(&text)?
        } else {
            serde_json::from_str::<JsonRpcResponse>(&text)
                .map_err(|e| GatewayError::Transport(format!("decode JSON-RPC: {e}")))?
        };

        // Capture the negotiated protocol version from the initialize result.
        if let Some(ver) = parsed
            .result
            .as_ref()
            .and_then(|r| r.get("protocolVersion"))
            .and_then(|v| v.as_str())
        {
            *self.protocol_version.lock().unwrap() = Some(ver.to_string());
        }

        Ok(parsed)
    }

    async fn notify(&self, note: JsonRpcNotification) -> Result<()> {
        let rb = self.decorate(self.client.post(&self.url).json(&note));
        rb.send()
            .await
            .map_err(|e| GatewayError::Transport(e.to_string()))?;
        Ok(())
    }
}

/// Extract the first JSON-RPC response from an SSE body. Concatenates the
/// `data:` lines of each event and returns the first that parses to a
/// response carrying a `result` or `error`.
fn parse_sse(text: &str) -> Result<JsonRpcResponse> {
    fn try_parse(buf: &str) -> Option<JsonRpcResponse> {
        serde_json::from_str::<JsonRpcResponse>(buf.trim())
            .ok()
            .filter(|r| r.result.is_some() || r.error.is_some())
    }

    let mut event = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            if !event.is_empty() {
                event.push('\n');
            }
            event.push_str(rest.trim_start());
        } else if line.trim().is_empty() {
            if let Some(r) = try_parse(&event) {
                return Ok(r);
            }
            event.clear();
        }
    }
    try_parse(&event)
        .ok_or_else(|| GatewayError::Transport("no JSON-RPC response in SSE stream".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_event_sse() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n";
        let resp = parse_sse(body).expect("should parse");
        assert_eq!(resp.result.unwrap()["ok"], serde_json::json!(true));
    }

    #[test]
    fn parses_multiline_data_sse() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"id\":1,\ndata: \"result\":{\"v\":2}}\n\n";
        let resp = parse_sse(body).expect("should parse");
        assert_eq!(resp.result.unwrap()["v"], serde_json::json!(2));
    }

    #[test]
    fn errors_when_no_response_frame() {
        let body = "event: ping\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"x\"}\n\n";
        assert!(parse_sse(body).is_err());
    }
}
