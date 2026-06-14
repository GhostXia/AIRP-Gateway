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

stdio 模式的工具实现、数据模型，全部维持现状。

---

# 第二批需求（stdio 真实联调 + 强制 CI 验证）

> 建档 2026-06-13。Gateway 核心已完成并 CI 全绿（mock 传输 e2e 测试）。下面是与 MCP-Server 做"真实进程"联调所需。

## A. stdio（优先：确认契约 + 提供可执行）

Gateway 以子进程拉起 `airp-mcp mcp --data-dir <dir>`，行分隔 JSON-RPC 通信。请确认/保证（任一不符请告知）：

| # | 契约 |
|---|------|
| A1 | `airp-mcp mcp --data-dir ./data` 进入 stdio MCP 服务，stdin 收 / stdout 发 |
| A2 | stdout 每行一个完整 JSON-RPC 2.0 对象，对象内无内嵌换行（newline-delimited，非 Content-Length 帧） |
| A3 | 日志/诊断只写 **stderr**，绝不污染 stdout |
| A4 | 生命周期：`initialize`（返回真实 `protocolVersion`+`capabilities`+`serverInfo`）→ 接受 `notifications/initialized` → 处理 `tools/list`、`tools/call`。Gateway 发送 `protocolVersion = "2025-06-18"`；若你方版本不同，回你方版本并告知我适配 |
| A5 | 冒烟工具：指定一个**只读、空目录可成功、无副作用**的工具（建议 `list_characters`，空目录返回空列表）。给出：工具名、arguments JSON schema、result 结构示例 |
| A6 | stdin 关闭后进程自行退出 |

**交付物（关键）**：提供 Linux x86_64 的 `airp-mcp` 可执行文件——经 GitHub Release 资产或 CI artifact 发布。原因：Gateway CI 跑在 Linux，需下载二进制做真实跨进程 e2e。若只能产 Windows 产物，告知我改用 windows runner。

## B. HTTP（次要，当前阻塞）

见本文档上半部 R1–R8。建议给 rmcp 开 `transport-streamable-http-server` feature + 挂载其 router 替换手写桩。不阻塞 stdio，可后做。

## C. 验证方式（硬性：必须用 GitHub Actions workflow，不接受口头"已完成"）

AIRP-MCP-Server 仓库须有 GitHub Actions workflow，且包含真实功能验证，CI 必须为绿：

1. 构建 + 单元/集成测试（`cargo build` / `cargo test`）。
2. **stdio e2e 测试（必须）**：测试内以子进程或进程内启动 MCP 服务，发 `initialize` → `notifications/initialized` → `tools/call`（用 A5 工具），断言：initialize 返回非空 `protocolVersion`+`serverInfo`；tools/call 返回**真实结果**（空目录 `list_characters` 返回空列表），非空 `{}` 或桩。
3. **http e2e 测试**（HTTP 完成后必须加）：真实 MCP 客户端打 `/mcp/v1`，断言响应头含 `Mcp-Session-Id` 且 tools/call 返回真实内容。
4. 完成后把 CI run 链接发我（须显示 success）。

以 CI 绿为准，不接受"本地测过"或截图——与 Gateway 一致（Gateway CI 已 fmt/clippy/test 全绿）。

## D. 验收（Gateway 据此判定可联调）

- MCP-Server CI 绿，含 A 段 stdio e2e 测试。
- Gateway 拿到 Linux `airp-mcp` 二进制（release/artifact）。
- Gateway CI 加 job：下载二进制 → 真实子进程 → initialize + tools/call 断言真实数据。两侧 CI 同绿 = 对接成功。
