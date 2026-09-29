# s2w-app

Runtime wiring: composes sources, the log, the core and the engines; read-only MCP; local web view.

## Allowed dependencies

- every `s2w-*` library crate; `tokio`, `tokio-stream`, `thiserror`; further runtime, MCP and
  HTTP libraries chosen in decision records
- `postcard` (no default features, `use-std`) for the world snapshot payload only (decision 0024)
- dev: `proptest` for the snapshot golden-equivalence test
- `memory-serve` at runtime and build time for the committed web bundle (decision 0016);
  Node is a frontend development/CI tool only, never part of a Rust build or runtime
- `rustix` (feature `fs`) for `statfs` in `status` (s2w#32): filesystem kind and free space
- dev only: `gungraun` (the `scale_ir` instruction-count benchmark, pinned exactly because
  `gungraun-runner` must match it) and `dhat` (the `scale_mem` heap test), s2w#32

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
  timeline (`Bridge::resume` after a snapshot restore: one whose head is past its base). It commits each batch's verdicts to the verdict store before serving any of its
  claims, and serves a stored verdict instead of calling the engine (decision 0012); a verdict
  that does not match the log is an error, never a re-evaluation. The one other write is
  `publish_source_stats`, a read-only telemetry side channel that never touches the timeline.
- `routes` (decision 0023) turns accepted `stream-mapping` proposals into routes. `resolve` is
  pure; `load` opens the proposal store read-only. `serve` builds its registry from it before
  snapshot restore, because the feed fingerprint depends on the routes. The world manifest's
  engine list is historical (the defaults at creation), not the live registry. Replay reads
  stored verdicts of registered engine names only (`VerdictStore::read_range_of`).
- `Timeline` (decision 0024) has a base world (empty, or a restored snapshot's) and serves offsets
  from the base to the head only; anything below is `offset_before_base` (410). Index the event
  list only through `events_after`, never by absolute offset. Without a snapshot the base is 0 and
  every answer is unchanged. Base and head share one world (`Arc`) until the first append after a
  restore copies it (#179); never hand out the `Arc` itself, or the next append copies the whole
  world and the old one stays alive.
  `QueryState::replace_timeline` is the one way to install a restored
  timeline, and only `serve`'s startup restore calls it, before the bridge exists.
- `snapshot/` (decision 0024): a snapshot is derived and never trusted. It is loaded only when
  every validity rule holds, and an invalid file is reported and skipped, never deleted. Its
  bytes carry no path or host detail. The codec and validity rules are pure; only `store` does
  I/O, writes atomically (temp file, fsync, rename, dir fsync), and touches only files matching
  `snapshot-<20 digits>.s2w`.
- `serve/snapshots.rs` (decision 0024, part 1b) is the only snapshot writer: it captures the head
  right after a successful `poll_once` with no `.await` in between, so the bridge's `mark()` and
  the timeline head describe the same moment, refuses a head that moved past that checkpoint,
  encodes the borrowed head under the read lock without cloning it (#179), and writes and fsyncs
  the bytes on its own thread. The final snapshot runs only in the stop-signal
  branch, before the bridge is dropped, never after a fatal error.
- `query/` is the one read contract for the view, `--json` and MCP (decision 0006). Its pure half does no
  I/O; the HTTP half only parses parameters and calls it. One SSE message per offset; stable error codes.
- No domain knowledge in this crate; see decision 0018.
- `status` (s2w#32) owns the storage figures on `watch`'s human progress line: the store's
  size (every `*.sqlite3*` file in the log directory, labelled `store`), and days until its
  disk is full. A figure measured on tmpfs or ramfs says
  `NOT A DISK NUMBER` and never prints days; a `statfs` failure, a non-Linux host and an
  overlay are all `Unknown`, never `Disk`. The `--json` reporter's output does not change.

## Scale measurements (s2w#32)

- `tests/support/scale_generator.rs` is the one seeded synthetic event generator, included by
  `#[path]` from each user below. Change its constants or distributions and every baseline
  moves; say so in the PR.
- `benches/scale_ir.rs`: gungraun library benchmark `fold_ir_per_event` (total instructions for
  folding `IR_EVENTS` events; setup not counted). Needs Valgrind and `gungraun-runner` at the
  same version as the `gungraun` pin. The summary lands in
  `target/gungraun/s2w-app/scale_ir/scale/fold_ir_per_event.events/summary.json`.
- `tests/scale_mem.rs`: `#[ignore]`d; owns a dhat global allocator; prints one JSON line with
  bytes per entity (gated) and bytes per relationship (reported).
- `benches/scale_wall.rs`: appends, one event per transaction, to a log under
  `$CARGO_TARGET_TMPDIR/scale/`; prints one JSON line naming the filesystem, plus a `warning`
  field (`status::TMPFS_WARNING`) on tmpfs, which `cargo xtask scale` prints as is. Must run;
  its value is reported, never gated.

## Web bundle

- `web/dist/` is committed and CI's `bundle` job fails if it differs from a clean build. Any change
  under `crates/s2w-app/web` (source, CSS, `index.html`, lockfile) must run `npm ci && npm run build`
  in that directory and commit the regenerated `web/dist/` in the same commit. Also run
  `npm run typecheck` and `npm test`; CI runs both.
