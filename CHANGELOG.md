# Changelog

How Stream2Worlds got built, one entry per sprint, newest first. Git history records every commit; this file records the arc: what became possible, what we learned, and where the plan changed direction.

Each entry has the same four parts:

- **Shipped:** what now works or is decided, and why it matters.
- **Learned:** what a measurement, review or research note taught us.
- **Changed course:** decisions that overturned an earlier plan, with the reason.
- **Next:** what the following sprint picks up.

A sprint without a merge still gets an entry. What it learned is often the most useful part.

---

## Sprint 58 — first build sprint (2026-09-27, 09:00–11:00) · in progress

The first sprint in the main sprint loop, with Stream2Worlds as its full focus.

**Shipped**
- A licence and security-advisory gate: CI now fails when a dependency is not MIT or Apache-2.0, or has a RustSec advisory ([#16](https://github.com/daveremy/stream2worlds/issues/16)).

**Changed course**
- **The event log starts on SQLite.** The first design, our own segment files plus `redb` (research 0003's first choice), was rejected at plan review twice as too much new crash-recovery code for slice 1. The log now uses SQLite in WAL mode, research 0003's runner-up ([#8](https://github.com/daveremy/stream2worlds/issues/8)).
- **Implementation moved to the most capable coding model.** Dave: *"i want code to be high, high quality."* From the next leg, Codex `gpt-6-astra` writes the code, every review round has two independent reviewers that are never the model that wrote it, and the pure core and every seam run the full workflow with a design consult.

**Next**
- The Wikipedia source ([#6](https://github.com/daveremy/stream2worlds/issues/6)) and the log ([#8](https://github.com/daveremy/stream2worlds/issues/8)) land, then the fold with golden replay ([#9](https://github.com/daveremy/stream2worlds/issues/9)).

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
