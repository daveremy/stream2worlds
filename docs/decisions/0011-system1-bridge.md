# 0011: System 1 bridge — engine contract, routing, and the live loop

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #51 · Amends [0005](0005-pure-fold.md)

## Decision

**One engine trait.** `s2w_system1::Engine` has `name()`, `version()` and
`evaluate(&RawEvent) -> Verdict`. `evaluate` is total: it never errors and must not panic.
A `Verdict` is `Propose { claims: Vec<WorldEvent>, confidence }` or `Abstain { reason }`.

- **Abstaining is a value.** `NotMine` (foreign schema, canary), `Unparseable`, `Insufficient`
  (a required field is missing), and `Panicked`, which only the bridge produces. `Propose` with
  no claims ("understood, nothing to say") is distinct from any abstention, and the bridge
  counts them separately.
- **Confidence is an integer**: basis points, `0..=10_000`, enforced on construction and on
  deserialization. The core bans floats from the world because float equality breaks
  byte-identical replay; a confidence threshold replayed across machines has the same problem.
- **Engines see only the payload.** They mint `NaturalKey`s and never see a `World`. That is
  why `WorldEvent`, `NaturalKey` and `AttrValue` move to `s2w-model` (0005's amendment).
- `s2w-system1` depends on `s2w-model`, `serde`, `serde_json` and `thiserror` only.

## Routing

Routing is app composition, in `s2w-app/src/bridge/registry.rs`. An `EngineRegistry` maps a
`Route` (`Exact("stdin")` or `Prefix("wikipedia.")`, with the separator included) to engines.
Every engine that matches a source runs, in registration order, so the timeline is a
deterministic function of the log and the registry. A source with no engine is counted and
logged once per bridge, never an error. Defaults: `wikipedia.*` → `wikimedia.page_change`,
`stdin` → `json_claims`.

> **2026-09-28 ([0021](0021-stream-mapping-v0.md)):** a data-driven `MappingEngine` now exists but
> is registered nowhere. In #163 PR 2, routes move from code to data: `serve` builds a route per
> source from the accepted stored mapping.

## The bridge

`Bridge<R: LogReader>` in `s2w-app/src/bridge/`. `LogReader` is a new read-only seam in
`s2w-log` with a blanket impl over every `EventLog`.

- **Poll, not notify.** SQLite has no cross-process notification. `poll_once` reads at most
  `batch` events after the last consumed position. `run` calls it on the blocking pool, backs
  off on empty polls (250 ms doubling to 2 s), and polls again at once after a full batch.
- **Resume from a log position, never a fold offset.** One raw event yields zero or more claims
  (0005: an offset is not a log position). The position is in memory only, so a new bridge
  replays the whole log, and `Bridge::new` refuses a timeline that already has events.
- **Time is receipt time.** Each claim is appended at the raw event's `received_at`, matching
  the timeline's existing semantics; a backwards timestamp is clamped and counted by the
  timeline (`/time`'s `clamped`).
- **The loop is total.** A panicking engine becomes `Abstain(Panicked)` via `catch_unwind`
  (this needs the default `panic = "unwind"`). A log error ends that poll; the next poll
  resumes after the last consumed event. Only an unavailable query state (a poisoned lock)
  stops the bridge.
- **`query/` is unchanged.** The bridge is a producer calling `QueryState::append`.

Wiring the bridge into a serving command is #10. A lockless reader that a second process can
open while the writer holds the log's lock is deferred until #10 decides whether ingest and
serve share a process.

## Verdict persistence: deferred, with its precondition written down

> **Superseded 2026-09-27 by [0012](0012-verdict-log.md) (#63):** verdicts are persisted and
> replayed, so the purity rule below no longer applies. The section stays as the record.

The system1 invariant "every verdict is persisted; replay never re-runs an engine" is **not
met yet**. The bridge re-evaluates every event on each start.

That is safe today only because both engines are pure functions of the payload: no clock, no
RNG, no model file. Re-evaluating then reproduces the same verdicts, so live and replay are the
same computation, and the only consumer is an in-memory timeline rebuilt at every start.

**Rule: an engine without a persisted verdict log must be a pure function of its payload.**
The type system does not enforce this; the registry does not check it. It stops holding when:

- an engine depends on an input the log does not capture, such as a model file (local
  embeddings, Jev). A pinned file is deterministic, but a swapped file silently changes what a
  restart serves;
- a world is persisted (#33);
- an existing engine's `version()` is bumped, which changes what a restart serves with nothing
  to detect it.

The shape is locked now so persistence is a table, not a redesign: `Verdict` serializes with
stable names (golden file `crates/s2w-system1/testdata/page-change-sample.verdict.json`), every
engine carries a name and version, and every verdict is produced by one function,
`bridge::evaluate_stored`, returning `VerdictRecord { position, engine, version, verdict }`.
Persisting is one write at that site. [#63](https://github.com/daveremy/stream2worlds/issues/63) tracks it.

## The second engine

The first slice promised "rules and local embeddings". The second engine shipped here is
`json_claims` instead: a payload that already is one `WorldEvent`, optionally wrapped with a
graded confidence. Two reasons:

1. Local embeddings depend on a model file the log does not capture, so under the rule above
   they need persisted verdicts first. Building both in #51 would have more than doubled it.
2. Two real implementations are what make the trait reviewable, and `json_claims` exercises
   the graded half of `Confidence` before embeddings exist.

Local embeddings are [#64](https://github.com/daveremy/stream2worlds/issues/64), after #63. The README row reads "Rules; JSON claims;
local embeddings (next)" (Dave, 2026-09-27).

## Consequences

- `s2w watch` output can now become a served world; no command does it yet (#10).
- Every bridge start re-folds the whole log. At gate 2's envelope (minutes of Wikipedia
  traffic) that is well under a second.
- `QueryState::append` takes the write lock once per claim. Fine at this scale; a batch append
  can come later if measured.

verify: `cargo test -p s2w-app --test bridge_replay && cargo test -p s2w-app --test bridge_verdicts` passes.
