# s2w

The `s2w` command-line tool.

## Allowed dependencies

- `s2w-app` only

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Every command is non-interactive and idempotent. Every one-shot command has `--json`.
  Exception (2026-09-27, #39): `watch` is streaming, so its NDJSON progress design is deferred
  to #79; `mcp` is already JSON-RPC over stdio; `serve` is a long-running HTTP server without `--json`.
- Errors say what to try next.
- No logic here beyond argument parsing and output formatting.
