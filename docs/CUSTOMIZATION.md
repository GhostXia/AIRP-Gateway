# AIRP-Gateway · 可定制点清单（Customization）

> 给**用户**(改配置即可)和**开发者**(写代码扩展)的一站式定制索引。
> 三层定制,从无需写码到深度扩展:
> 1. **配置**(TOML / 环境变量)—— 零代码
> 2. **Cargo feature** —— 开关可选模块
> 3. **代码扩展点**(trait / 组合 API)—— 自建传输/前端/中间件
>
> 真相以源码为准:`src/config.rs`、`src/lib.rs`(公共导出)。

---

## 一、配置（零代码,改 `config.toml` 或环境变量）

`GatewayConfig::load(path)` 分层加载:**默认 → TOML 文件 → 环境变量**(后者覆盖前者)。

### 顶层字段

| 字段 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `bind` | string | `127.0.0.1:8080` | 前端面绑定地址。改 `0.0.0.0:*` = 显式对外暴露 |
| `access_key` | string? | 无 | 前端 bearer 令牌(常数时间校验);不设=开放 |
| `rate_limit.enabled` | bool | `true` | 是否启用 per-IP 限流 |
| `rate_limit.per_second` | u64 | `10` | 稳态速率 |
| `rate_limit.burst` | u32 | `20` | 突发额度 |
| `cors.allow_any` | bool | `true` | 允许任意 origin/method/header |
| `cors.allow_origins` | string[] | `[]` | `allow_any=false` 时的白名单 origin |
| `upstreams` | Upstream[] | `[]` | 上游 MCP 服务列表(见下) |
| `routes` | Route[] | `[]` | 前端路径 → MCP 操作映射(见下) |
| `allowed_commands` | string[] | `[]` | stdio 命令白名单。**空=放行**;非空=stdio `command` 须按全串/文件名匹配 |
| `max_request_bytes` | usize | `1048576` (1 MiB) | 入站请求体上限 |
| `max_response_bytes` | usize | `10485760` (10 MiB) | 上游响应体上限 |
| `upstream_timeout_secs` | u64 | `30` | 上游请求超时(连接+读);`0`=不超时 |
| `block_private_upstream_urls` | bool | `true` | SSRF:拦私网/link-local/保留段(含 169.254.169.254)。**loopback 默认放行** |
| `allow_arbitrary_args` | bool | `false` | `allowed_commands` 非空时,是否允许 stdio 上游带 `args` |

### 上游（`[[upstreams]]`）

```toml
# stdio:网关拉起子进程
[[upstreams]]
name = "backend"
transport = "stdio"
command = "your-mcp-server"
args = ["mcp", "--data-dir", "./data"]   # 可选
cwd = "/path"                            # 可选

# http:连已运行的 MCP 服务
[[upstreams]]
name = "backend-http"
transport = "http"
url = "http://127.0.0.1:3000/mcp/v1"
auth_token = "可选 bearer"
```

### 路由（`[[routes]]`）

```toml
[[routes]]
path = "/v1/do-thing"      # 前端路径
method = "POST"            # 默认 POST
upstream = "backend"       # 指向某 upstream.name
target = { kind = "tool", name = "your_tool" }       # 调 MCP 工具
# 或
target = { kind = "resource", uri = "mcp://..." }    # 读 MCP 资源
# tool 可加 stream = true(待 Stage 2 流式)
```

### 环境变量(最高优先)

| 变量 | 覆盖字段 |
|------|---------|
| `AIRP_GW_BIND` | `bind` |
| `AIRP_GW_ACCESS_KEY` | `access_key`(空=清除) |
| `AIRP_GW_MAX_REQUEST_BYTES` | `max_request_bytes` |
| `AIRP_GW_MAX_RESPONSE_BYTES` | `max_response_bytes` |
| `AIRP_GW_UPSTREAM_TIMEOUT_SECS` | `upstream_timeout_secs` |

> 中间件顺序固定:**CORS → 限流 → 鉴权 → 分发**。

---

## 二、Cargo Features（开关可选模块）

| feature | 默认 | 作用 |
|---------|------|------|
| `agentbus` | **关** | State-Protocol `AgentBus` SSE 适配前端(`/airp/dispatch` + `/airp/stream`)。纯桥使用者默认不编译该项目专属代码 |

```toml
# 纯 MCP 桥(最小)
airp-gateway = { git = "https://github.com/GhostXia/AIRP-Gateway" }
# 需要 State-Protocol 前端
airp-gateway = { git = "...", features = ["agentbus"] }
```

