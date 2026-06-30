# AIRP-Gateway · 计划书（Roadmap）

> 本文件是**前瞻性计划书**：当前能力快照 + 未来展望 + 版本规划。
> 偏「做什么、往哪走、为什么」。开发过程的详细追踪/决策/研究见 [`DESIGN.md`](DESIGN.md)。
> 最后更新：2026-06-29

---

## 一、定位（不变）

通用、高性能的**纯协议桥**：`任意前端 (HTTP/SSE) → AIRP-Gateway → 任意 MCP 服务 → Agent/后端`。

- **乐高，不是套件**：对接任意 MCP server、服务任意前端，可完全独立使用，零外部项目依赖。
- 库优先（无 exe）、前端无关、传输无关、轻依赖可移植。
- 业务逻辑归上游 MCP 服务；Gateway 只做鉴权、限流、转发。

---

## 二、当前状态快照（2026-06-29）

### 已完成能力
| 能力 | 状态 | 验证 |
|------|------|------|
| 分层配置（default → TOML → env） | ✅ | 单测 |
| 鉴权（常数时间 bearer）/ CORS / 限流（governor per-IP） | ✅ | 集成测试 |
| 声明式路由 `RouteRule`（path/method → tool/resource） | ✅ | 集成测试 |
| stdio 传输（子进程 + NDJSON + initialize 握手） | ✅ | 跨进程 e2e（自带 mock） |
| HTTP 传输（session-id、协商版本头、JSON + SSE） | ✅ | 真实 TCP e2e（in-process mock） |
| 协议版本协商捕获 + 协商校验（不支持则断开） | ✅ | e2e + 单测 |
| 前端无关核心（`GatewayState::build` + 暴露 `Bridge` 等） | ✅ | API |
| 安全：stdio 命令白名单(opt-in) + 默认 loopback + 暴露告警 | ✅ | 单测（ADR-008） |
| **健壮性首批（ADR-009）**：HTTP SSRF 阵御 / 请求体上限 / 错误脱敏 / 优雅关机 / stdio 规范关机序列 + EOF drain / notification broadcast / UpstreamPool 构建回滚 | ✅ | 单测 + CI 全绿 |
| **Stage 6 深化（P2a-P2d）**：请求超时 / 上游响应大小上限 / stdio args 校验 / 故障注入 e2e | ✅ | 集成测试 + CI 全绿 |
| **安全堵漏（R11）**：HTTP 流式读防 OOM / stdio 行长上限 / 配置引用完整性校验 | ✅ | 单测 + CI 全绿 |

### 验证策略（已确立）
- **全部经 GitHub Actions workflow 验证**，CI 绿为准。
- **自给自足**：CI 用本仓库自带 mock（`examples/mock_mcp_stdio.rs` + in-process HTTP mock），**不依赖任何外部项目**。
- 三类检查：`test`(fmt + clippy -D warnings + 单测 + http e2e) ｜ `e2e-stdio`(跨进程)。

### 里程碑进度（详见 DESIGN §5）
- ✅ Stage 0 脚手架 ｜ ✅ Stage 1 stdio 端到端 ｜ ✅ Stage 3 HTTP 传输 ｜ ✅ 安全加固（ADR-008）｜ ✅ 健壮性首批（ADR-009）｜ ✅ Stage 6 深化批（P2a-P2d）｜ ✅ 安全堵漏（R11）
- ⬜ Stage 2 流式（**下一步**）｜ ⬜ Stage 4 路由增强 ｜ ⬜ Stage 5 嵌入 Core ｜ ⬜ Stage 6 硬化（深化续：重连/健康检查/可观测性）

---

## ⭐ 接力指引（给下一个 agent）

> 冷启动接手本项目，按此即可。

**先读**：本文件 §一/§二 → `DESIGN.md` §4（架构快照）+ §5（阶段看板）。真理顺序：源码 > DESIGN > 本文件。

