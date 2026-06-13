//! End-to-end integration tests for the gateway core, using a mock MCP
//! transport at the `McpTransport` seam — no real subprocess / network.
//!
//! Exercises the full inbound path: axum router → auth middleware → dispatch
//! handler → Bridge → McpClient (initialize handshake + tools/call) → mock
//! transport → JSON response.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt; // for `oneshot`

use airp_gateway::mcp::types::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
use airp_gateway::{
    Bridge, Gateway, GatewayConfig, GatewayState, McpClient, McpTransport, RouteRule, RouteTarget,
    UpstreamPool,
};

/// A mock MCP server: answers `initialize` and echoes `tools/call` arguments.
struct MockTransport;

#[async_trait]
impl McpTransport for MockTransport {
    async fn request(&self, req: JsonRpcRequest) -> airp_gateway::Result<JsonRpcResponse> {
        let result = match req.method.as_str() {
            "initialize" => json!({
                "protocolVersion": "2025-06-18",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "mock", "version": "0.0.0" }
            }),
            "tools/call" => {
                let args = req
                    .params
                    .as_ref()
                    .and_then(|p| p.get("arguments"))
                    .cloned()
                    .unwrap_or(Value::Null);
                json!({ "ok": true, "echo": args })
            }
            other => json!({ "unhandled": other }),
        };
        Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: Some(result),
            error: None,
        })
    }

    async fn notify(&self, _note: JsonRpcNotification) -> airp_gateway::Result<()> {
        Ok(())
    }
}

/// Build a gateway whose single upstream is backed by [`MockTransport`].
fn mock_gateway(access_key: Option<&str>) -> Gateway {
    let client = Arc::new(McpClient::new("mock", Arc::new(MockTransport)));
    let mut pool = UpstreamPool::default();
    pool.insert("mock", client);
    let pool = Arc::new(pool);

    let routes = vec![RouteRule {
        path: "/v1/echo".to_string(),
        method: "POST".to_string(),
        upstream: "mock".to_string(),
        target: RouteTarget::Tool {
            name: "list_characters".to_string(),
            stream: false,
        },
    }];
    let bridge = Bridge::new(pool.clone(), routes.clone());

    let config = GatewayConfig {
        access_key: access_key.map(|s| s.to_string()),
        // governor needs ConnectInfo; skip rate limiting under `oneshot`.
        rate_limit: airp_gateway::config::RateLimitConfig {
            enabled: false,
            ..Default::default()
        },
        routes,
        ..Default::default()
    };

    Gateway::from_state(Arc::new(GatewayState {
        config,
        bridge,
        pool,
    }))
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn dispatch_tool_end_to_end() {
    let app = mock_gateway(None).router();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/echo")
        .header("content-type", "application/json")
        .body(Body::from(json!({"q": 42}).to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Value = serde_json::from_str(&body_string(resp).await).unwrap();
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["echo"]["q"], json!(42)); // request body forwarded as tool args
}

#[tokio::test]
async fn unknown_route_is_404() {
    let app = mock_gateway(None).router();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/nope")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn health_is_public() {
    let app = mock_gateway(Some("secret")).router();
    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_string(resp).await, "ok");
}

#[tokio::test]
async fn auth_rejects_missing_token() {
    let app = mock_gateway(Some("secret")).router();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/echo")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_accepts_valid_token() {
    let app = mock_gateway(Some("secret")).router();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/echo")
        .header("authorization", "Bearer secret")
        .header("content-type", "application/json")
        .body(Body::from(json!({"q": 1}).to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
