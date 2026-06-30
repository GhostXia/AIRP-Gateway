//! Axum handlers for the State-Protocol SSE adapter surface.
//!
//! Routes:
//! - `POST /airp/dispatch` — receive an upstream `Envelope` from the UI
//! - `GET  /airp/stream`   — open an SSE connection for downstream `Envelope`s
//!
//! Both are mounted by [`super::adapter_router`] onto the host's axum app. They
//! depend only on [`airp_gateway::GatewayState`] (the shared bridge + pool) and
//! the adapter's own [`super::bus::Bus`] + [`super::config::AdapterConfig`].

use std::sync::Arc;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::stream::unfold;
use serde_json::Value;

use crate::bridge::DispatchOutcome;
use crate::server::GatewayState;

use super::bus::Bus;
use super::config::AdapterConfig;
use super::envelope::*;

/// Shared state injected into every handler via axum's `State`.
//
// Not `Debug`: `GatewayState` contains a `Bridge` whose `UpstreamPool` holds
// transport handles that are not `Debug`. We don't need debug printing here.
#[derive(Clone)]
pub struct AdapterState {
    pub gateway: Arc<GatewayState>,
    pub bus: Bus,
    pub config: AdapterConfig,
}

// ─── POST /airp/dispatch ────────────────────────────────────────────────

/// Receive an upstream envelope from the UI and process it.
pub async fn dispatch(
    State(state): State<Arc<AdapterState>>,
    // The SSE connection id, sent as a header so subscribe intents can be
    // attributed to the right stream. The UI sets this when it opens the
    // SSE connection and echoes it on every POST.
    headers: HeaderMap,
    Json(envelope): Json<Envelope>,
) -> Response {
    // Validate envelope version.
    let envelope = match envelope.validate() {
        Ok(e) => e,
        Err(e) => {
            return Json(serde_json::json!({
                "ok": false,
                "error": e.to_string()
            }))
            .into_response();
        }
    };

    // Extract the SSE connection id from a header.
    let conn_id: Option<u64> = headers
        .get("x-airp-conn")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());

    // Capture the envelope id before `body` is moved, so we can correlate
    // the downstream reply with the upstream request.
    let env_id = envelope.id.clone();
    // `body` is matched out of `envelope`; from here on we use `env_id`
    // rather than `envelope.id` (which is still valid — only `body` moved).
    match envelope.body {
        Body::Hello {
            client,
            version,
            accept,
        } => {
            tracing::info!(client, version, "hello from client; accept={:?}", accept);
            // Push initial state down the bus.
            send_initial_state(&state).await;
            // Ack the hello.
            let ack = Envelope::gateway_ref(
                env_id.clone(),
                Body::Ack {
                    ref_: env_id,
                    ok: true,
                },
            );
            Json(serde_json::json!({"ok": true, "ack": ack})).into_response()
        }

        Body::Intent {
            name,
            params,
            source,
        } => {
            tracing::info!(intent = %name, source = ?source, "dispatch intent");
            handle_intent(&state, &env_id, &name, params, source.as_deref()).await
        }

        Body::Subscribe { scopes } => {
            if let Some(cid) = conn_id {
                state.bus.set_scopes(cid, scopes.clone()).await;
                tracing::debug!(conn_id = cid, ?scopes, "subscribed");
            } else {
                tracing::warn!("subscribe intent without x-airp-conn header; ignored");
            }
            Json(serde_json::json!({"ok": true})).into_response()
        }

        Body::Ack { ref_, ok } => {
            tracing::debug!(ref = %ref_, ok, "ack received");
            Json(serde_json::json!({"ok": true})).into_response()
        }

        Body::Unknown => {
            tracing::warn!(id = %envelope.id, "unknown envelope kind; dropping");
            Json(serde_json::json!({"ok": false, "error": "unknown kind"})).into_response()
        }

        // Downstream kinds should never arrive upstream; log and reject.
        _ => {
            tracing::warn!(id = %env_id, "downstream-only envelope received upstream");
            Json(serde_json::json!({"ok": false, "error": "downstream-only kind"})).into_response()
        }
    }
}

