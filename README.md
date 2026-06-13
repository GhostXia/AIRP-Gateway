# AIRP-Gateway

**AIRP-Gateway** = AI Roleplay Gateway。通用、高性能的**纯协议桥**：把前端的 HTTP/SSE 请求，鉴权 + 限流后翻译成 MCP（JSON-RPC）调用，转发给上游 MCP 服务。

> ## 🧭 核心理念：通用，不捆绑
>
> **本项目一贯以「通用」为理念核心，不捆绑任何项目使用。任何第三方项目均可便捷地用本项目做任何事情。**
>
> AIRP-Gateway 诞生于 AIRP 生态，但**不属于、不依赖、不绑定**其中任何单一项目。它是一个独立的通用协议桥：上游是什么 MCP 服务、下游是什么前端，全部由配置决定。与 AIRP-Core / AIRP-MCP-Server / AIRP-State-Protocol 的「联动」一律是**可选适配**，而非前提。

[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#许可)

> 相关生态（均为可选）：[AIRP-Core](https://github.com/GhostXia/AIRP-Core) · [AIRP-MCP-Server](https://github.com/GhostXia/AIRP-MCP-Server)（上游数据底座）· [AIRP-State-Protocol](https://github.com/GhostXia/AIRP-State-Protocol)（前端契约，见下方「生态联动」）。

```text
前端 (UI)  ──►  AIRP-Gateway  ──►  AIRP-MCP-Server  ──►  Agent / 推理后端
                (本项目·纯协议桥)     (数据底座·MCP)
```

---

## 设计戒律（Iron Laws）

桥之所以通用，因为它**不懂业务**。任何改动不得破坏：

1. **纯协议桥** — 不做推理、不拼 prompt、不懂「角色 / 预设 / 世界书」。领域语义只以字符串出现在配置里。
2. **库优先** — 核心是 crate，无独立 exe。宿主自管进程启动，调 `Gateway::build(cfg).run()`。
3. **传输无关** — 上游 stdio / HTTP 对 bridge 透明，只经 `McpTransport` trait。新传输零成本接入。
4. **轻依赖、可移植** — 手写 JSON-RPC 契约，分层薄。未来换语言可照搬契约。

业务逻辑归 MCP 服务（AIRP-MCP-Server 已持有 38 个 MCP 工具）。Gateway 只负责鉴权、限流、转发。

---

## 快速开始（作为库嵌入）

`Cargo.toml`：

```toml
[dependencies]
airp-gateway = { git = "https://github.com/GhostXia/AIRP-Gateway" }
```

宿主代码：

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    airp_gateway::telemetry::init();

    // 分层加载：default -> TOML -> env(AIRP_GW_*)
    let cfg = airp_gateway::GatewayConfig::load(Some("config.toml".as_ref()))?;

    // 连接上游、装配 bridge、起服务
    airp_gateway::Gateway::build(cfg).await?.run().await?;
    Ok(())
}
```

`config.toml`：

```toml
bind = "127.0.0.1:8080"
# access_key = "前端访问令牌（不设则开放）"

[rate_limit]
enabled = true
per_second = 10
burst = 20

[cors]
allow_any = true

# 上游 MCP 服务（stdio 现可用）
[[upstreams]]
name = "airp"
transport = "stdio"
command = "airp-mcp"
args = ["mcp", "--data-dir", "./data"]

# 上游 MCP 服务（HTTP，待 AIRP-MCP-Server 补完 /mcp/v1，见下）
# [[upstreams]]
# name = "airp-http"
# transport = "http"
# url = "http://127.0.0.1:3000/mcp/v1"
# auth_token = "AIRP_HTTP_TOKEN 的值"

# 声明式路由：前端路径 -> MCP 工具/资源
[[routes]]
path = "/v1/characters"
method = "GET"
upstream = "airp"
target = { kind = "tool", name = "list_characters" }
```

---

## 配置

| 键 | 说明 |
|----|------|
| `bind` | 前端面服务绑定地址，默认 `127.0.0.1:8080` |
| `access_key` | 前端 bearer 令牌（常数时间校验）；不设 = 开放 |
| `rate_limit` | per-IP 令牌桶：`enabled` / `per_second` / `burst` |
| `cors` | `allow_any`，或 `allow_origins = [...]` |
| `upstreams[]` | 上游 MCP 服务：`name` + `transport`(`stdio`\|`http`) |
| `routes[]` | `path` + `method` + `upstream` + `target`(`tool`\|`resource`) |

环境变量覆盖（最高优先）：`AIRP_GW_BIND`、`AIRP_GW_ACCESS_KEY`。

中间件顺序：**CORS → 限流 → 鉴权 → 分发**。

---

## 架构

| 模块 | 职责 |
|------|------|
| `config` | 分层配置 default → TOML → env |
| `server` | axum 路由、常数时间 bearer 鉴权、CORS、governor 限流 |
| `bridge` | 请求 → MCP 操作 → 响应（领域无关） |
| `mcp::types` | JSON-RPC 2.0 / MCP 线类型（手写） |
| `mcp::client` | 单上游：`initialize` 握手、`call_tool`、`read_resource` |
| `mcp::pool` | name → client 注册表 |
| `mcp::transport` | `McpTransport` trait + `stdio` / `http` 两实现 |

数据流：

```text
前端请求 → CORS → 限流 → 鉴权 → dispatch 兜底
  → 匹配 RouteRule → 取上游 client
  → McpClient（首次惰性 initialize）→ McpTransport
  → 上游 MCP 服务 → result → 回前端
```

完整设计与开发追踪见 [`docs/DESIGN.md`](docs/DESIGN.md)。

---

## 上游对接状态

| 传输 | 状态 |
|------|------|
| **stdio** | ✅ 可用。Gateway 拉起 `airp-mcp mcp --data-dir` 子进程，行分隔 JSON-RPC，对接 AIRP-MCP-Server 真实 MCP，零改动 |
| **HTTP** | ⛔ 阻塞。AIRP-MCP-Server 的 `/mcp/v1` 当前为未完成的桩（POST 不转发、无 session）。需 MCP-Server 侧补完——见 [`docs/MCP-SERVER-REQUIREMENTS.md`](docs/MCP-SERVER-REQUIREMENTS.md) |

---

## 生态联动（可选）

> 重申：以下全部是**可选适配**。不接入任何一个，Gateway 也能独立服务任意前端与任意 MCP 服务。

### AIRP-State-Protocol —— 前端契约
[AIRP-State-Protocol](https://github.com/GhostXia/AIRP-State-Protocol) 定义了「Agent 产出声明式数据、UI 只负责渲染」的前端↔网关契约：`Envelope` / `Blueprint` / `State + Patch`(RFC 6902) / 可扩展 Widget 注册表，以及进程级的 **`AgentBus` trait**——并显式把 **AIRP-Gateway 列为该 trait 的实现方之一**。它本身**传输无关**（Tauri IPC / HTTP / SSE / WebSocket 皆可）。

完整链路设想：

```text
AIRP-State-Protocol UI  ──AgentBus(SSE/WS/HTTP)──►  AIRP-Gateway  ──MCP──►  AIRP-MCP-Server  ──►  Agent
        (前端·声明式渲染)                              (本项目·桥)             (数据底座)
```

联动方式（计划中，未实现）：Gateway 提供一个**可选**的 `AgentBus` 适配层，把前端的 State-Protocol 消息映射到本项目既有的 `RouteRule → MCP` 分发，再把结果按 `Blueprint`/`Patch` 形态回传。核心桥保持纯净——适配层是插件，不进 `bridge`。

> 它是「前端那一侧」的契约，与上游 MCP 互不冲突：State-Protocol 管 `前端↔Gateway`，MCP 管 `Gateway↔后端`。

### 接入任意第三方
不必是 AIRP 项目。任何能发 HTTP/SSE 的前端 + 任何讲 MCP 的服务，配一份 `config.toml` 即可对接。新前端契约 = 加可选适配层；新上游传输 = `impl McpTransport`。

---

## 开发

构建工具链固定为 GNU（规避 MSVC `link.exe` 依赖，与 AIRP-Core 一致）：

```powershell
# 本机 D: 盘构建脚本执行受系统策略限制，需将 target 目录重定向到 C:
$env:CARGO_TARGET_DIR = "C:\Users\<you>\airp-gw-target"
cargo +stable-x86_64-pc-windows-gnu check --manifest-path .\Cargo.toml
```

`.cargo/config.toml` 已锁定 `x86_64-pc-windows-gnu` target。

---

## 许可

双许可 **Apache-2.0 OR MIT**，任选其一（"at your option"）。
向本项目贡献即视为以相同双许可授权。
