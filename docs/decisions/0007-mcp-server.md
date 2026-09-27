# 0007: The read-only MCP server

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #52 · Research [0003 §5](../../research/0003-rust-substrate.md)

## Decision

Expose the world query contract through `s2w mcp`, using `rmcp =3.4.1` with default
features disabled and only `server`, `macros`, and `transport-io` enabled. Research
0003 §5 recommends the official, spec-current, Tokio-native SDK with read-only tool
annotations. Stdio-only gives the smallest read-only transport footprint among the
options considered; the exact pin makes the fast-moving SDK's upgrades explicit reviews.
The `client` feature is enabled only as a dev-dependency for an in-process round trip.

Use stdio only, from a current-thread Tokio runtime built at the synchronous CLI entry
point. Stdout is exclusively the JSON-RPC channel; diagnostics go to stderr. No listener,
authentication layer, or Streamable HTTP transport is added here.

## Contract details

| Tool | Mirrors HTTP route | Pure function(s) it calls |
|---|---|---|
| `world_view` | `GET /world?at=&branch=&lod=&focus=&hops=` | `query::world_view` |
| `world_diff` | `GET /diff?from=&to=` | `query::diff` |
| `entity_history` | `GET /entity/{id}/history?to=` | `Timeline::history` |
| `branches` | `GET /branches` | `QueryState::branches` (shared branch summary) |
| `time` | `GET /time?ts=` | `Timeline::time_range` / `Timeline::offset_at` |

All five advertise `read_only_hint: true`. `/events` is an SSE subscription, not a sixth
request/response tool. HTTP and MCP call the same `QueryState` methods; MCP also reuses
the HTTP branch and level-of-detail validators. Optional parameters and defaults match
the routes, including `from=0`, `to=head`, and the actual branch.

Successful results contain one text item from `serde_json::to_string(&dto)`, without
`structured_content`, preserving the HTTP JSON bytes. Domain `QueryError`s return
`is_error: true` with the same JSON error body (`error` from `QueryError::code()` and
`message` from its display text). Missing required or malformed typed parameters are
rejected at the JSON-RPC protocol level before the handler runs; that channel is not
required to mirror HTTP's 400 body. rmcp 3.4.1's `ToolRouter::call` would convert typed
deserialization failures into error tool content. Our `ServerHandler::call_tool` invokes
the macro-generated route directly, preserving its `INVALID_PARAMS` error instead; the
fixed five-tool set has no dynamically disabled routes. Integration tests compare actual response bytes on
the golden fixture and an empty timeline, and exercise initialization, tool discovery,
calls and both error channels over `tokio::io::duplex`.

Measured in this worktree on 2026-09-27 (`--offline` used with cached crates):

```text
$ cargo tree -e no-dev -p s2w-app -i rmcp
rmcp v3.4.1
└── s2w-app v0.0.0 (/tmp/stream2worlds-52/crates/s2w-app)
```

The reverse tree above identifies the consumer. The forward tree below records the
actual resolved stdio-only dependency tail, including transitive dependencies; `(*)`
marks a repeated subtree. It contains no HTTP transport stack. `transport-io` enables
Tokio's `io-std` feature through rmcp, so the app needs no additional normal Tokio feature.

<details>
<summary>Measured rmcp dependency tail</summary>

