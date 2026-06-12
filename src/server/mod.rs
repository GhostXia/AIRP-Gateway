//! Frontend-facing server: builds the axum app, wires middleware, and runs it.

pub mod handlers;
pub mod middleware;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::get;
use axum::Router;

use crate::bridge::Bridge;
use crate::config::GatewayConfig;
use crate::error::{GatewayError, Result};
use crate::mcp::UpstreamPool;

/// Shared application state handed to every handler.
pub struct GatewayState {
    pub config: GatewayConfig,
    pub bridge: Bridge,
    pub pool: Arc<UpstreamPool>,
}

impl GatewayState {
    pub fn bridge_upstreams(&self) -> Vec<String> {
        self.pool.names().cloned().collect()
    }
}

/// A fully-built gateway, ready to serve.
pub struct Gateway {
    state: Arc<GatewayState>,
}

impl Gateway {
    /// Build the gateway: connect upstream transports and assemble the bridge.
    pub async fn build(config: GatewayConfig) -> Result<Self> {
        let pool = Arc::new(UpstreamPool::from_config(&config.upstreams).await?);
        let bridge = Bridge::new(pool.clone(), config.routes.clone());
        let state = Arc::new(GatewayState { config, bridge, pool });
        Ok(Self { state })
    }

    /// Construct the axum router with middleware applied.
    pub fn router(&self) -> Router {
        let state = self.state.clone();

        // Authenticated API surface (everything except the public routes below).
        let api = Router::new()
            .fallback(handlers::dispatch)
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                middleware::auth,
            ));

        // Public, unauthenticated routes.
        let public = Router::new()
            .route("/health", get(handlers::health))
            .route("/version", get(handlers::version));

        let mut app = public
            .merge(api)
            .layer(middleware::cors_layer(&self.state.config.cors));

        if self.state.config.rate_limit.enabled {
            // Per-IP token bucket shared across all routes.
            let conf = Arc::new(
                tower_governor::governor::GovernorConfigBuilder::default()
                    .per_second(self.state.config.rate_limit.per_second)
                    .burst_size(self.state.config.rate_limit.burst)
                    .finish()
                    .expect("valid governor config"),
            );
            app = app.layer(tower_governor::GovernorLayer { config: conf });
        }

        app.with_state(state)
    }

    /// Bind and serve until the process is terminated.
    pub async fn run(self) -> Result<()> {
        let addr: SocketAddr = self
            .state
            .config
            .bind
            .parse()
            .map_err(|e| GatewayError::Config(format!("invalid bind address: {e}")))?;

        let app = self.router();
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("airp-gateway listening on http://{addr}");

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await?;
        Ok(())
    }
}
