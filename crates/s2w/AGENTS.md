# s2w

The `s2w` command-line tool.

## Allowed dependencies

- `s2w-app` only

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Every command is non-interactive and idempotent. Every one-shot command has `--json`.
  `watch --json` (#79) streams NDJSON progress instead of the human eprintln lines. `serve` and
  `mcp` accept prefix-only `--json` (`s2w --json serve` / `s2w --json mcp`) for structured
  stderr notes/errors, plus startup/usage error rendering (mcp); `serve` also streams the same
  per-flush NDJSON progress lines as `watch --json`, on stdout, while ingesting — `mcp` has no
  progress stream because stdout is reserved for JSON-RPC once serving.
- `serve --snapshot-every <n>` (n > 0) and `--no-snapshot` (decision 0021) are mutually
  exclusive; passing both is a usage error. `serve` stops on SIGINT or SIGTERM.
- Errors say what to try next.
- No logic here beyond argument parsing and output formatting.
- No domain knowledge in this crate; see decision 0018.