/// Process an `intent` envelope: match route → dispatch → wrap result → broadcast.
async fn handle_intent(
    state: &Arc<AdapterState>,
    env_id: &str,
    name: &str,
    params: Value,
    source: Option<&str>,
) -> Response {
    // 1. Resolve scope.
    let scope = match state.config.scope_for(name, source) {
        Some(s) => s,
        None => {
            let err_env = Envelope::gateway_ref(
                env_id,
                Body::Error {
                    ref_: Some(env_id.to_string()),
                    code: "no_scope".into(),
                    message: format!(
                        "intent `{name}` has no source and no scope fallback; \
                         set `source` on the intent or configure fallback_scopes"
                    ),
                },
            );
            state.bus.send(err_env).await;
            return Json(serde_json::json!({"ok": false, "error": "no_scope"})).into_response();
        }
    };

    // 2. Map intent name to route path and match.
    let route_path = state.config.route_path(name);
    let rule = match state.gateway.bridge.match_route("POST", &route_path) {
        Some(r) => r.clone(),
        None => {
            let err_env = Envelope::gateway_ref(
                env_id,
                Body::Error {
                    ref_: Some(env_id.to_string()),
                    code: "no_route".into(),
                    message: format!("no route matched POST {route_path}"),
                },
            );
            state.bus.send(err_env).await;
            return Json(serde_json::json!({"ok": false, "error": "no_route"})).into_response();
        }
    };

    // 3. Dispatch through the bridge (→ MCP call_tool / read_resource).
    let outcome = match state.gateway.bridge.dispatch(&rule, params).await {
        Ok(o) => o,
        Err(e) => {
            let err_env = Envelope::gateway_ref(
                env_id,
                Body::Error {
                    ref_: Some(env_id.to_string()),
                    code: "dispatch_error".into(),
                    message: e.to_string(),
                },
            );
            state.bus.send(err_env).await;
            return Json(serde_json::json!({"ok": false, "error": "dispatch_error"}))
                .into_response();
        }
    };

    // 4. Wrap the MCP result as a state envelope and broadcast.
    let mcp_value = match outcome {
        DispatchOutcome::Json(v) => v,
        DispatchOutcome::Stream => {
            let err_env = Envelope::gateway_ref(
                env_id,
                Body::Error {
                    ref_: Some(env_id.to_string()),
                    code: "stream_unsupported".into(),
                    message: "streaming dispatch not yet supported by the adapter".into(),
                },
            );
            state.bus.send(err_env).await;
            return Json(serde_json::json!({"ok": false, "error": "stream_unsupported"}))
                .into_response();
        }
    };

    // Extract structuredContent if present (MCP spec), otherwise the whole result.
    let state_value = mcp_value
        .get("structuredContent")
        .cloned()
        .unwrap_or(mcp_value);

    let state_env = Envelope::gateway_ref(
        env_id,
        Body::State {
            scope: scope.clone(),
            op: StateOp::Set,
            patch: vec![PatchOp::replace_root(state_value)],
        },
    );
    state.bus.send(state_env).await;

    // Ack the intent.
    Json(serde_json::json!({"ok": true})).into_response()
}

/// Push the configured initial state (blueprint + manifests + per-scope state)
/// onto the broadcast bus. Called on `hello`.
async fn send_initial_state(state: &Arc<AdapterState>) {
    if let Some(ref bp) = state.config.initial_blueprint {
        let env = Envelope::gateway(Body::Blueprint {
            op: BlueprintOp::Set,
            blueprint: bp.clone(),
        });
        state.bus.send(env).await;
    }

    if !state.config.initial_manifests.is_empty() {
        let env = Envelope::gateway(Body::Manifest {
            op: ManifestOp::Set,
            manifests: state.config.initial_manifests.clone(),
        });
        state.bus.send(env).await;
    }

    for (scope, value) in &state.config.initial_state {
        let env = Envelope::gateway(Body::State {
            scope: scope.clone(),
            op: StateOp::Set,
            patch: vec![PatchOp::replace_root(value.clone())],
        });
        state.bus.send(env).await;
    }
}

// ─── GET /airp/stream ───────────────────────────────────────────────────

/// Open an SSE stream for downstream envelopes.
///
/// The stream stays open until the client disconnects or the server shuts
/// down. Each `data:` line is a JSON `Envelope`. When the broadcast channel
/// closes (or the SSE response is dropped), the connection is removed from
/// the subscription map.
pub async fn stream(State(state): State<Arc<AdapterState>>) -> Response {
    let (conn_id, rx) = state.bus.subscribe();
    tracing::info!(conn_id, "SSE stream opened");

    let bus = state.bus.clone();

    // Poll the broadcast receiver with `unfold`. Each iteration yields one
    // SSE event or skips (lagged) envelopes. On `Closed`, we disconnect this
    // connection from the subscription map and end the stream.
    let event_stream = unfold(rx, move |mut rx| {
        let bus = bus.clone();
        async move {
            loop {
                match rx.recv().await {
                    Ok(envelope) => {
                        // Determine scope of this envelope.
                        let scope: Option<&str> = match &envelope.body {
                            Body::State { scope, .. } => Some(scope),
                            _ => None,
                        };

                        // Check if this connection wants this scope.
                        if !bus.wants_scope(conn_id, scope).await {
                            continue;
                        }

                        // Serialize envelope as SSE data line.
                        match serde_json::to_string(&envelope) {
                            Ok(json) => {
                                let event = Event::default().data(json);
                                return Some((Ok::<_, std::convert::Infallible>(event), rx));
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "failed to serialize envelope for SSE");
                                continue;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(conn_id, lagged = n, "SSE consumer lagged; skipping");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        // Channel closed: remove this connection from the
                        // subscription map and end the stream.
                        bus.disconnect(conn_id).await;
                        tracing::info!(conn_id, "SSE stream closed");
                        return None;
                    }
                }
            }
        }
    });

    Sse::new(event_stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
