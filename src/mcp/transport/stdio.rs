//! stdio MCP transport.
//!
//! Launches the MCP server as a child process and exchanges newline-delimited
//! JSON-RPC messages over its stdin/stdout. A background reader task matches
//! responses to in-flight requests by `id`.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{oneshot, Mutex};

use crate::error::{GatewayError, Result};
use crate::mcp::transport::McpTransport;
use crate::mcp::types::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>;

pub struct StdioTransport {
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    // Keep the child alive for the lifetime of the transport.
    _child: Child,
}

impl StdioTransport {
    pub async fn connect(command: &str, args: &[String], cwd: Option<&str>) -> Result<Self> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| GatewayError::Transport(format!("spawn `{command}`: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| GatewayError::Transport("child stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GatewayError::Transport("child stdout unavailable".into()))?;

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        // Reader task: route each response line to its waiting caller.
        let pending_reader = pending.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&line) {
                    let key = id_key(&resp.id);
                    if let Some(tx) = pending_reader.lock().await.remove(&key) {
                        let _ = tx.send(resp);
                    }
                }
                // Lines that aren't responses (server-initiated notifications)
                // are ignored for now. TODO: surface them for streaming.
            }
        });

        Ok(Self {
            stdin: Mutex::new(stdin),
            pending,
            _child: child,
        })
    }

    async fn write_line(&self, payload: &str) -> Result<()> {
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| GatewayError::Transport(e.to_string()))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| GatewayError::Transport(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| GatewayError::Transport(e.to_string()))?;
        Ok(())
    }
}

fn id_key(id: &serde_json::Value) -> String {
    match id {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn request(&self, req: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let key = id_key(&req.id);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(key.clone(), tx);

        let line = serde_json::to_string(&req)?;
        if let Err(e) = self.write_line(&line).await {
            self.pending.lock().await.remove(&key);
            return Err(e);
        }

        rx.await
            .map_err(|_| GatewayError::Transport("upstream closed before responding".into()))
    }

    async fn notify(&self, note: JsonRpcNotification) -> Result<()> {
        let line = serde_json::to_string(&note)?;
        self.write_line(&line).await
    }
}
