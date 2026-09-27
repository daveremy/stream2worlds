# Report — stream2worlds #6, Leg A: Wikipedia EventStreams SSE source

## Provenance note

An earlier run on this branch wrote nearly all of the implementation (wikipedia.rs, both
fixtures, the decision record, and the Cargo.toml / lib.rs / AGENTS.md / allowlist.toml /
README.md edits) but hit a tooling usage limit before building anything and left an empty
report. This session verified that implementation line-by-line against the brief's plan,
applied `cargo fmt` (the only source change needed), and ran every gate for the first time
(Cargo.lock gained the reqwest/tokio tree, ~1700 lines). No redesign was made; where the
prior run's code is what the plan specifies, it was kept as-is.

## Files created / changed

- `crates/s2w-sources/src/wikipedia.rs` (new) — `WikipediaSource` (`Stream` via
  `tokio_stream::wrappers::ReceiverStream` around a bounded 64-slot mpsc channel fed by an
  owned `tokio::spawn` task; `Drop` aborts its `AbortHandle`), `LastEventId` /
  `StreamPosition` / `PositionAt`, `WikipediaSourceError` (thiserror), hand-rolled SSE
  frame parser (`id:`/`data:`/comment/blank-line dispatch, LF/CRLF/bare-CR line endings,
  chunk-boundary-safe), internal `Connect` trait + `ReqwestConnect` (rustls, descriptive
  `User-Agent`), capped exponential backoff (250 ms → 30 s), canary + `examplewiki`
  filtering after cursor advance, malformed-id poison-frame cap (3 consecutive → fresh
  connection with last good cursor), and 12 inline `#[cfg(test)]` tests.
- `crates/s2w-sources/src/lib.rs` — `pub mod wikipedia;` + crate-level `SourceEvent`
  (`payload` raw `data:` JSON text, `cursor: LastEventId`, `received_at: Timestamp`).
- `crates/s2w-sources/Cargo.toml` — deps `reqwest 0.13` (`default-features = false`,
  `["rustls", "stream"]`), `tokio` (`macros`, `rt`, `sync`, `time`), `tokio-stream`,
  `serde_json`, `thiserror` (both workspace-inherited). No test-only deps.
- `crates/s2w-sources/testdata/wikipedia-page-change.raw.sse` (pre-captured, untouched) —
  4.6 MB / 1615 real frames with curl command + UTC capture time in leading comment lines.
- `crates/s2w-sources/testdata/wikipedia-malformed.synthetic.sse` (new) — clearly-named
  synthetic frames: `id: not-json`, canary (`meta.domain == "canary"`), `examplewiki`
  (`database == "examplewiki"`), and a final frame cut mid-`data:` with no blank line.
- `docs/decisions/0002-wikipedia-sse-client.md` (new) — hand-rolled loop on reqwest 0.13;
  all four 0003 §2 alternatives dispositioned; each of 0003's four open-item bullets given
  its own explicit "N/A to this choice" line; fixture-recording question recorded as its
  own resolved open item; revisit-when clause.
- `crates/s2w-sources/AGENTS.md` — chosen deps named; User-Agent + canary/examplewiki
  filter-after-cursor-advance invariants added.
- `xtask/allowlist.toml` — `s2w-sources` normal deps; `[external.reqwest/tokio/tokio-stream]`
  → "Sources" row; `[external.serde_json]` already existed (checked, no duplicate).
- `README.md` — Sources row now carries `reqwest`, `tokio`, `tokio-stream` backticked tokens.
- `Cargo.lock` — reqwest/tokio tree locked on first build.

## Design decisions vs the plan

All as specified, with these choice-points resolved as follows:

- Tests are inline `#[cfg(test)]` in `wikipedia.rs` (plan allowed "decide by size"; the fake
  `Connect` and helpers are ~400 lines tightly coupled to the module's private items).
- reqwest feature is `rustls` — 0.13.5's actual name, verified by building; the plan flagged
  that `rustls-tls` (0.12's name) must not be assumed.
- The `has_cursor: bool` state machine is realized as `cursor: Option<LastEventId>` — same
  semantics: `since` sent verbatim on every connect while the cursor is `None`, then
  `Last-Event-ID` only, never both.
- Cursor advance exactly per the round-1 rule: only complete blank-line-terminated frames
  with a parseable id advance it; canary/examplewiki frames advance-then-drop; a frame with
  unparseable id is surfaced as `InvalidLastEventId`, drops its event, does not advance.
- The `ReqwestConnect` construction test asserts against a built-but-not-executed
  `reqwest::Request` (exact `?since=` URL with percent-encoding, `Last-Event-ID` header
  presence/absence/value, `User-Agent`), the plan's "less code" option.
- Two details the plan left open, resolved conservatively: a data-only frame with no `id:`
  field surfaces as `InvalidLastEventId` (Wikimedia always sends ids; silence would risk a
  mis-cursored resume), and backoff resets on every successfully parsed frame (any delivered
  frame proves the connection works).

## Gates (exact commands, in order)

```
export CARGO_TARGET_DIR=~/.cache/cargo-target/stream2worlds/1c685dfc
export CARGO_BUILD_JOBS=6
export PATH=~/.cargo/bin:$PATH
cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo xtask check
```

- `cargo fmt --check` — PASS (after one `cargo fmt` application to two formatting slips in
  wikipedia.rs left by the interrupted run)
- `cargo clippy --workspace --all-targets -- -D warnings` — PASS (25 s first-compile; no
  `unwrap()`/`expect()` anywhere; every public item documented)
- `cargo test --workspace` — PASS: 12 wikipedia tests (incl. the 1615-frame recorded
  fixture replayed exactly once across a cursor-checked reconnect split, canary and
  examplewiki filters, since-retention through a 503 + empty connect, partial-frame discard,
  4-poison-frame run forcing a reconnect on last good cursor, CRLF split across chunks,
  bare-CR endings, status+transport retry, reqwest request shape, drop-abort during idle
  read and during backoff sleep) + 4 s2w-model + 2 xtask tests.
- `cargo xtask check` — PASS ("10 crates, 7 external dependencies").

## Open questions / risks the plan didn't anticipate

- `WikipediaSource::new` builds the reqwest client with no explicit connect timeout. A read
  timeout would be wrong for SSE, but a hung *connect* currently only recovers via reqwest
  internals; consider `Client::builder().connect_timeout(...)` when this is wired into the
  CLI (next leg).
- The fixture-replay oracle (`fixture_frames` in tests) is a deliberately naive second
  parser; it is a test-only comparison, never a regeneration path for the fixture bytes.
- reqwest 0.13.5 pulled a large transitive tree into Cargo.lock (hyper, rustls, icu/idna
  via url) — expected, but the first CI build on a cold cache will be slow.
- `codex-work/` (report, run logs) is left untracked on purpose: no branch has ever
  committed it and `*.log` is gitignored.
