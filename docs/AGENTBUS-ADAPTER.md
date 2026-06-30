# AIRP State-Protocol AgentBus Adapter

> Status: implemented (adapter module + example + integration tests)
> Owner: AIRP-Gateway
> Counterpart: AIRP-State-Protocol UI (`SSEBus`)
> Related: ADR-007 (adapter layer is optional, not part of the core)

## What this is

An **optional frontend** that exposes the gateway core (`GatewayState` + `Bridge`)
over the State-Protocol `Envelope` wire format, transported on SSE. This is the
integration surface AIRP-State-Protocol's UI talks to when it is not running
inside the Tauri shell.

It lives in `src/agentbus/` and `examples/agentbus_sse.rs`. It does **not**
modify `bridge/mod.rs` or `server/mod.rs` — per ADR-007, the adapter is built
entirely on top of the already-public `GatewayState::build` / `Bridge::dispatch`
seam.

## HTTP surface

| Method | Path              | Body / Response                | Purpose                         |
|--------|-------------------|--------------------------------|---------------------------------|
| POST   | `/airp/dispatch`  | JSON `Envelope`                | Upstream: UI → gateway          |
| GET    | `/airp/stream`    | `text/event-stream`            | Downstream: gateway → UI (SSE)  |

Each SSE `data:` line is one JSON `Envelope`. The UI opens the SSE connection
once, holds it open, and receives every downstream envelope the gateway emits.
`subscribe` intents narrow which scopes a given connection receives.

### Connection correlation

The UI assigns itself a connection id when it opens `GET /airp/stream`. It
echoes that id as the `x-airp-conn` header on every `POST /airp/dispatch`. The
adapter uses this to attribute `subscribe` intents to the right SSE stream.

(SSE has no client→server channel, so the connection id is the join key. If you
later switch to WebSocket, the connection id becomes implicit and this header
goes away.)

## Mapping rules (the only designed part)

| UI upstream                          | Gateway action                                                                | Downstream reply                                    |
|--------------------------------------|-------------------------------------------------------------------------------|-----------------------------------------------------|
| `hello`                              | log client + accept list; emit initial blueprint / manifests / per-scope state | multiple `Envelope`s + an `ack`                    |
| `intent name=X params=P source=S`    | `Bridge::match_route("POST", "/v1/X")` → `dispatch` → MCP `call_tool`         | `state op:set` on scope `S` (or fallback)           |
| `subscribe scopes=[...]`             | set this connection's scope filter                                            | none (ack only)                                     |
| `ack ref=R`                          | log/metric                                                                    | none                                                |

### Decisions (locked)

1. **intent → RouteRule mapping: reuse existing `[[routes]]`.** The intent name
   is the route path minus the `/v1/` prefix. `chat.send` → `POST /v1/chat.send`.
   Zero new config. No match → downstream `error` envelope with `code: "no_route"`.

2. **MCP result → state envelope: `state op:set` the whole scope.** The adapter
   prefers `structuredContent` from the MCP tool result (per spec); falls back to
   the raw result. It emits a single `replace` patch at the scope root. Per-field
   patches are a later iteration — `set` is a valid patch and the UI reactive
   store handles it by replacing the scope.

3. **scope resolution order: `intent.source` → `fallback_scopes` table →
   `default_scope` → `error`.** The `source` field on the intent body (the widget
   instance id that emitted it) always wins. Without it, the adapter consults a
   per-intent fallback table, then a global default. If none resolve, the intent
   is rejected with a downstream `error` envelope (`code: "no_scope"`).

## Envelope wire contract

Truth lives in `AIRP-State-Protocol/schema/airp-state-protocol.schema.json`
(draft 2020-12). The adapter implements the subset needed for the minimal closed
loop and is forward-compatible: unknown `kind` values deserialize to a catch-all
`Unknown` variant (logged, dropped) rather than crashing the gateway.

Notable: the correlation field is `ref` on the wire (a Rust keyword); the Rust
struct field is `ref_` with `#[serde(rename = "ref")]`. Same for `Ack.ref` and
`Error.ref`. See `src/agentbus/envelope.rs` for the full type list.

## How the UI connects

On the AIRP-State-Protocol side, add an `SSEBus implements AgentBus`:

```typescript
// dispatch (upstream)
await fetch(`${gateway}/airp/dispatch`, {
  method: "POST",
  headers: { "content-type": "application/json", "x-airp-conn": connId },
  body: JSON.stringify(envelope),
});

// subscribe (downstream)
const es = new EventSource(`${gateway}/airp/stream`);
es.onmessage = (e) => handler(JSON.parse(e.data));
```

`bus-factory.ts` picks `SSEBus` when not in the Tauri shell; `TauriBus` still
used inside the shell (Tauri IPC → Rust core → Gateway HTTP). The transport is
transparent to the UI — only the bus implementation changes.

## Running the example

The example binary mounts the adapter alongside the gateway's built-in HTTP
surface, with a stdio mock MCP upstream so it runs with no external project:

```sh
# 1. Build the mock MCP server (the stdio upstream).
cargo build --example mock_mcp_stdio

# 2. Run the adapter example.
AIRP_MCP_BIN=target/debug/examples/mock_mcp_stdio \
  cargo run --example agentbus_sse

# 3. The gateway logs its surface:
#    POST http://127.0.0.1:8080/airp/dispatch
#    GET  http://127.0.0.1:8080/airp/stream
#    upstream: stdio `mock_mcp_stdio` (tool `echo`)
```

Env knobs: `AIRP_BIND` (default `127.0.0.1:8080`), `AIRP_MCP_BIN` (the stdio
binary path), `AIRP_ACCESS_KEY` (optional bearer; unset = open).

The example wires one route: `POST /v1/chat.send` → MCP tool `echo`. The mock
echoes the call args back under `structuredContent`, so the closed loop is:

```
UI POST /airp/dispatch { intent chat.send, params {text:"hello"} }
  → adapter matches route /v1/chat.send
  → Bridge::dispatch → McpClient::call_tool("echo", {text:"hello"})
  → mock returns { structuredContent: { reply:"echo", received:{text:"hello"} } }
  → adapter emits state op:set on scope w-chat (from intent.source or fallback)
  → broadcast → SSE → UI receives → patch reactive store → ChatWidget re-renders
```

## Verification

- **Integration tests** (`tests/agentbus_adapter.rs`): the minimal closed loop
  against an in-process mock MCP transport (no subprocess). Five tests cover:
  intent → state patch roundtrip; unknown intent → error envelope; scope
  fallback table; `hello` emits initial blueprint/state; wrong envelope version
  rejected.
- **Cross-process e2e** (`tests/e2e_stdio.rs`, existing): the gateway core
  roundtrip via a real subprocess. The adapter sits on top of the same core, so
  this still validates the MCP dispatch path the adapter relies on.

## Limits / next iterations

- **Streaming tools** (`RouteTarget::Tool { stream: true }`): the adapter
  rejects these with a `stream_unsupported` error envelope. Streaming maps onto
  the gateway's own Stage 2 SSE work (DESIGN.md §Stage 2); when that lands, the
  adapter forwards partial results as incremental `state op:patch` envelopes.
- **`state op:patch` granularity**: today the adapter emits `op:set` (whole
  scope replace). Per-field JSON Patch needs per-tool patch templates, which
  belong in a later, domain-aware mapping config — not the current generic core.
- **blueprint/manifest authority**: the example wires a static initial
  blueprint. A real deployment would source it from a config file or a dedicated
  MCP resource read on `hello`.