---

## 三、代码扩展点（写码,在共享核心之上扩展）

核心**前端无关 + 传输无关**。下表每行可独立扩展,**均不需改 `bridge/mod.rs`**。

| 想做 | 怎么做 | 入口 |
|------|--------|------|
| **新上游传输**(WebSocket/gRPC/进程内…) | `impl mcp::transport::McpTransport`(`request`/`notify`/可选 `shutdown`),并在 `transport::connect` 加分支 | `src/mcp/transport/mod.rs` |
| **新路由目标类型** | 扩 `config::RouteTarget` + `bridge::Bridge::dispatch` 处理 | `src/config.rs`、`src/bridge/mod.rs` |
| **新鉴权/中间件** | 加 axum 中间件 | `src/server/middleware.rs` |
| **新前端协议**(任意:WS/gRPC/AgentBus/自定义) | `GatewayState::build(cfg)` 拿共享状态,自建前端调 `state.bridge.dispatch(rule, body)`;无需用内置 axum 前端 | `GatewayState` / `Bridge` |
| **编程式装配上游池** | `UpstreamPool::insert(name, client)` 手工放 `McpClient`(测试/动态场景) | `src/mcp/pool.rs` |
| **直接驱动单个上游** | `McpClient::call_tool` / `read_resource` / `list_tools` / `protocol_version` | `src/mcp/client.rs` |

### 自建前端最小骨架

```rust
// 1. 构建共享核心(连上游 + 装配 bridge,不碰内置 HTTP 前端)
let state = airp_gateway::GatewayState::build(cfg).await?;

// 2. 你的前端(任意协议)收到请求后,走同一个 bridge 分发到 MCP
if let Some(rule) = state.bridge.match_route("POST", "/v1/your-path") {
    match state.bridge.dispatch(&rule, body_json).await? {
        airp_gateway::DispatchOutcome::Json(v) => { /* 回你的前端 */ }
        airp_gateway::DispatchOutcome::Stream => { /* 流式(待实现) */ }
    }
}
```

**可运行示例**:[`examples/custom_frontend.rs`](../examples/custom_frontend.rs) —— 纯桥(无 agentbus)、不起内置 HTTP server,直接 `GatewayState::build` + `bridge.dispatch` 驱动自带 mock 上游:
```text
cargo build --example mock_mcp_stdio
AIRP_MCP_BIN=<built mock path> cargo run --example custom_frontend
# → MCP result: {"content":[{"text":"ok",...}],"isError":false,"structuredContent":{"hello":"world"}}
```

### 公共构件(已 re-export,直接 `use airp_gateway::*`)

`GatewayConfig` · `RouteRule` · `RouteTarget` · `TransportConfig` · `UpstreamConfig`
· `GatewayError` · `Result` · `Gateway` · `GatewayState`
· `Bridge` · `DispatchOutcome` · `McpClient` · `UpstreamPool` · `McpTransport` · `connect_transport`

> 这些就是「移植契约」的一部分(见 DESIGN §9):换语言时保持这些形状即可平移。

---

## 四、AgentBus 适配配置（仅 `features = ["agentbus"]`）

`agentbus::AdapterConfig` —— State-Protocol 专属映射,**adapter-local,不进核心 config**:

| 字段 | 说明 |
|------|------|
| `default_scope` | intent 无 `source` 且无 fallback 时的默认 scope;`None`=拒绝 |
| `fallback_scopes` | 每个 intent 名的默认 scope 表 |
| `route_prefix` | intent 名 → 路由路径前缀,默认 `/v1/`(`chat.send` → `/v1/chat.send`) |
| `initial_blueprint` | `hello` 时下推的初始布局 |
| `initial_manifests` | `hello` 时下推的 widget manifest |
| `initial_state` | `hello` 时下推的各 scope 初始状态 `{scope: value}` |

连接关联:SSE 流经 `?conn=<id>` 提供 id,或读首个 `airp-ready` 事件得 id;后续 `POST /airp/dispatch` 用 `x-airp-conn` 头回传。详见 [`AGENTBUS-ADAPTER.md`](AGENTBUS-ADAPTER.md)。

---

## 不可定制（设计铁律,见 DESIGN §1/§2）

- 核心 `bridge` 不懂业务、不做推理/拼 prompt —— 领域语义只以字符串出现在 `routes`。
- 不为单一第三方写进核心的适配器(应作 feature 或外部 crate)。
- 这些是「通用、不捆绑」的保证,扩展时请绕开而非改动它们。
