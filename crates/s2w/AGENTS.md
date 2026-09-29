# s2w

The `s2w` command-line tool.

## Allowed dependencies

- `s2w-app`, `s2w-log`, `s2w-model`, `serde_json`

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Every command is non-interactive and idempotent. Every one-shot command has `--json`.
  `watch --json` (#79) streams NDJSON progress instead of the human eprintln lines. `serve` and
  `mcp` accept prefix-only `--json` (`s2w --json serve` / `s2w --json mcp`) for structured
  stderr notes/errors, plus startup/usage error rendering (mcp); `serve` also streams the same
  per-flush NDJSON progress lines as `watch --json`, on stdout, while ingesting — `mcp` has no
  progress stream because stdout is reserved for JSON-RPC once serving.
- `proposals list|grade|decide` (#185) only parses and renders: every read and write goes through
  `s2w_app::proposals`, the service MCP `decision_record` also uses. Reads never create the
  store; `decide` needs an explicit `--log-dir` and writes only the `human` decider. Data errors
  exit 1 with the HTTP/MCP `{"error", "message"}` body; usage errors exit 2.
- `serve --snapshot-every <n>` (n > 0) and `--no-snapshot` (decision 0024) are mutually
  exclusive; passing both is a usage error. `serve` stops on SIGINT or SIGTERM.
- Errors say what to try next.
- No logic here beyond argument parsing and output formatting.
- No domain knowledge in this crate; see decision 0018.
