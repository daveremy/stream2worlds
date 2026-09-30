# s2w-app

Runtime wiring: composes sources, the log, the core and the engines; read-only MCP; local web view.

## Allowed dependencies

- every `s2w-*` library crate, `s2w-discover` included (the learned-mapping producer); `tokio`, `tokio-stream`, `thiserror`; further runtime, MCP and
  HTTP libraries chosen in decision records
- `postcard` (no default features, `use-std`) for the world snapshot payload only (decision 0024)
- dev: `proptest` for the snapshot golden-equivalence test
- `memory-serve` at runtime and build time for the committed web bundle (decision 0016);
  Node is a frontend development/CI tool only, never part of a Rust build or runtime
- `rustix` (feature `fs`) for `statfs` in `status` (s2w#32): filesystem kind and free space
- dev only: `gungraun` (the `scale_ir` instruction-count benchmark, pinned exactly because
  `gungraun-runner` must match it) and `dhat` (the `scale_mem` heap test), s2w#32
- dev only: `mimalloc`, the global allocator of the `backfill_memory_mimalloc` measurement
  target (s2w#220)

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Module layering (s2w#240)

`query` is the bottom of this crate: it depends on no sibling module (one pending exception:
`query::http` still uses `bridge::SourceStats` until s2w#240 PR 2 moves it). `bridge`, `proposals`,
`routes`, `snapshot`, `discover` depend on `query` (and on each other downward only); `serve` and
`mcp` are the top and compose everything. Check 15 (`cargo xtask check`, module cycles) reports
violations; it is report-only until s2w#240 flips `ENFORCE`.

## Invariants

- The only crate that composes layers. Business rules live in the core, not here.
- MCP is read-only; capabilities never expand from a good track record. The one exception is the
  opt-in `decision_record` tool (`s2w mcp --allow-decisions`, decision 0020): it appends a decision
  with the `agent` decider only, never edits a proposal, and is absent from the default tool list.
- `proposals` (#185) is the one proposal read and decision-write service: MCP `decision_record`
  (the `agent` seat) and the `s2w proposals decide` CLI (the `human` seat, basis stored as
  `reviewer=<id>; <basis>`) both call `record_decision`. An unknown id or missing store never
  creates the store; an accept on an undecodable `stream-mapping` envelope is refused, and so is
  an accept on a `dashboard-manifest` row whose envelope does not decode or whose manifest is
  null (decision 0029). It is also the one human proposal-write service (#309):
  `record_mapping_proposal` (the `s2w proposals propose` CLI) appends a `stream-mapping`
  proposal with a `human` actor and decides nothing. Unlike a decision, it creates the store on
  a fresh log dir, and only after the author, source and mapping validate; it stores only a
  payload `decode_envelope` accepts. Its id is `routes::proposal_id` (shared with `discover`,
  window = position 1), so a re-run is an identical retry. It deliberately skips discover's
  same-(source, identity) lookup: a human row beside a producer's is harmless, since routing
  runs the earliest accepted proposal of an identity.
- Local by default: nothing leaves the machine without an approved export manifest.
- A stored cursor beats `--since`: passing both is a usage error, never a silent ignore, and a
  cursor that cannot be decoded is a loud error, never a fresh start.
- Full `/world` (`lod=entity`) projections are single-flight (`query/generation.rs`, s2w#270): one
  build per `QueryState`, requests grouped by (epoch, `at`, `lod`, `focus`, `hops`), a request never
  joins a build under way, a full queue answers 503 `world_queue_full`. Never build a full view
  outside the queue: each holds about 330 MiB.
- The System 1 bridge (`bridge/`, decision 0011) writes to `QueryState`'s timeline only
  through `append`, resumes from a log position (never a fold offset), and refuses a non-empty
  timeline (`Bridge::resume` after a snapshot restore: one whose head is past its base). It commits each batch's verdicts to the verdict store before serving any of its
  claims, and serves a stored verdict instead of calling the engine (decision 0012); a verdict
  that does not match the log is an error, never a re-evaluation. The one other write is
  `publish_source_stats`, a read-only telemetry side channel that never touches the timeline.
- `query::resolve_class` is decision 0023's resolution rule, once, generic over a proposal class
  and the scope key its decoder returns. `routes::resolve` (per source) and the dashboard read
  (`query::dashboard`, per world, decision 0029) both call it; never copy the rule. The
  `stream-mapping` envelope lives in `query::stream_mapping` and `routes` re-exports it, so
  `query` stays the bottom of the crate.
- `routes` (decision 0023) turns accepted `stream-mapping` proposals into routes. `resolve` is
  pure; `load` opens the proposal store read-only. `serve` builds its registry from it before
  snapshot restore, because the feed fingerprint depends on the routes. The world manifest's
  engine list is historical (the defaults at creation), not the live registry. Replay reads
  stored verdicts of registered engine names only (`VerdictStore::read_range_of`).
- `dashboard` (decision 0029, s2w#301) is the dashboard-manifest filer and the other `policy`
  decider (`dashboard-auto-apply/1`). It builds the input from the log tail
  (`LogReader::read_head` plus a doubling window), asks a `ManifestProposer` outside the
  writer lock, re-plans under the lock, and never files twice for one (world, input hash,
  actor): a manifest row stops it, and so do `MAX_ATTEMPTS` null-manifest rows. It runs only
  from `s2w dashboard propose`, never from `serve`.
- `discover` (decision 0025) is the learned-mapping producer and a `policy` decider.
  It runs between `serve`'s two route resolutions, profiles only member sources with no
  effective mapping, writes nothing when a `stream-mapping` proposal for the same (source,
  identity) exists from any actor (looked up under the writer lock), mints the proposal id from
  (actor, source, window, identity) with `routes::proposal_id`, opens the proposal writer per
  run and drops it (never held by `serve`), and turns every failure into a `discover:` note, never an error.
  `discover::in_run` (#197 PR 4b) runs it once per source from the bridge loop, for a source
  unrouted at start that reaches the window mid-run; it notes through `Reporter::note_sink`,
  never changes routes itself (the live-rebuild watcher sees the store move), and keeps a source
  pending only while the store is locked, retrying every `LOCK_RETRY_POLLS` polls.
  A window this actor already filed and someone decided is skipped before profiling.
- `Timeline` (decisions 0024, 0026) holds one world, the head, and at most `history_cap` recent
  events (`DEFAULT_HISTORY_CAP`), each with the `Delta` its fold produced; past the cap it drops
  the oldest down to half. Never add a second resident world (a base, or one per SSE follower):
  the head alone is most of the memory budget. World queries serve from `base()` (0 while every
  event since offset 0 is retained, else the head); `/events` replays from `replay_base()` using
  the stored deltas (`?last=N` replays the latest N through the head and clamps to it, so it never
  answers `offset_before_base`); anything below is `offset_before_base` (410). Index the event list only
  through `events_after`, never by absolute offset. Never hand out the head `Arc` itself, or the
  next append copies the whole world and the old one stays alive.
  `QueryState::replace_timeline` is the one way to install a timeline. `serve`'s startup
  (`snapshots::prepare`) calls it before the bridge exists, and a live rebuild
  (`serve/rebuild.rs`, decision 0023 "Rebuild") calls it between two polls, before
  `Bridge::restart`; nothing else does. A timeline carries its
  `Epoch` (decision 0023): `prepare` installs the registry's feed fingerprint on the empty
  timeline first, and a restored one chains `.with_epoch`. Every read that resolves a client
  offset calls `Timeline::check_epoch` under the same lock, before any bounds check.
- `snapshot/` (decision 0024): a snapshot is derived and never trusted. It is loaded only when
  every validity rule holds, and an invalid file is reported and skipped, never deleted. Its
  bytes carry no path or host detail. The codec and validity rules are pure; only `store` does
  I/O, writes atomically (temp file, fsync, rename, dir fsync), and touches only files matching
  `snapshot-<16 hex>-<20 digits>.s2w` (feed fingerprint, offset) of the serving fingerprint.
- `serve/snapshots.rs` (decision 0024, part 1b) is the only snapshot writer: it captures the head
  right after a successful `poll_once` with no `.await` in between, so the bridge's `mark()` and
  the timeline head describe the same moment, refuses a head that moved past that checkpoint,
  encodes the borrowed head under the read lock without cloning it (#179), and writes and fsyncs
  the bytes on its own thread. The final snapshot runs only in the stop-signal
  branch, before the bridge is dropped, never after a fatal error.
- `serve/rebuild.rs` (decision 0023 "Rebuild") is the only code that changes the registry
  while serving. `RouteWatcher` reads the proposal store's watermark, then its rows (never the
  other order) after a poll, at most every 250 ms; a changed feed fingerprint swaps the world with no
  `.await`: retire the snapshot writer, `replace_timeline` under the new epoch, `prepare`,
  `Bridge::restart`.
- `query/` is the one read contract for the view, `--json` and MCP (decision 0006). Its pure half does no
  I/O; the HTTP half only parses parameters and calls it. One SSE message per offset; stable error codes.
- No domain knowledge in this crate; see decision 0018.
- `status` (s2w#32) owns the storage figures on `watch`'s human progress line: the store's
  size (every `*.sqlite3*` file in the log directory, labelled `store`), and days until its
  disk is full. A figure measured on tmpfs or ramfs says
  `NOT A DISK NUMBER` and never prints days; a `statfs` failure, a non-Linux host and an
  overlay are all `Unknown`, never `Disk`. The `--json` reporter's output does not change.
- `lag` (s2w#168) renders the started source's `Watermarks` on the same human progress line, for
  `watch` and `serve`: `lag p0 12, p1 0`, `?` for a position not known yet, `lag not reported`
  for a source with no watermark (never 0); past 8 partitions the furthest behind print, then
  `+N more`. The `--json` reporter's output does not change.

## Scale measurements (s2w#32, s2w#174)

Each scale number is measured on two event supplies side by side: the seeded generator and a
recorded stream.

- `tests/support/scale_generator.rs` is the one seeded synthetic event generator, included by
  `#[path]` from each user below. Change its constants or distributions and every baseline
  moves; say so in the PR.
- `tests/fixtures/recorded-10min.raw.sse` is the recorded supply: 10 minutes of raw SSE, 11,667
  events, 34.2 MB, human-owned (provenance and licence in `tests/fixtures/README.md`; never
  regenerate or edit it to make a gate pass). `tests/support/recorded.rs` loads it once
  (`OnceLock`), refuses bytes whose FNV-1a 64 is not `FIXTURE_HASH`, and maps it with
  `tests/fixtures/recorded.mapping.json` (a symlink to check 11's `sample.mapping.json`).
  `tests/recorded_fixture.rs` checks its bytes and counts (58,335 claims; `ENTITIES` 11,462 and
  `RELATIONSHIPS` 19,512 after the fold) and that replay is deterministic. A re-recording
  re-pins `FIXTURE_HASH`, those counts, the linked-mapping counts in
  `tests/support/recorded_links.rs`, and `[recorded]`, `[ir.recorded] events`, `[parse] events`
  and the `[memory.recorded]` counts in `xtask/scale-baseline.toml` in the same PR.
- `tests/support/recorded_links.rs` (shared by `#[path]` next to `recorded.rs`) loads
  `tests/fixtures/recorded-links.mapping.json` (a symlink to `s2w-system1/testdata/sample-links.mapping.json`,
  the engine's own link-merge fixture) and pins what it makes of the recorded fixture: 81,669
  claims, 11,667 link merges, 0 abstentions. `tests/recorded_fixture.rs` checks the pins on every
  `cargo test`; the parse benchmark's teardown asserts the same ones.
- `benches/scale_ir.rs`: gungraun library benchmark `fold_ir_per_event` (total instructions for
  folding `IR_EVENTS` events; setup not counted). Needs Valgrind and `gungraun-runner` at the
  same version as the `gungraun` pin. The summary lands in
  `target/gungraun/s2w-app/scale_ir/scale/fold_ir_per_event.events/summary.json`. The second
  benchmark, `fold_ir_per_event_recorded` (id `fixture`), folds every claim of the recorded
  fixture in emission order (loading and mapping not counted) and asserts the pinned entities
  and relationships in teardown; its summary is `scale/fold_ir_per_event_recorded.fixture/summary.json`
  under the same directory. Its Ir is divided by raw events, not claims. The third,
  `parse_ir_per_event` (id `fixture`, s2w#166), measures System 1's parse: one
  `MappingEngine::evaluate` per raw event of the recorded fixture with the linked mapping
  (loading the fixture and building the engine not counted), asserting the pinned claim, merge
  and abstention counts in teardown; summary `scale/parse_ir_per_event.fixture/summary.json`. Its
  body is a plain loop, never a closure: gungraun toggles collection on entering any symbol
  under the benchmark function's name, so a closure's callees go uncounted.
- `tests/scale_mem.rs`: `#[ignore]`d; owns a dhat global allocator; each test prints one JSON
  line with bytes per entity (gated) and bytes per relationship (reported):
  `bytes_per_entity_and_relationship` on the generator,
  `bytes_per_entity_and_relationship_recorded` on the fixture.
- `benches/scale_wall.rs`: appends, one event per transaction, to a log under
  `$CARGO_TARGET_TMPDIR/scale/`; prints one JSON line naming the filesystem, plus a `warning`
  field (`status::TMPFS_WARNING`) on tmpfs, which `cargo xtask scale` prints as is. Must run;
  its value is reported, never gated.

## Web bundle

- `web/dist/` is committed and CI's `bundle` job fails if it differs from a clean build. Any change
  under `crates/s2w-app/web` (source, CSS, `index.html`, lockfile) must run `npm ci && npm run build`
  in that directory and commit the regenerated `web/dist/` in the same commit. Also run
  `npm run typecheck` and `npm test`; CI runs both.
