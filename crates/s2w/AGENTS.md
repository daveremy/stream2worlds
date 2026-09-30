# s2w

The `s2w` command-line tool.

## Allowed dependencies

- `s2w-app`, `s2w-log`, `s2w-model`, `serde_json`
- `mimalloc`, the binary's global allocator (decision 0031)

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- This binary is the only place a global allocator is set (`main.rs`, mimalloc, decision 0031).
  No library crate sets one; test targets that set their own are measurement targets.
- Every command is non-interactive and idempotent. Every one-shot command has `--json`.
  `watch --json` (#79) streams NDJSON progress instead of the human eprintln lines. `serve` and
  `mcp` accept prefix-only `--json` (`s2w --json serve` / `s2w --json mcp`) for structured
  stderr notes/errors, plus startup/usage error rendering (mcp); `serve` also streams the same
  per-flush NDJSON progress lines as `watch --json`, on stdout, while ingesting — `mcp` has no
  progress stream because stdout is reserved for JSON-RPC once serving.
- `proposals list|grade|propose|decide` (#185) only parses and renders: every read and write goes through
  `s2w_app::proposals`, the service MCP `decision_record` also uses. Reads never create the
  store; `decide` needs an explicit `--log-dir` and writes only the `human` decider.
  `proposals propose` (#309) likewise needs an explicit `--log-dir`, reads the `--mapping` file
  and hands its bytes to `s2w_app::proposals::record_mapping_proposal`; it writes only a
  `human`-actor `stream-mapping` proposal and never a decision. Data errors
  exit 1 with the HTTP/MCP `{"error", "message"}` body; usage errors exit 2.
- `dashboard show` (decision 0029) only parses and renders `s2w_app::query::read_dashboard`;
  `--json` prints the query API's exact bytes. It never creates the proposal store.
- `dashboard propose` (s2w#301) only parses and renders `s2w_app::dashboard::propose_fallback`;
  it needs an explicit `--log-dir` because it writes. The proposer lives in `s2w-discover`,
  which this crate does not depend on: the entry point in `s2w-app` names it.
- `serve --snapshot-every <n>` (n > 0) and `--no-snapshot` (decision 0024) are mutually
  exclusive; passing both is a usage error. `serve` stops on SIGINT or SIGTERM.
- Errors say what to try next.
- No logic here beyond argument parsing and output formatting.
- No domain knowledge in this crate; see decision 0018.
