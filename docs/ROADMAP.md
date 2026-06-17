# AIRP-Gateway · 计划书（Roadmap）

> 本文件是**前瞻性计划书**：当前能力快照 + 未来展望 + 版本规划。
> 偏「做什么、往哪走、为什么」。开发过程的详细追踪/决策/研究见 [`DESIGN.md`](DESIGN.md)。
> 最后更新：2026-06-14

---

## 一、定位（不变）

通用、高性能的**纯协议桥**：`任意前端 (HTTP/SSE) → AIRP-Gateway → 任意 MCP 服务 → Agent/后端`。

- **乐高，不是套件**：对接任意 MCP server、服务任意前端，可完全独立使用，零外部项目依赖。
- 库优先（无 exe）、前端无关、传输无关、轻依赖可移植。
- 业务逻辑归上游 MCP 服务；Gateway 只做鉴权、限流、转发。

---

## 二、当前状态快照（2026-06-14）

### 已完成能力
| 能力 | 状态 | 验证 |
|------|------|------|
| 分层配置（default → TOML → env） | ✅ | 单测 |
| 鉴权（常数时间 bearer）/ CORS / 限流（governor per-IP） | ✅ | 集成测试 |
| 声明式路由 `RouteRule`（path/method → tool/resource） | ✅ | 集成测试 |
| stdio 传输（子进程 + NDJSON + initialize 握手） | ✅ | 跨进程 e2e（自带 mock） |
| HTTP 传输（session-id、协商版本头、JSON + SSE） | ✅ | 真实 TCP e2e（in-process mock） |
| 协议版本协商捕获 | ✅ | e2e |
| 前端无关核心（`GatewayState::build` + 暴露 `Bridge` 等） | ✅ | API |
| 安全：stdio 命令白名单(opt-in) + 默认 loopback + 暴露告警 | ✅ | 单测（ADR-008） |

### 验证策略（已确立）
- **全部经 GitHub Actions workflow 验证**，CI 绿为准。
- **自给自足**：CI 用本仓库自带 mock（`examples/mock_mcp_stdio.rs` + in-process HTTP mock），**不依赖任何外部项目**。
- 三类检查：`test`(fmt + clippy -D warnings + 单测 + http e2e) ｜ `e2e-stdio`(跨进程)。

### 里程碑进度（详见 DESIGN §5）
- ✅ Stage 0 脚手架 ｜ ✅ Stage 1 stdio 端到端 ｜ ✅ Stage 3 HTTP 传输 ｜ ✅ 安全加固（ADR-008）
- ⬜ Stage 2 流式（**下一步**）｜ ⬜ Stage 4 路由增强 ｜ ⬜ Stage 5 嵌入 Core ｜ ⬜ Stage 6 硬化

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

### 近期

**Stage 2 · 流式（streaming，SSE 前端 ↔ MCP 增量）** — 下一步
- 目标：`RouteTarget::Tool{stream:true}` 经 `axum::Sse` 把上游增量结果推给前端（当前为 `Unimplemented` 桩）。
- 价值：聊天/长任务实时回显；当前唯一明确功能缺口。
- 验收：`McpTransport` 增 `request_stream`；mock 产出多帧；e2e 断言前端收到有序 SSE 流；背压/断连处理。
- 参考：ST-ClaudeCacheGateway 的 SSE 流式透传（DESIGN R9）。

**Stage 4 · 路由与兼容增强**
- 目标：覆盖复杂映射（如 OpenAI 兼容 `/v1/chat/completions`）；可选 request/response 变换层（保持核心纯净）；多上游 fan-out 策略。
- 价值：对接更多现成前端，降低接入门槛。
- 验收：变换层为可选插件，不进 `bridge`；e2e 覆盖一种兼容端点。
- 参考：ST-ClaudeCacheGateway 的 chat/completions ↔ messages 转换（DESIGN R9）。注意：本项目映射到 MCP 工具，非 LLM messages。
- 边界提醒：prompt caching / 格式转换属 LLM 跳（Agent / MCP-Server），不进纯桥。

### 中期

**Stage 5 · 嵌入 AIRP-Core 验证（可选生态）**
- 目标：在 Core 内 `Gateway::build().run()` 跑通，替换其 daemon 的转发部分。
- 价值：证明「可并入」承诺。
- 验收：Core 侧构建/运行通过；构建目录策略（D: 盘）在 Core 侧解决。

**Stage 6 · 硬化（生产可用）**
- 目标：请求超时 + 取消通知；上游重连 / 健康检查；指标与追踪；限流键可选（IP/token）。
- 安全续（ADR-008 留项）：HTTP upstream SSRF 防护（url 指向内网）、上游响应大小上限、stdio args 校验。
- 价值：生产环境稳定性 + 安全。
- 验收：故障注入测试（上游崩溃/超时/慢响应）经 CI 验证。

### 远期展望

- **可选前端适配层**（独立 crate / example）：如 AIRP-State-Protocol 的 `AgentBus`、WebSocket、gRPC —— 全部基于已暴露的 `GatewayState`/`Bridge`，**不进核心**。证明「任意前端」。
- **可移植性**：维持「移植契约」（JSON-RPC 线格式、`RouteRule`/`GatewayConfig` schema、中间件顺序），使未来换语言可照搬。见 DESIGN §9。
- **性能基线**：建立吞吐/延迟基准（criterion + CI），守住「高性能」定位。
- **可观测性**：结构化日志 + 可选 metrics 导出。

---

## 四、版本规划（暂定）

| 版本 | 内容 | 状态 |
|------|------|------|
| 0.1.x | 核心桥 + stdio/HTTP 传输 + 鉴权/限流/路由 + 安全(ADR-008) + 自给自足 CI | 当前 |
| 0.2.0 | Stage 2 流式 | 计划 |
| 0.3.0 | Stage 4 路由/兼容增强 | 计划 |
| 0.4.0 | Stage 6 硬化（超时/重连/指标） | 计划 |
| 0.5.0 | Stage 5 Core 嵌入验证 + 可选前端适配示例 | 计划 |
| 1.0.0 | API 稳定 + 性能基线 + 文档完备 | 远期 |

> 版本号非承诺，随实际推进调整。原则：每个版本都必须 CI 全绿、自给自足、不破坏「不变量」（DESIGN §1/§2）。

---

## 五、不做什么（边界）

- 不做推理 / 不拼 prompt / 不懂业务语义。
- 不内建任何项目专属逻辑（角色卡、预设等归上游 MCP）。
- 不强绑任何前端协议；内置 HTTP 只是「其中一个前端」。
- 不为单一第三方写进核心的适配器（应作外部可选 crate/example）。
