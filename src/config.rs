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
    /// Allowlist of permitted stdio `command`s (defense-in-depth against a
    /// config that spawns arbitrary programs). **Empty = allow any** (config is
    /// operator-trusted). Non-empty = a stdio upstream's `command` must match an
    /// entry by full string *or* file name, else [`GatewayConfig::validate`]
    /// fails. Has no effect on HTTP upstreams.
    pub allowed_commands: Vec<String>,
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
            allowed_commands: Vec::new(),
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

/// True if `command` matches an allowlist entry by full string or file name
/// (so `airp-mcp` matches `/usr/bin/airp-mcp`).
fn command_allowed(command: &str, allow: &[String]) -> bool {
    let base = std::path::Path::new(command)
        .file_name()
        .and_then(|s| s.to_str());
    allow
        .iter()
        .any(|a| a == command || Some(a.as_str()) == base)
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

    /// Validate security-relevant invariants. Call before building transports.
    ///
    /// Currently: if [`allowed_commands`](Self::allowed_commands) is non-empty,
    /// every stdio upstream's `command` must be allowed.
    pub fn validate(&self) -> Result<()> {
        if self.allowed_commands.is_empty() {
            return Ok(());
        }
        for up in &self.upstreams {
            if let TransportConfig::Stdio { command, .. } = &up.transport {
                if !command_allowed(command, &self.allowed_commands) {
                    return Err(GatewayError::Config(format!(
                        "stdio command `{command}` (upstream `{}`) is not in allowed_commands",
                        up.name
                    )));
                }
            }
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio_up(name: &str, command: &str) -> UpstreamConfig {
        UpstreamConfig {
            name: name.into(),
            transport: TransportConfig::Stdio {
                command: command.into(),
                args: vec![],
                cwd: None,
            },
        }
    }

    fn cfg_with(upstreams: Vec<UpstreamConfig>, allowed: Vec<&str>) -> GatewayConfig {
        GatewayConfig {
            upstreams,
            allowed_commands: allowed.into_iter().map(String::from).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn empty_allowlist_permits_any_command() {
        let cfg = cfg_with(vec![stdio_up("a", "/bin/bash")], vec![]);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn allowlist_rejects_unlisted_command() {
        let cfg = cfg_with(vec![stdio_up("a", "/bin/bash")], vec!["airp-mcp"]);
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn allowlist_matches_by_basename() {
        let cfg = cfg_with(
            vec![stdio_up("a", "/usr/local/bin/airp-mcp")],
            vec!["airp-mcp"],
        );
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn allowlist_ignores_http_upstreams() {
        let http = UpstreamConfig {
            name: "h".into(),
            transport: TransportConfig::Http {
                url: "http://127.0.0.1:3000/mcp/v1".into(),
                auth_token: None,
            },
        };
        let cfg = cfg_with(vec![http], vec!["airp-mcp"]);
        assert!(cfg.validate().is_ok());
    }
}