```text
$ cargo tree -e no-dev -p rmcp
rmcp v3.4.1
├── chrono v0.4.45
│   ├── num-traits v0.2.19
│   │   [build-dependencies]
│   │   └── autocfg v1.5.1
│   └── serde v1.0.229
│       ├── serde_core v1.0.229
│       └── serde_derive v1.0.229 (proc-macro)
│           ├── proc-macro2 v1.0.107
│           │   └── unicode-ident v1.0.26
│           ├── quote v1.0.47
│           │   └── proc-macro2 v1.0.107 (*)
│           └── syn v3.0.6
│               ├── proc-macro2 v1.0.107 (*)
│               ├── quote v1.0.47 (*)
│               └── unicode-ident v1.0.26
├── futures v0.3.34
│   ├── futures-channel v0.3.34
│   │   ├── futures-core v0.3.34
│   │   └── futures-sink v0.3.34
│   ├── futures-core v0.3.34
│   ├── futures-executor v0.3.34
│   │   ├── futures-core v0.3.34
│   │   ├── futures-task v0.3.34
│   │   └── futures-util v0.3.34
│   │       ├── futures-channel v0.3.34 (*)
│   │       ├── futures-core v0.3.34
│   │       ├── futures-io v0.3.34
│   │       ├── futures-macro v0.3.34 (proc-macro)
│   │       │   ├── proc-macro2 v1.0.107 (*)
│   │       │   ├── quote v1.0.47 (*)
│   │       │   └── syn v3.0.6 (*)
│   │       ├── futures-sink v0.3.34
│   │       ├── futures-task v0.3.34
│   │       ├── memchr v2.8.3
│   │       ├── pin-project-lite v0.2.17
│   │       └── slab v0.4.12
│   ├── futures-io v0.3.34
│   ├── futures-sink v0.3.34
│   ├── futures-task v0.3.34
│   └── futures-util v0.3.34 (*)
├── indexmap v2.14.2
│   ├── equivalent v1.0.2
│   ├── hashbrown v0.17.1
│   │   └── foldhash v0.2.0
│   └── serde_core v1.0.229
├── pastey v0.2.3 (proc-macro)
├── pin-project-lite v0.2.17
├── rmcp-macros v3.4.1 (proc-macro)
│   ├── darling v0.24.1
│   │   ├── darling_core v0.24.1
│   │   │   ├── ident_case v1.0.1
│   │   │   ├── proc-macro2 v1.0.107 (*)
│   │   │   ├── quote v1.0.47 (*)
│   │   │   ├── strsim v0.11.1
│   │   │   └── syn v3.0.6 (*)
│   │   └── darling_macro v0.24.1 (proc-macro)
│   │       ├── darling_core v0.24.1 (*)
│   │       ├── quote v1.0.47 (*)
│   │       └── syn v3.0.6 (*)
│   ├── proc-macro2 v1.0.107 (*)
│   ├── quote v1.0.47 (*)
│   ├── serde_json v1.0.151
│   │   ├── itoa v1.0.18
│   │   ├── memchr v2.8.3
│   │   ├── serde_core v1.0.229
│   │   └── zmij v1.0.23
│   └── syn v3.0.6 (*)
├── schemars v1.2.2
│   ├── chrono v0.4.45 (*)
│   ├── dyn-clone v1.0.20
│   ├── ref-cast v1.0.27
│   │   └── ref-cast-impl v1.0.27 (proc-macro)
│   │       ├── proc-macro2 v1.0.107 (*)
│   │       ├── quote v1.0.47 (*)
│   │       └── syn v3.0.6 (*)
│   ├── schemars_derive v1.2.2 (proc-macro)
│   │   ├── proc-macro2 v1.0.107 (*)
│   │   ├── quote v1.0.47 (*)
│   │   ├── serde_derive_internals v0.30.0
│   │   │   ├── proc-macro2 v1.0.107 (*)
│   │   │   ├── quote v1.0.47 (*)
│   │   │   └── syn v3.0.6 (*)
│   │   └── syn v3.0.6 (*)
│   ├── serde v1.0.229 (*)
│   └── serde_json v1.0.151
│       ├── itoa v1.0.18
│       ├── memchr v2.8.3
│       ├── serde_core v1.0.229
│       └── zmij v1.0.23
├── serde v1.0.229 (*)
├── serde_json v1.0.151 (*)
├── thiserror v2.0.21
│   └── thiserror-impl v2.0.21 (proc-macro)
│       ├── proc-macro2 v1.0.107 (*)
│       ├── quote v1.0.47 (*)
│       └── syn v3.0.6 (*)
├── tokio v1.53.1
│   ├── bytes v1.12.1
│   ├── libc v0.2.189
│   ├── mio v1.2.3
│   │   └── libc v0.2.189
│   ├── pin-project-lite v0.2.17
│   ├── socket2 v0.6.5
│   │   └── libc v0.2.189
│   └── tokio-macros v2.7.2 (proc-macro)
│       ├── proc-macro2 v1.0.107 (*)
│       ├── quote v1.0.47 (*)
│       └── syn v3.0.6 (*)
├── tokio-util v0.7.19
│   ├── bytes v1.12.1
│   ├── futures-core v0.3.34
│   ├── futures-sink v0.3.34
│   ├── libc v0.2.189
│   ├── pin-project-lite v0.2.17
│   └── tokio v1.53.1 (*)
├── tracing v0.1.44
│   ├── pin-project-lite v0.2.17
│   ├── tracing-attributes v0.1.31 (proc-macro)
│   │   ├── proc-macro2 v1.0.107 (*)
│   │   ├── quote v1.0.47 (*)
│   │   └── syn v2.0.119
│   │       ├── proc-macro2 v1.0.107 (*)
│   │       ├── quote v1.0.47 (*)
│   │       └── unicode-ident v1.0.26
│   └── tracing-core v0.1.36
│       └── once_cell v1.21.4
└── uuid v1.26.1
    └── getrandom v0.4.3
        ├── cfg-if v1.0.5
        └── libc v0.2.189
```

</details>

## Alternatives considered

- **Streamable HTTP now.** Unneeded for a local subprocess client; defer the additional
  transport dependencies and listener configuration until a remote client needs them.
- **A separate MCP query implementation or structured JSON helper.** Duplicates the read
  contract or introduces another serialization path; the shared DTO's text keeps parity
  directly testable.
- **Other Rust MCP SDKs.** Research 0003 §5 compares `rust-mcp-sdk`, `tower-mcp`,
  `poem-mcpserver`, and dormant SDKs. rmcp fits the existing Tokio runtime and current spec
  while providing the read-only annotations required here.

## Revisit when

The live bridge (New A: event-log → `WorldEvent`) lands. The CLI deliberately starts with
an empty `Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP)` and has no log-directory flag.
`QueryState` is already cheaply `Clone`-able through shared state, so `serve` and `mcp`
can use one live-updating instance without another state refactor.

A remote MCP client needs Streamable HTTP (add it behind a feature), or an rmcp upgrade
requires a reviewed pin change. Branches and additional read contracts follow decision
0006 rather than inventing MCP-only behavior.
