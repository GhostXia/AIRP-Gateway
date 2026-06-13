//! Layered gateway configuration: `default -> config file (TOML) -> env`.
//!
//! Mirrors the merge philosophy of AIRP-Core's `config.rs` but is fully
//! domain-agnostic: the gateway knows about *bind address, auth, rate limits,
//! upstream MCP servers, and route mappings* — nothing about characters,
//! presets, or any roleplay concept.

use serde::Deserialize;
use std::path::Path;

use crate::error::{GatewayError, Result};

/// Top-level gateway configuration.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    /// Socket address the frontend-facing server binds to, e.g. `127.0.0.1:8080`.
    pub bind: String,
    /// Optional bearer token required on `/` API routes. `None` = open.
    pub access_key: Option<String>,
    pub rate_limit: RateLimitConfig,
    pub cors: CorsConfig,
    /// MCP servers this gateway can forward to, keyed by `name`.
    pub upstreams: Vec<UpstreamConfig>,
    /// Declarative frontend-path -> MCP-operation mappings.
    pub routes: Vec<RouteRule>,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8080".to_string(),
            access_key: None,
            rate_limit: RateLimitConfig::default(),
            cors: CorsConfig::default(),
            upstreams: Vec::new(),
            routes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct RateLimitConfig {
    pub enabled: bool,
    /// Sustained requests per second per client key (IP).
    pub per_second: u64,
    /// Burst allowance above the sustained rate.
    pub burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            per_second: 10,
            burst: 20,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct CorsConfig {
    /// Allow any origin/method/header (permissive). Convenient for local dev.
    pub allow_any: bool,
    /// Explicit origins when `allow_any` is false.
    pub allow_origins: Vec<String>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allow_any: true,
            allow_origins: Vec::new(),
        }
    }
}

/// One upstream MCP server.
#[derive(Clone, Debug, Deserialize)]
pub struct UpstreamConfig {
    /// Unique name referenced by [`RouteRule::upstream`].
    pub name: String,
    #[serde(flatten)]
    pub transport: TransportConfig,
}

/// How to reach an upstream MCP server. Both stdio and HTTP are first-class.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "transport", rename_all = "lowercase")]
pub enum TransportConfig {
    /// Launch the MCP server as a child process; talk JSON-RPC over stdin/stdout.
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
    },
    /// Connect to a running MCP server over streamable HTTP.
    Http {
        url: String,
        #[serde(default)]
        auth_token: Option<String>,
    },
}

/// A declarative mapping from a frontend request to an MCP operation.
#[derive(Clone, Debug, Deserialize)]
pub struct RouteRule {
    /// Frontend path, e.g. `/v1/chat/completions`.
    pub path: String,
    #[serde(default = "default_method")]
    pub method: String,
    /// Name of the [`UpstreamConfig`] to dispatch to.
    pub upstream: String,
    pub target: RouteTarget,
}

fn default_method() -> String {
    "POST".to_string()
}

/// What MCP primitive a route invokes.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RouteTarget {
    /// Call an MCP tool. The request body is passed through as the tool args.
    Tool {
        name: String,
        /// Whether the response should be streamed back to the frontend as SSE.
        #[serde(default)]
        stream: bool,
    },
    /// Read an MCP resource by URI.
    Resource { uri: String },
}

impl GatewayConfig {
    /// Build config by layering: defaults, then optional TOML file, then env.
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let mut cfg = match path {
            Some(p) if p.exists() => {
                let text = std::fs::read_to_string(p)?;
                toml::from_str(&text).map_err(|e| GatewayError::Config(e.to_string()))?
            }
            _ => GatewayConfig::default(),
        };
        cfg.apply_env();
        Ok(cfg)
    }

    /// Apply `AIRP_GW_*` environment overrides (top-level scalars only).
    fn apply_env(&mut self) {
        if let Ok(v) = std::env::var("AIRP_GW_BIND") {
            if !v.is_empty() {
                self.bind = v;
            }
        }
        if let Ok(v) = std::env::var("AIRP_GW_ACCESS_KEY") {
            self.access_key = if v.is_empty() { None } else { Some(v) };
        }
    }
}
