# Changelog

How Stream2Worlds got built, one entry per sprint, newest first. Git history records every commit; this file records the arc: what became possible, what we learned, and where the plan changed direction.

Each entry has the same four parts:

- **Shipped:** what now works or is decided, and why it matters.
- **Learned:** what a measurement, review or research note taught us.
- **Changed course:** decisions that overturned an earlier plan, with the reason.
- **Next:** what the following sprint picks up.

A sprint without a merge still gets an entry. What it learned is often the most useful part.

---

## Sprint 64 — NDJSON watch progress (2026-09-27, 21:00–22:50)

The follow-up #39 deferred: `s2w watch --json` ([#79](https://github.com/daveremy/stream2worlds/issues/79)).

**Shipped**
- `s2w watch <source> --json`: one NDJSON object per flush on stdout
  (`{"appended","duplicates","reconnects","cursor","at"}`), and `{"error","fatal"}` lines on
  stderr for source errors and the one fatal error that stops the pump — a clean machine-readable
  stream alongside the existing human progress lines, never both at once.
- `crates/s2w-app/src/group_commit.rs` gained a `Reporter` trait (`HumanReporter` reproduces
  today's eprintln lines exactly; `JsonReporter` is the new NDJSON path) so `pump`/`pump_events`
  route through one seam instead of hardcoded `eprintln!`. `serve` keeps `HumanReporter` —
  `serve --json` is out of scope for this issue.
- `crates/s2w/AGENTS.md`'s dated exception (2026-09-27, #39) is resolved: `watch` now has
  `--json`; only `serve` still lacks one.

**Learned**
- `Reporter` needs an explicit `Send` bound: `pump`'s background flush task crosses
  `tokio::spawn`, which requires every value held across an `.await` in that future — including
  `&mut dyn Reporter` — to be `Send`. `dyn Trait` isn't `Send` by default.
- `"at"` is epoch milliseconds (`i64`), not an RFC3339 string, matching this codebase's existing
  convention (`query/timeline.rs`'s `first_ts`/`last_ts`) rather than the issue's original sketch
  — there's no RFC3339-rendering helper anywhere in the workspace to reuse.

**Next**
- `serve --json` progress, if a future issue asks for it — explicitly out of scope here.

---

## Sprint 63 — local embeddings (2026-09-27, 19:00–20:50)

A third System 1 engine ([#64](https://github.com/daveremy/stream2worlds/issues/64)), following
#63's verdict log: local embeddings classify an `enwiki` edit comment into a category by
similarity, additive alongside the existing rules engine on the same page.

**Shipped**
- **`LocalEmbeddingsEngine`**, using `model2vec-rs` with potion-base-8M (vendored, no network
  fetch), split into a plain `CommentClassifier` (no `Engine` dependency, reusable by a future
  System 2/Jev consumer) and a thin `Engine` adapter. Scoped to `enwiki` only; abstains below
  threshold or on a near-tie rather than guessing. [Decision 0013](docs/decisions/0013-local-embeddings-engine.md).
- **Provenance now covers two hashes**, model identity and taxonomy/config, independently
  visible in a stored verdict.
- **Version-bump enforcement is a table, not a pinned pair**: an append-only
  `(version, model_hash, config_hash)` history, with the shipped version derived from the
  table's last row so a hash change and a version bump can't drift apart.
- **A golden-output test on 8 real English edit comments** catches a `Cargo.lock` bump or
  scoring change that a model/config hash alone would miss, and doubles as a calibration
  sanity check: one of the eight lands in `NoMatch` on real, plausibly-worded text.
- README's "System 1 engines" row moves from "next" to built, and a new "How embeddings fit in"
  section explains the engine at the application level.

**Learned**
- One real comment in the golden set — a revert notice — lands in `NoMatch` rather than a
  confident match, evidence (not just a constructed boundary test) that the starting
  `threshold_bps`/`margin_bps` values may be too strict on real `enwiki` text. Named as a
  calibration follow-up in decision 0013, not built here.
- The golden test must run each case through the same `normalize_comment` step production
  runs before classification — an earlier version pinned one case's raw, marker-prefixed text,
  which described a classification production never actually produces (code review round 2,
  s2w#64).
- `model2vec-rs`'s `default-features = false` alone does not compile: `tokenizers` needs
  `onig` or `fancy-regex`. Chose `fancy-regex` over `onig` (which binds the C Oniguruma
  library) — the plan's round-2 dependency-tree check never actually compiled, so this was
  invisible until implementation, and it surfaced a second `cargo deny` advisory
  (`RUSTSEC-2025-0119`, `number_prefix` via `indicatif`) that round 2's check against the
  non-compiling tree couldn't have found either. `fancy-regex`'s own regex engine is pure
  Rust, but the tree still needs a C++ toolchain regardless of this choice: `tokenizers`'s
  `esaxx_fast` feature pulls in `esaxx-rs`, which compiles C++ via `cc` (decision 0013).
- Full-mode plan review hit its 2-round cap with both reviewers still blocking on real,
  convergent findings (the version-bump test and the `BelowThreshold`-reused-for-a-near-tie
  issue); a karpathy ruling folded all four remaining findings into implementation rather than
  spending a third plan-text round on changes that didn't alter the plan's shape.

**Next**
- Threshold/margin calibration against real `enwiki` comment traffic.
- Full boilerplate-template stripping beyond the single leading `/* Section */` marker.
- Jev joins behind the same `Engine` trait once its latency/cost/accuracy are measured.

---

## Gate 2 — live HTTP command (#10, PR1, 2026-09-27)

**Shipped:** `s2w serve <source>` owns source ingestion, durable verdicts and the live query API
in one process. It binds loopback, checks Host headers, refuses competing store owners and
bounds HTTP shutdown even with an open SSE client.

**Learned:** the existing bridge's async runner requires `Send`; a local `poll_once` loop lets
non-Send source streams and a single shared SQLite handle coexist on the current-thread runtime.

**Changed course:** the proposed separate watch/serve processes and lockless reader were
replaced by one owner. Loopback binding also needs a Host allowlist against DNS rebinding.
[Decision 0014](docs/decisions/0014-serve-topology.md) records both choices.

**Next:** PR2 adds the evidence view, assets and bundle gates. The dashboard remains unfinished.

---

## Sprint 62 — watch fails loudly (2026-09-27, 17:00–19:00)

The four hardening items deferred from the #29 review ([#39](https://github.com/daveremy/stream2worlds/issues/39)).
`watch` no longer looks healthy when it isn't: every reconnect says why, `--since` is validated
before any connection opens, and the stored-cursor-versus-`--since` decision is a pure, tested
function instead of live-run glue.

**Shipped**
- **SSE reconnects are reported, never silent.** Connection failures (with attempt count and
  retry delay), dropped byte streams, and zero-frame disconnects all send a `Retrying` error
  down the channel before the backoff; a connection that delivered frames and then closed
  (Wikimedia's routine periodic reconnects) stays quiet. The attempt count resets only when an
  event is accepted, not when a connection succeeds — a 200 that delivers nothing keeps
  counting.
- **`--since` is validated, and invalid values are exit code 2 everywhere.** Wikipedia and
  Kafka share one RFC 3339-or-epoch-millisecond parser (in `s2w-sources`); the SSE
  `SseDialect::apply_since` distinguishes unsupported from invalid, and invalid is a usage
  error. This intentionally narrows Wikipedia's old ISO-8601 wording: a bare date such as
  `2026-09-27` is not RFC 3339 and is now rejected instead of being forwarded silently.
- **The start decision is pure and positively tested.** `sse::start::choose` decides resume
  versus fresh versus error from `(stored cursor, --since)`; `SseSource::start` calls it instead
  of inlining the logic. An app-level loopback test (hand-rolled HTTP over `tokio::net`, no new
  dependency) seeds a cursor, asserts the request carries it as `Last-Event-ID`, and watches the
  event land in the SQLite log.

**Learned**
- **A conflict beats a typo.** When a stored cursor and `--since` are both present, the
  `SinceWithStoredCursor` error wins over validating the `--since` value — the user's mistake is
  the combination, and naming it first saves them fixing a value that was going to be refused
  anyway.

**Changed course**
- **`--json` on `watch` became a dated exception** instead of shipping untested: `watch` is
  streaming, and its NDJSON progress design is deferred to [#79](https://github.com/daveremy/stream2worlds/issues/79);
  `mcp` is already JSON-RPC over stdio. `crates/s2w/AGENTS.md` records the exception.

**Next**
- [#79](https://github.com/daveremy/stream2worlds/issues/79), when a machine consumer needs
  progress from a running watch.

---

## Sprint 61 — the live bridge (2026-09-27, 15:00–17:00)

The log and the world met. Until this sprint the query API and the MCP server served a world
that only a golden replay could fill; now a bridge reads the stored log, asks System 1 engines
what each raw event claims, and folds the claims into query state. It runs as a library, not yet
behind a command. Two engines ship behind one trait, and the second one is what will let anyone
write a world by hand from a file of claims.

**Shipped**
- **The live bridge: log → System 1 engines → query state** ([#51](https://github.com/daveremy/stream2worlds/issues/51), [decision 0011](docs/decisions/0011-system1-bridge.md)). `s2w_system1::Engine` names itself (`name()`, `version()`) and judges with `evaluate(&RawEvent) -> Verdict`, which is total: it never errors and must not panic; a `Verdict` is `Propose { claims, confidence }` or `Abstain { reason }`, and abstaining is a value with a named reason (`NotMine`, `Unparseable`, `Insufficient`, or `Panicked`, which only the bridge produces). Confidence is an integer in basis points, because a float threshold replayed across machines would break byte-identical replay just as floats in the world would. `Bridge<R: LogReader>` polls the log (SQLite has no cross-process notification), backs off on empty polls, and every matching engine runs in registration order, so the timeline is a deterministic function of the log and the registry.
- **Two engines, not one.** `WikimediaPageChangeEngine` turns a page-change event into claims by rules; `JsonClaimsEngine` treats a payload that already is a claim as one (Dave's choice for the second engine). The second is what will let a file of hand-written claims piped through `s2w watch -` fold into exactly the world you wrote, once the bridge is wired into a command ([#10](https://github.com/daveremy/stream2worlds/issues/10)); today `s2w watch -` only stores the lines. It also keeps the engine seam from being designed from a single case.
- **`WorldEvent`, `NaturalKey` and `AttrValue` moved to `s2w-model`** (a dated amendment to [decision 0005](docs/decisions/0005-pure-fold.md)). Engines see only the payload and mint natural keys; they never see a `World`, so the claim types belong to the model, not the core.
- **In-crate fitness functions, first slice: the module-size checker** ([#44](https://github.com/daveremy/stream2worlds/issues/44), a dated amendment to [decision 0001](docs/decisions/0001-workspace-layers.md)). `cargo xtask check` walks every non-test target with `syn`, counts non-test lines per module, refuses `#[path]` and `include!`-family macros, and cross-checks rustc dep-info so a compiled file the walker missed is reported. Cap 400, report-only for now; growth of the exemption list against `origin/main` blocks even in report-only mode, unless a `Baseline-growth: s2w#<N>` commit trailer authorizes it. Over the cap today: `s2w_log` (564) and `s2w_app::query::view` (420), both queued for [#66](https://github.com/daveremy/stream2worlds/issues/66).
- **Research 0007: decision models for System 1 and System 2** ([research](research/0007-decision-models.md)). Jev and the at least a dozen open models that speak its `/v1/systemone` format, read latency first: only Blink-tiny fits per-event at 1,000 events/s; the text-reading classifiers are sampled or asynchronous rungs. Routing beyond latency (cascades, learned routers, bandits over engines) and a ranked spike shortlist. Refreshed weekly (lifeos#1121).

**Learned**
- **Codex was walled mid-sprint on both accounts; Claude Opus 5.5 implemented both PRs.** Dave made Opus 5.5 a peer implementer alongside Codex astra rather than a fallback. The two-independent-reviewer rule held as it did in Sprints 59 and 60.
- **A Codex run left a truncated file on disk** — `module_size.rs` was 1 byte after the run. Clippy caught it, and the file was rebuilt byte-exact from the run log before review. The build gates, not the author, are what noticed.
- **The deepseek reviewer fails on prompts over about 30–40 KB.** Split the diff; a review that never ran is not a review.

**Changed course**
- **Persist verdicts, then embeddings, then measure H** ([#63](https://github.com/daveremy/stream2worlds/issues/63) → [#64](https://github.com/daveremy/stream2worlds/issues/64) → [#56](https://github.com/daveremy/stream2worlds/issues/56)). Local embeddings were the planned second engine; they now wait on the verdict log, because a model file is an input the log does not capture. H is measured only once embeddings are part of it, as the contract's B1 and [decision 0010](docs/decisions/0010-gate3-h-arm.md) define the arm.
- **Gate 3's obfuscated stream folds `wiki` into the title and revision hash domains** ([#17](https://github.com/daveremy/stream2worlds/issues/17), Dave). Cross-wiki composite-key discovery is reported as its own unfloored sub-metric rather than through the floored hash domains. The contract's B2 and B3 text stands as signed; the ruling is recorded there as a dated pointer note.
- **The scale fitness function's scope was cut** ([#32](https://github.com/daveremy/stream2worlds/issues/32), accepted).
- **System 1 as a learning layer is now a thesis, not a row in a table** ([#71](https://github.com/daveremy/stream2worlds/issues/71)): an engine adapter, a router over judgment kind and latency, and System 2 feedback into System 1. Dave: differentiating. Research 0007's deferred implications land there.

**Next**
- The verdict store in two PRs ([#63](https://github.com/daveremy/stream2worlds/issues/63)), then the local embeddings engine ([#64](https://github.com/daveremy/stream2worlds/issues/64)); `watch` hardening ([#39](https://github.com/daveremy/stream2worlds/issues/39)); wiring the bridge into a serving command so a live stream reaches the query API and MCP ([#10](https://github.com/daveremy/stream2worlds/issues/10)); the bridge follow-ups from review ([#74](https://github.com/daveremy/stream2worlds/issues/74)). Also filed: operator-supplied domain context ([#61](https://github.com/daveremy/stream2worlds/issues/61)), the remaining fitness-function slices ([#65](https://github.com/daveremy/stream2worlds/issues/65)–[#69](https://github.com/daveremy/stream2worlds/issues/69)), and the lifeos-side tooling epic ([#62](https://github.com/daveremy/stream2worlds/issues/62)).

---

## Sprint 60 — sources become adapters and presets (2026-09-27, 13:00–15:00)

Wikipedia stopped being special. The single `Source` trait research 0006 built the group-commit
API for now has three real transports behind it, and Wikipedia is one named configuration of one
of them, not its own module.

**Shipped**
- **Kafka, by explicit partition assignment** ([#7](https://github.com/daveremy/stream2worlds/issues/7), [decision 0007](docs/decisions/0007-kafka-client.md)). `s2w watch kafka://broker/topic` reads the topic's partitions from cluster metadata, resolves one start offset per partition, and runs one fetch loop per partition — never a consumer group, never an offset commit. Each partition is its own log source with its own stored offset; a resume offset deleted by retention or past the partition's end is a loud, fatal error, never a silent skip. A record is stored as a byte-deterministic JSON envelope so the log's content-hash dedupe collapses redeliveries but never distinct records.
- **The registry resolves `s2w watch <uri>` by scheme** ([#49](https://github.com/daveremy/stream2worlds/issues/49)): `kafka://`, `sse://`/`https://`/`http://`, `-` for stdin, or an exact preset name. `s2w-app` no longer has a line of per-source code — `WatchWikipediaArgs`, `WIKIPEDIA_SOURCE` and `watch_wikipedia` are gone, replaced by one `s2w_app::watch(WatchArgs)` over any `Source`.
- **The SSE transport generalized; Wikipedia became a preset over it** ([decision 0008](docs/decisions/0008-generic-sse-adapter.md), a dated amendment to [decision 0003](docs/decisions/0003-wikipedia-sse-client.md)). `sse/{mod,connect,frame}.rs` carry the connection, backpressure and reconnect-backoff logic every SSE stream shares; `sse/dialect.rs`'s `SseDialect` trait carries what only one stream knows — how an `id:` becomes a cursor, how to ask for a start time, which frames to keep. `Wikimedia` (now under `presets/`) implements the existing cursor-arbitration and canary/`examplewiki` filtering; `Opaque` is the default for a bare `sse://`/`https://`/`http://` target: the `id:` verbatim as the cursor, no `--since` support, every payload kept. A frame with no `id:` cannot be resumed from, so three in a row force a reconnect — forever, not a crash or a hang, because the transport cannot know whether an arbitrary stream was ever meant to carry ids.
- **stdin NDJSON** joined the same seam: one raw-line-per-event adapter, no `--since` support, ending at end of input.
- **A read-only MCP server** ([#52](https://github.com/daveremy/stream2worlds/issues/52), [decision 0009](docs/decisions/0009-mcp-server.md)). `s2w mcp` serves five tools over stdio (`world_view`, `world_diff`, `entity_history`, `branches`, `time`) that return the same JSON bytes as the HTTP query routes, because both now call the same `QueryState` methods. Every tool is annotated read-only; the world is empty until the live bridge (#51) lands.
- **`--json` on the CLI** ([#53](https://github.com/daveremy/stream2worlds/issues/53)): `--version`, `--help` and top-level errors print JSON with `--json`. `watch --json` is deferred to [#79](https://github.com/daveremy/stream2worlds/issues/79) (NDJSON progress design, dated exception recorded by #39); `s2w mcp` refuses extra arguments so nothing but MCP messages ever reaches its stdout.
- **What the gate-3 heuristics arm H contains, decided** ([#4](https://github.com/daveremy/stream2worlds/issues/4), [decision 0010](docs/decisions/0010-gate3-h-arm.md)): research 0002's seven-stage design, with Rebmann, Rehse and van der Aa (BPM 2022) as a component of H rather than a fourth arm. Measuring H-min is [#56](https://github.com/daveremy/stream2worlds/issues/56). The signed evaluation contract gets a dated pointer note, not an in-place edit.
- **Generic SSE keeps distinct events distinct.** The `Opaque` dialect stores `{"data","id"}` as a byte-deterministic envelope, so two events with different ids and identical data no longer collapse under the log's dedupe; the `wikipedia` preset still stores raw `data:` bytes, so logs from Sprint 59 resume unchanged (checked: 268 → 908 events, contiguous).
- **`sse/mod.rs` split to stay under the 400-line cap** ([#44](https://github.com/daveremy/stream2worlds/issues/44)): the HTTP connection, request-building and backoff moved to `sse/connect.rs`; `mod.rs` keeps the `Source` impl and the read loop.

**Learned**
- **Codex astra was walled on both ChatGPT accounts for the first hour.** The Kafka and adapter chunks were implemented on Claude Opus instead (Dave-approved fallback); astra implemented the MCP server and `--json` after its reset and reviewed the adapter PR. Its four review rounds found five real bugs (two Kafka source-id collisions, an SSE source-id collision, a timestamp-fallback skip race, same-data SSE events collapsing). The two-independent-reviewer rule held regardless of who wrote the code.
- **"Kafka never emits `Skipped`" needed a new error variant, not a workaround.** Kafka's fetch errors are always retryable from the same offset, never a decode failure, so they needed their own non-fatal `SourceError::Retrying` rather than overloading `Skipped`, which now means only "a malformed frame, safe to drop."
- **A dialect that assumes JSON is the wrong shape for a *generic* transport.** The plan's first `SseDialect::is_filtered(&Value) -> bool` baked in a JSON assumption a bare `https://` target does not share. It became `accept(&str) -> Result<bool, String>`: the dialect decides whether and how to parse, and a parse failure is a reported `Skipped`, not a panic.

**Changed course**
- **None.** Both plan-review rounds (fable + opus, `full` mode's 2-round cap) converged on APPROVE with fixes folded in before implementation, rather than a course change mid-build.

**Next**
- The live bridge from the log into query state ([#51](https://github.com/daveremy/stream2worlds/issues/51), plan reviewed), in-crate fitness functions ([#44](https://github.com/daveremy/stream2worlds/issues/44), plan written), `watch` hardening and `watch --json` ([#39](https://github.com/daveremy/stream2worlds/issues/39)), measuring H-min ([#56](https://github.com/daveremy/stream2worlds/issues/56)), and the evidence view ([#10](https://github.com/daveremy/stream2worlds/issues/10)).

---

## Sprint 59 — the world becomes queryable (2026-09-27, 11:00–13:00)

Five merges in two hours. The sprint opened with a stored log and a source, and closed with a command that runs for hours, a fold that replays byte-for-byte, and an HTTP surface over the folded world.

**Shipped**
- **`s2w watch wikipedia` is a real command** ([#29](https://github.com/daveremy/stream2worlds/issues/29), [#25](https://github.com/daveremy/stream2worlds/issues/25)). It resumes from the stored cursor, replays history with `--since` into a fresh log, and the log collapses redelivered events. The live proof: 47,282 real edits over 38.5 minutes, 0 duplicates, 0 gaps across a restart, and exactly 300 forced redeliveries collapsed.
- **The pure fold, with golden replay** ([#9](https://github.com/daveremy/stream2worlds/issues/9), [decision 0005](docs/decisions/0005-pure-fold.md)). Events in, world out, no I/O, clock or randomness. An entity id is never reused: a merge aliases, a revoke splits. Relationships into an entity past an in-degree of 10^4 become an attribute plus counters, so one hub page cannot swamp the world. `cargo xtask check` folds the golden log twice and from every prefix save/reload and requires byte-identical output.
- **A world query API in `s2w-app`** ([#36](https://github.com/daveremy/stream2worlds/issues/36), [decision 0006](docs/decisions/0006-world-query-api.md)). `/world?at=&branch=&lod=&focus=&hops=` returns the world at a fold offset in the d3 shape; SSE deltas arrive one per offset so `Last-Event-ID` resume is unambiguous; `/branches`, `/diff`, `/entity/:id/history` and `/time` complete the contract. Only the actual world is served: another `branch` is `501 branch_not_yet`, `lod=cluster` is `501 lod_not_yet`. A hub's own relationship into another hub shows at `lod=entity` too ([#42](https://github.com/daveremy/stream2worlds/issues/42)).
- **The slice-1 scale envelope, decided** ([#37](https://github.com/daveremy/stream2worlds/issues/37), [decision 0004](docs/decisions/0004-scale-envelope.md), Dave approved). One process on a 4-core, 16 GB laptop: 1,000 events/s, 10^6 live entities in 1 GB, 20 forks in under 100 ms. `synchronous=FULL` everywhere; throughput comes from group commit, not from a weaker durability setting. Not a distributed system.
- **Research 0005 (3D exploration) and 0006 (scaling)** ([research](research/)), both with every design implication dispositioned. 0006 found where `s2w` breaks first as a stream grows and the cheapest step past each wall; the envelope above and the hub cap are its first adopted implications.
- **The README now calls this a research project**, pre-alpha, with the gates as pre-registered questions that can fail. A hosted direction is filed for later: a dedicated machine per user, log and snapshots in object storage ([#35](https://github.com/daveremy/stream2worlds/issues/35)).
- **Cold builds about 9% faster** with `[profile.dev] debug = 1` ([#41](https://github.com/daveremy/stream2worlds/issues/41)): line tables stay, variable and type debuginfo goes.

**Learned**
- **Codex ran out by 11:15 on both accounts.** Four legs were implemented on Claude Opus instead. The two-independent-reviewers rule held; what changed was who wrote the code.
- **Two plan reviews hit the two-round cap.** Both times the right move was to apply the small fixes the reviewers had converged on and proceed, not to run a third round.
- **`s2w-app` is Wikipedia-shaped.** Wiring the first source straight into the app was fast, but the wiring knows it is Wikipedia. The second source exposes that as a layering finding, not a style nit.

**Changed course**
- **Every source goes behind one `Source` trait and `s2w watch <source-uri>`** ([#7](https://github.com/daveremy/stream2worlds/issues/7), in progress). The group-commit batch append API from research 0006 is already built for it; leg C moves Wikipedia, Kafka and stdin behind the same seam.
- **Fitness functions move into the crates** ([#44](https://github.com/daveremy/stream2worlds/issues/44)): small modules, small functions, visible public APIs, tests that bite, checked next to the code they judge rather than only from `xtask`.

**Next**
- #7 leg C (Kafka and stdin behind the `Source` trait), #44 (in-crate fitness functions), and the evidence view ([#10](https://github.com/daveremy/stream2worlds/issues/10)), now unblocked by the query API.

---

## Sprint 58 — first live stream (2026-09-27, 09:00–11:00)

The first sprint in the main loop, with Stream2Worlds as its full focus. It ended with live Wikipedia edits landing in a durable log: the first time `s2w` code touched a real stream.

**Shipped**
- **Live Wikipedia edits land in a durable log.** The Wikipedia EventStreams source ([#6](https://github.com/daveremy/stream2worlds/issues/6)) and the append-only event log ([#8](https://github.com/daveremy/stream2worlds/issues/8)) merged within twenty minutes of each other. The sprint demo streamed 561 live edits (1.5 MB) into the log in 30 seconds; a restart found them all and appended 310 more. The command that wires the two together, `s2w watch wikipedia`, is next.
- **An event log that cannot be rewritten.** Each append saves the event and advances its source cursor in one SQLite transaction, so a failed append means "not stored" and a retry is safe. Triggers refuse UPDATE, DELETE and INSERT OR REPLACE. A crash test kills the writer mid-batch and checks nothing is half-written. An in-memory implementation passes the same test suite, so the seam has two implementations from the start ([decision 0002](docs/decisions/0002-event-log-storage.md)).
- **A source that survives Wikimedia's disconnects.** Wikimedia drops every connection within 15 minutes. The source reconnects with the exact `Last-Event-ID` it last parsed, never advances past a half-received frame, and drops test-wiki and canary events ([decision 0003](docs/decisions/0003-wikipedia-sse-client.md)).
- **A licence and security-advisory gate.** CI fails when a dependency's licence is not on the permissive list, or when it has a RustSec advisory ([#16](https://github.com/daveremy/stream2worlds/issues/16)).

**Learned**
- **Independent reviewers earn their keep.** On the Wikipedia source, round 2 found that a fix for untestable assertions still raced a background task (opus), and that the test-wiki filter checked a field the real stream does not use: 1,615 of 1,615 recorded frames carry `wiki_id`, not `database` (Fable). Neither reviewer wrote the code.
- **The TLS stack widened the licence list.** `reqwest` with `rustls` pulls in more than 20 crates under ISC, BSD-3-Clause and Unicode-3.0. All are permissive, so the gate now allows them by name instead of by per-crate exception.
- **A long-lived branch can go silent in CI.** The source's PR drifted behind main and GitHub queued no check runs at all, which reads as "not started yet". The fix was merging main, which surfaced the licence finding above.

**Changed course**
- **The event log starts on SQLite.** The first design, our own segment files plus `redb` (research 0003's first choice), was blocked at plan review twice on crash-atomicity gaps between two stores. SQLite in WAL mode, the runner-up, removes all four findings by construction. The custom format waits for a measured need ([#19](https://github.com/daveremy/stream2worlds/issues/19)).
- **Deduplication on resume belongs to the log.** Resuming from a timestamp can redeliver an event, so dedup by event id moves to the log layer ([#25](https://github.com/daveremy/stream2worlds/issues/25)).
- **Implementation moved to the most capable coding model.** Dave: *"i want code to be high, high quality."* From the next leg, Codex `gpt-6-astra` writes the code, every review round has two independent reviewers that are never the model that wrote it, and the pure core and every seam run the full workflow with a design consult.
- **The explorer will be 3D.** Research 0005 settled on three.js for a world explorer ([#21](https://github.com/daveremy/stream2worlds/issues/21)), with the web view reading the world through the same query API as MCP.

**Next**
- `s2w watch wikipedia` ([#29](https://github.com/daveremy/stream2worlds/issues/29)): the source wired into the log behind a command, resuming from the stored cursor; then the fold with golden replay ([#9](https://github.com/daveremy/stream2worlds/issues/9)) and the evidence view ([#10](https://github.com/daveremy/stream2worlds/issues/10)).

---

## Foundation — pair mode (2026-09-27, before 09:00)

Dave and karpathy built the base the rest stands on, working together live before the project entered the sprint loop.

**Shipped**
- **Gate 1: the evaluation contract, signed** ([contract](docs/evaluation-contract.md)). It says how we will know whether `s2w` works, written before any code: the forecast question, which edits count, the baselines, and the pass thresholds. Five Codex review rounds took it from 17 findings to "sign".
- **Gate 2's skeleton, reviewed and approved.** Nine crates whose boundaries are the architecture, plus `cargo xtask check`, which fails the build when a crate reaches across a layer, adds an unlisted dependency, or drops its `AGENTS.md`. Four review rounds ([decision 0001](docs/decisions/0001-workspace-layers.md)).
- **Four research notes** ([research](research/)): prior art, structure discovery without an LLM, the Rust substrate, and a live revert pilot. Every design implication is adopted or filed as an issue.
- **A roadmap you can follow:** one milestone and one epic per gate.

**Learned**
- **The forecast question is well posed.** A 30-minute pilot on English Wikipedia: 1,854 eligible edits, 3.8% reverted within 30 minutes, and Wikimedia's own revert-risk model at ROC AUC 0.888 on that question, but with raw scores far above the base rate, so it must be recalibrated ([0004](research/0004-revert-pilot.md)).
- **The closest prior art is Zep's Graphiti**, which puts an LLM on every event. `s2w`'s claim is the combination: the LLM never touches an event, the world replays from its log, and every forecast is graded ([0001](research/0001-prior-art.md)).
- **The heuristics arm may be very strong.** Structure discovery without an LLM might score above 0.90 on Wikipedia, which would leave System 2 no room to win by gate 3's margin. So we build and measure the heuristics first ([0002](research/0002-structure-without-llm.md), [#4](https://github.com/daveremy/stream2worlds/issues/4)).

**Changed course**
- **No text-scanning ratchet.** The first skeleton counted `unwrap` and `#[allow]` in source text. Review kept finding ways around it, so it was replaced by compiler lints set to `forbid`, which cannot be bypassed from inside a file.
- **"Possible worlds" gets a definition.** In databases the phrase means uncertainty about the present; ours means sampled futures. The README now defines it once and borrows the database field's Monte Carlo semantics.

## Design (before the repository)

The idea went through the [design document](docs/design/stream2worlds-design.html) and two rounds of critic reviews from Codex and Claude ([reviews](docs/reviews/)) before the first commit. It drew on its predecessors: PredictStream, which proved that forecasting on a stream works and that an LLM on the event path is too slow, and three Rust prototypes (worldcraft, timely_worlds, strema) that explored world models over streams.