**构建 / 验证（本机有坑，必读）**：
- 无 MSVC linker，用 GNU 工具链；`D:\` 盘构建脚本执行被系统策略拒（`os error 5`），target 目录须重定向到 C:。
- 本机只能 `check` / `fmt` / `clippy`（无 codegen）；**完整 `test` 跑不了**（缺 `dlltool`）→ **测试一律靠 CI 验证**。
```powershell
$env:CARGO_TARGET_DIR = "C:\Users\xiach\airp-gw-target"
cargo +stable-x86_64-pc-windows-gnu fmt --all
cargo +stable-x86_64-pc-windows-gnu clippy --all-targets -- -D warnings
```
- 改完 → commit → push → `gh run watch <id> --repo GhostXia/AIRP-Gateway --exit-status`。CI 绿才算完成。
- 提交信息含双引号时，PowerShell here-string 会被 git 拆碎 → 用单行 `-m` 或 `$msg` 变量。

**仓库地图**：
| 路径 | 作用 |
|------|------|
| `src/config.rs` | 配置 + `validate()`（安全校验） |
| `src/server/` | axum 前端：`mod.rs`(Gateway/router/run) `middleware.rs`(auth/cors) `handlers.rs`(dispatch) |
| `src/bridge/mod.rs` | 请求→MCP 分发（`DispatchOutcome::Stream` 是 Stage 2 桩） |
| `src/mcp/` | `client.rs`(握手/call_tool/list_tools) `pool.rs` `transport/{mod,stdio,http}.rs` `types.rs` |
| `examples/mock_mcp_stdio.rs` | 自带 mock（e2e 用，勿删） |
| `tests/{integration,e2e_stdio,e2e_http}.rs` | mock-传输 / 跨进程 / 真实 HTTP |
| `.github/workflows/ci.yml` | 两 job：`test` + `e2e-stdio` |

**铁律（不得违反，详见 DESIGN §1/§2）**：纯协议桥、库优先、传输无关、不捆绑任何项目、新功能必须自给自足 CI 验证。

**下一步 = Stage 2 流式**（见下）。入口：`bridge::DispatchOutcome::Stream` + `handlers::dispatch` 的 stream 分支 + `McpTransport` 加 `request_stream`。

---

## 三、路线图（未来展望）

> 每项：目标 / 价值 / 验收（均要求 workflow 验证、自给自足）。
> 优先级标记：P0（立即）→ P1（近期）→ P2（中近期）→ P3（中期）→ P5（远期）。

### P0 · 当前分支落地 + 安全堵漏

**提交 ADR-009 健壮性首批改动 → CI 验证 → 合 main**
- 内容：SSRF 阵御 / 请求体上限 / 错误脱敏 / 优雅关机 / stdio 规范关机序列 + EOF drain / notification broadcast / 协议版本协商校验 / UpstreamPool 构建回滚（详见 DESIGN ADR-009）+ P2a-P2d 深化（超时/响应上限/args校验/故障注入）。
- 验收：CI 全绿（fmt + clippy -D warnings + test + e2e-stdio）。✅ 已达成。

**安全堵漏（R11 高优先项，当前 PR 续）** ✅ 已落地
- **R11.1 · HTTP 响应流式读 + 先检 Content-Length** ✅：改为 `bytes_stream()` 逐 chunk 累加 + `Content-Length` 头先检，恶意上游无法 OOM Gateway。
- **R11.7 · stdio 行长度上限** ✅：逐字节读 + `MAX_LINE_BYTES=1MiB`，超限断开子进程并 drain pending。
- **R11.3 · Io/Json 错误脱敏完善** ✅：审读确认 `is_client_safe()` 已正确排除 `Io`/`Json`，`into_response` 对非 safe 变体返回泛化消息。
- **R11.5 · 配置校验：路由引用上游存在性** ✅：`validate()` 加 `upstream_names` HashSet 引用完整性检查 + 单测 `route_referencing_unknown_upstream_is_rejected`。
- **R11.8 · http.rs 超时常量** ✅：审读确认 `CONNECT_TIMEOUT`/`REQUEST_TIMEOUT` 已在 `with_max_response()` 中使用。
- 验收：CI 全绿 ✅（fmt + clippy -D warnings + test + e2e-stdio）。

### P1 · 近期

**Stage 2 · 流式（streaming，SSE 前端 ↔ MCP 增量）** — 下一步
- 目标：`RouteTarget::Tool{stream:true}` 经 `axum::Sse` 把上游增量结果推给前端（当前为 `Unimplemented` 桩）。
- 价值：聊天/长任务实时回显；当前唯一明确功能缺口。
- 已有基础：`StdioTransport::subscribe_notifications()`（broadcast channel）、`HttpTransport::parse_sse`、`DispatchOutcome::Stream` 桩、`RouteTarget::Tool{stream}` 配置字段。
- 实现步骤：
  1. `McpTransport` trait 增 `async fn request_stream(&self, req) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>>>>>`，定义 `StreamEvent` 枚举（`Progress` / `Data` / `Done`）。
  2. `StdioTransport::request_stream`：发请求后从 `subscribe_notifications()` 过滤 `notifications/progress`，直到收到最终 result。
  3. `HttpTransport::request_stream`：发 POST + `Accept: text/event-stream`，用 `reqwest` 的 `bytes_stream()` 逐帧解析 SSE。
  4. `handlers::dispatch` 的 `DispatchOutcome::Stream` 分支：返回 `axum::response::Sse<impl Stream>`。
  5. 背压/断连：客户端断开时 cancel token 传播到上游。
- 验收：`McpTransport` 增 `request_stream`；mock 产出多帧 progress；e2e 断言前端收到有序 SSE 流；断连不泄漏。
- 参考：ST-ClaudeCacheGateway 的 SSE 流式透传（DESIGN R9）。

### P2 · 中近期

**Stage 6 深化 · 请求超时 + 取消传播**
- 目标：`McpClient::invoke` 包 `tokio::time::timeout`；客户端断开时 cancel 上游请求。
- 配置：`GatewayConfig.upstream_timeout`（默认 30s）。
- 验收：mock 上游延迟 >timeout → Gateway 返回 504；客户端断开 → 上游请求被 cancel。

**Stage 6 深化 · 上游健康检查 + 自动重连**
- 目标：`McpClient::health_check()`（ping / 轻量 tools/list）；`UpstreamPool` 后台 tick 检查；unhealthy 标记 + 自动重连。
- 配置：`health_check_interval`、`max_reconnect_attempts`。
- 验收：mock 上游中途崩溃 → Gateway 标记 unhealthy → 重启后自动恢复。

**Stage 6 深化 · 上游响应大小上限 + stdio args 校验**
- 目标：`max_response_bytes`（默认 10 MiB），HTTP transport 读 body 前检查 Content-Length / 读后检查实际长度；stdio reader 累积行长度超限断开。`validate()` 中 stdio args 非空时 warn 或 opt-in `allow_arbitrary_args`。
- 验收：mock 上游发 >limit 响应 → Gateway 返回 502；config args 校验单测。

**故障注入 e2e 测试**
- 目标：覆盖非 happy path：上游崩溃 / 超时 / 慢响应 / 巨大响应 / 无效 JSON。
- 验收：每种场景 Gateway 不挂起、返回规范错误码，CI 全绿。

### P3 · 中期

**Stage 4 · 路由与兼容增强**
- 目标：覆盖复杂映射（如 OpenAI 兼容 `/v1/chat/completions`）；可选 request/response 变换层（保持核心纯净）；多上游 fan-out 策略。
- 子项：
  - **路径参数**：`RouteRule::path` 支持 `{param}` 占位符，匹配时提取参数注入 MCP tool arguments。
  - **变换层**：声明式 jmespath/jq 风格 JSON 映射或 WASM 沙箱插件，在 `bridge::dispatch` 前后执行，**不进 bridge 核心**。
  - **多上游 fan-out**：`RouteTarget::MultiTool { targets: Vec<(upstream, tool)> }`，`tokio::join!` 并行调用，聚合为 JSON array。
- 价值：对接更多现成前端，降低接入门槛。
- 验收：变换层为可选插件，不进 `bridge`；e2e 覆盖一种兼容端点。
- 边界提醒：prompt caching / 格式转换属 LLM 跳（Agent / MCP-Server），不进纯桥。

**可观测性**
- 目标：prometheus metrics + 请求追踪 + 结构化日志。
- 子项：
  - 关键指标（feature flag `metrics`）：`requests_total{route,upstream,status}` / `request_duration_seconds{route}` / `upstream_health{upstream}` / `rate_limit_rejected_total{ip}`。
  - `/metrics` 端点（可选，需 admin key）。
  - `x-request-id` 中间件：无则生成 UUID，全链路 tracing 带 request_id，透传到上游 HTTP。
  - 可选 JSON 日志格式（`tracing-subscriber` 的 `fmt().json()`，feature flag `json-log`）。
- 验收：`/metrics` 返回合规 prometheus text；request_id 贯穿日志。

**Stage 5 · 嵌入 AIRP-Core 验证（可选生态）**
- 目标：在 Core 内 `Gateway::build().run()` 跑通，替换其 daemon 的转发部分。
- 价值：证明「可并入」承诺。
- 验收：Core 侧构建/运行通过；构建目录策略（D: 盘）在 Core 侧解决。

### P5 · 远期

- **开发者体验**：
  - 配置校验扩展：路由引用的上游必须存在、path 不重复、启动时打印配置摘要。
  - 运行时状态端点：`GET /status`（各上游健康/版本/最后请求时间）、`GET /routes`（当前路由表），走独立 `admin_key`。
  - OpenAPI 从 routes 自动导出（`GET /openapi.json`，feature flag）。
  - 热重载：`SIGHUP` → 重载 routes（不改 upstreams 连接）；后续 admin API 动态增删；`notify` crate watch 文件变化。
- **可选前端适配层**（独立 crate / example）：如 AIRP-State-Protocol 的 `AgentBus`、WebSocket、gRPC —— 全部基于已暴露的 `GatewayState`/`Bridge`，**不进核心**。证明「任意前端」。
- **可移植性**：维持「移植契约」（JSON-RPC 线格式、`RouteRule`/`GatewayConfig` schema、中间件顺序），使未来换语言可照搬。见 DESIGN §9。
- **性能基线**：criterion benchmark（单次 tool call 延迟、并发 100 吞吐），CI 回归超 10% 告警。守住「高性能」定位。
- **属性测试**：`proptest` 覆盖 `parse_sse` / `url_is_private_or_loopback` / `RouteRule` 匹配等纯函数边界。
- **限流键可选**：per-IP（当前）+ per-token + 混合（Stage 6 留项）。

---

## 四、版本规划（暂定）

| 版本 | 内容 | 状态 |
|------|------|------|
| 0.1.x | 核心桥 + stdio/HTTP 传输 + 鉴权/限流/路由 + 安全(ADR-008+009) + 健壮性首批 + P2a-P2d 深化 + R11 安全堵漏 + 自给自足 CI | 当前 |
| 0.2.0 | Stage 2 流式 | 计划（P1） |
| 0.3.0 | Stage 6 深化（超时/重连/健康检查/响应上限/args校验）+ 故障注入 e2e | 计划（P2） |
| 0.4.0 | Stage 4 路由/兼容增强 + 可观测性 | 计划（P3） |
| 0.5.0 | Stage 5 Core 嵌入验证 + 可选前端适配示例 | 计划（P3） |
| 1.0.0 | API 稳定 + 性能基线 + 属性测试 + 文档完备 | 远期（P5） |

> 版本号非承诺，随实际推进调整。原则：每个版本都必须 CI 全绿、自给自足、不破坏「不变量」（DESIGN §1/§2）。

---

## 五、不做什么（边界）

- 不做推理 / 不拼 prompt / 不懂业务语义。
- 不内建任何项目专属逻辑（角色卡、预设等归上游 MCP）。
- 不强绑任何前端协议；内置 HTTP 只是「其中一个前端」。
- 不为单一第三方写进核心的适配器（应作外部可选 crate/example）。
