# AIRP-Gateway → AIRP-MCP-Server 对接需求

> 本文件是 AIRP-Gateway 对上游 **AIRP-MCP-Server** 的外部接口需求，用于追踪对方完成情况。
> 写成独立可交付（对方无本项目上下文）。配套追踪见 `docs/DESIGN.md` §6 R6 / Stage 3。
> 状态：⬜待 MCP-Server 实现 ｜ 建档 2026-06-12

---

## 背景

AIRP-Gateway 是纯协议桥，作为 **MCP 客户端**连接 AIRP-MCP-Server。
链路：`前端 → AIRP-Gateway → AIRP-MCP-Server → Agent`。
Gateway 支持两种上游传输：**stdio** 与 **HTTP**。

## 现状结论（已审 AIRP-MCP-Server `src/transport/`）

| 传输 | 状态 | 是否需改 |
|------|------|---------|
| stdio（`airp-mcp mcp --data-dir`） | ✅ 真实 MCP：`serve_server(Router, rmcp::transport::io::stdio())` | **勿动**，Gateway 现可对接 |
| HTTP（`airp-mcp serve --bind`，`/mcp/v1`） | ⛔ 未完成的桩 | **需你方修复** |

### HTTP 模式当前的问题
1. `POST /mcp/v1` 的 `handle_mcp_post` 返回空 `{"jsonrpc":"2.0","id":...,"result":{}}`，`State(_state)` 未使用，**从不转发给 rmcp 服务**。
2. `GET /mcp/v1` 的 SSE 为单一全局广播，**无会话隔离**，无 `Mcp-Session-Id`。
3. `Cargo.toml` 的 `rmcp` 仅启用 `server, transport-io, macros`，**缺** `transport-streamable-http-server`（AIRP-Core 已启用）。

---

## 需求（MCP 规范 2025-06-18，Streamable HTTP transport）

| 编号 | 需求 |
|------|------|
| **R1（核心）** | `POST /mcp/v1` 必须把 JSON-RPC 请求真实派发给 rmcp 服务并返回真实 `result`/`error`，而非空对象 |
| **R2 生命周期** | 完整支持 `initialize`（返回真实 `protocolVersion`+`capabilities`+`serverInfo`）→ 接受 `notifications/initialized` → 之后处理 `tools/list`、`tools/call`、`resources/read` 等 |
| **R3 会话** | `initialize` 响应返回 `Mcp-Session-Id` 头；后续请求校验该头；SSE 按会话隔离（非全局广播） |
| **R4 协议头** | 初始化后所有请求要求并校验 `MCP-Protocol-Version` 头 |
| **R5 内容协商** | 依 `Accept: application/json, text/event-stream`：单次响应用 `application/json`，流式用 `text/event-stream`(SSE) |
| **R6 鉴权** | 保留 `AIRP_HTTP_TOKEN` 的 bearer + 常数时间校验，统一作用于 `/mcp/v1` |
| **R7 CORS** | 允许请求头 `Authorization, Mcp-Session-Id, MCP-Protocol-Version`，并 expose `Mcp-Session-Id` |
| **R8 错误** | 规范 JSON-RPC error code（如协议版本不符返回 `-32602`） |

---

## 建议实现路径（省事且与 AIRP-Core 一致）

给 `rmcp` 开启 `transport-streamable-http-server`（及 session 相关 feature，参照 AIRP-Core 的 `Cargo.toml`），用 rmcp 自带的 streamable-HTTP router 挂到 `/mcp/v1`，替换手写桩。
如此 R1–R5、R8 基本由 rmcp 满足，你只需保留外层鉴权(R6)与 CORS(R7)。

---

## 验收标准（Gateway 将据此验证）

1. `POST /mcp/v1` 发 `initialize` → 返回真实 `protocolVersion`（与请求一致或服务端支持版本）+ 响应头含 `Mcp-Session-Id`。
2. 带会话头发 `tools/list` → 返回全部 38 个工具。
3. 发 `tools/call`（如 `list_characters`）→ 返回真实内容，而非空 `{}`。

## 不需要改的

stdio 模式、数据模型、工具实现，全部维持现状。
