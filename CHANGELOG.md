# Changelog

How Stream2Worlds got built, one entry per sprint, newest first. Git history records every commit; this file records the arc: what became possible, what we learned, and where the plan changed direction.

Each entry has the same four parts:

- **Shipped:** what now works or is decided, and why it matters.
- **Learned:** what a measurement, review or research note taught us.
- **Changed course:** decisions that overturned an earlier plan, with the reason.
- **Next:** what the following sprint picks up.

A sprint without a merge still gets an entry. What it learned is often the most useful part.

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
