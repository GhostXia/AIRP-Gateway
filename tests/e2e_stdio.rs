//! Real cross-process end-to-end test over stdio.
//!
//! Launches an actual `airp-mcp` server as a child process and dispatches a
//! tool call through the full gateway stack. Verifies the parts a mock cannot:
//! real subprocess spawn, newline-delimited JSON-RPC framing, the live MCP
//! `initialize` handshake, and a real `tools/call` result.
//!
//! Skipped unless `AIRP_MCP_BIN` points at an `airp-mcp` binary (CI sets it to
//! a freshly built one). Local `cargo test` without it is a no-op pass.

use airp_gateway::config::{RateLimitConfig, TransportConfig, UpstreamConfig};
use airp_gateway::{Gateway, GatewayConfig, RouteRule, RouteTarget};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn list_characters_via_real_stdio_server() {
    let Ok(bin) = std::env::var("AIRP_MCP_BIN") else {
        eprintln!("AIRP_MCP_BIN not set; skipping real stdio e2e");
        return;
    };

    // Fresh empty data dir → list_characters should report "none yet".
    let data_dir =
        std::env::temp_dir().join(format!("airp-gw-e2e-{}-{}", std::process::id(), "stdio"));
    std::fs::create_dir_all(&data_dir).unwrap();

    let config = GatewayConfig {
        rate_limit: RateLimitConfig {
            enabled: false,
            ..Default::default()
        },
        upstreams: vec![UpstreamConfig {
            name: "airp".into(),
            transport: TransportConfig::Stdio {
                command: bin,
                args: vec![
                    "mcp".into(),
                    "--data-dir".into(),
                    data_dir.to_string_lossy().into_owned(),
                ],
                cwd: None,
            },
        }],
        routes: vec![RouteRule {
            path: "/v1/characters".into(),
            method: "GET".into(),
            upstream: "airp".into(),
            target: RouteTarget::Tool {
                name: "list_characters".into(),
                stream: false,
            },
        }],
        ..Default::default()
    };

    let gateway = Gateway::build(config).await.expect("build gateway");
    let app = gateway.router();

    let req = Request::builder()
        .method("GET")
        .uri("/v1/characters")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        status,
        StatusCode::OK,
        "dispatch should succeed; body={body}"
    );

    // MCP CallToolResult: { content: [{ type: "text", text: ... }], isError: false }
    assert_eq!(
        body["isError"],
        Value::Bool(false),
        "tool should not error: {body}"
    );
    let text = body["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("No characters imported yet"),
        "unexpected tool result: {body}"
    );

    let _ = std::fs::remove_dir_all(&data_dir);
}
