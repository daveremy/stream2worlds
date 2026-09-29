# s2w-app

Runtime wiring: composes sources, the log, the core and the engines; read-only MCP; local web view.

## Allowed dependencies

- every `s2w-*` library crate; `tokio`, `tokio-stream`, `thiserror`; further runtime, MCP and
  HTTP libraries chosen in decision records
- `postcard` (no default features, `use-std`) for the world snapshot payload only (decision 0021)
- dev: `proptest` for the snapshot golden-equivalence test
- `memory-serve` at runtime and build time for the committed web bundle (decision 0016);
  Node is a frontend development/CI tool only, never part of a Rust build or runtime

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- The only crate that composes layers. Business rules live in the core, not here.
- MCP is read-only; capabilities never expand from a good track record. The one exception is the
  opt-in `decision_record` tool (`s2w mcp --allow-decisions`, decision 0020): it appends a decision
  with the `agent` decider only, never edits a proposal, and is absent from the default tool list.
- Local by default: nothing leaves the machine without an approved export manifest.
- A stored cursor beats `--since`: passing both is a usage error, never a silent ignore, and a
  cursor that cannot be decoded is a loud error, never a fresh start.
- The System 1 bridge (`bridge/`, decision 0011) writes to `QueryState`'s timeline only
  through `append`, resumes from a log position (never a fold offset), and refuses a non-empty
  timeline. It commits each batch's verdicts to the verdict store before serving any of its
  claims, and serves a stored verdict instead of calling the engine (decision 0012); a verdict
  that does not match the log is an error, never a re-evaluation. The one other write is
  `publish_source_stats`, a read-only telemetry side channel that never touches the timeline.
- `Timeline` (decision 0021) has a base world (empty, or a restored snapshot's) and serves offsets
  from the base to the head only; anything below is `offset_before_base` (410). Index the event
  list only through `events_after`, never by absolute offset. Without a snapshot the base is 0 and
  every answer is unchanged.
- `snapshot/` (decision 0021): a snapshot is derived and never trusted. It is loaded only when
  every validity rule holds, and an invalid file is reported and skipped, never deleted. Its
  bytes carry no path or host detail. The codec and validity rules are pure; only `store` does
  I/O, writes atomically (temp file, fsync, rename, dir fsync), and touches only files matching
  `snapshot-<20 digits>.s2w`. Writer, trigger and serve wiring land in #33 part 1b.
- `query/` is the one read contract for the view, `--json` and MCP (decision 0006). Its pure half does no
  I/O; the HTTP half only parses parameters and calls it. One SSE message per offset; stable error codes.
- No domain knowledge in this crate; see decision 0018.

## Web bundle

- `web/dist/` is committed and CI's `bundle` job fails if it differs from a clean build. Any change
  under `crates/s2w-app/web` (source, CSS, `index.html`, lockfile) must run `npm ci && npm run build`
  in that directory and commit the regenerated `web/dist/` in the same commit. Also run
  `npm run typecheck` and `npm test`; CI runs both.
