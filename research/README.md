# Research

Stream2Worlds should be well researched (Dave, 2026-09-27). This folder holds the research
behind its decisions: what exists, what is known, and what we would be foolish to reinvent.

## Conventions

- **One note per question**, numbered `NNNN-<slug>.md`, never renumbered.
- Each note starts with: the question, the date, who researched it, and **which decision or
  gate it feeds**. Research that feeds nothing waits.
- Every claim cites a source URL. Anything not verified is marked **UNVERIFIED**. Facts and the
  researcher's judgment are kept apart.
- Notes are **dated snapshots**. They are not rewritten when the world changes; a later note
  supersedes an earlier one and says so, and this index marks the old one superseded.
- **Research must reach the design** (Dave, 2026-09-27). Every note ends with a **Design
  implications** section: each proposed change to the architecture, the design page, the README,
  the contract or a decision record, with exactly one disposition:
  - `→ adopted: <commit or decision record>`;
  - `→ rejected: <one-line reason>`;
  - `→ deferred: <issue>`.

  A note is not done until every implication has a disposition. The design, the README or the
  contract then cites the note wherever it changed them.

## Index

| # | Question | Feeds | Status |
|---|---|---|---|
| 0001 | Prior art: who already does parts of this, what is open, what to cite | Novelty claim, paper, launch framing, contract A10 (revert-model target) | done |
| 0002 | Discovering structure without an LLM: data profiling, key and dependency discovery, semantic type detection | Gate 3: the heuristics arm must be strong, or System 2 wins against a straw man | done |
| 0003 | The Rust substrate: Kafka client, event-log storage, SSE, incremental computation, MCP server | Gate 2 decision records | done |
| 0004 | Revert pilot: base rate, delays, cutoff losses and B2 behaviour on 30 minutes of English Wikipedia | Contract A10, A8 test length | done |
| 0005 | Exploring worlds in 3D: libraries, stable layouts for a changing graph, navigating time and possible futures, uncertainty in 3D, when 3D is worse than 2D | 3D world explorer (#21), the gate-2 view API (#10) | done |
| 0006 | Scaling: where s2w breaks first as streams grow, and the cheapest way past each wall | Scale envelope (#31), scale fitness function (#32), long runs (#33), the fold (#9), gate 3 and 4 epics | done |
| 0007 | Decision models for System 1 and System 2: which fast judgment models fit a live stream, and routing beyond latency | Local embeddings (#64), verdict log (#63), System 1 router, gate 3 H arm, gate 4 predictor arms | done; refreshed weekly (lifeos#1121) |
| 0008 | A domain-specific dashboard from a domain-free core: a System 2-authored view spec, shape detection, lifetime baseline and surprise, readiness to form an opinion | Decision 0017, #116, #117; the Gate 3 view | draft: design-implication dispositions pending (#116) |
| 0009 | H-lite on plain Wikipedia `recentchange`, held out: identity F1, entity recovery, the v0 alias ceiling, #17 sensitivity | Gate 3 headroom, #17, #244 (containment), #245 (alias limit) | done; implication 3 → #250 (PR 1: `log_action` fixed, measured on `reserved-2`, addendum 2026-09-29; PR 2: `user` keyed, `PROFILER_VERSION` 4, measured on `reserved-3`, second addendum); implication 1 → #244 (containment, `PROFILER_VERSION` 5, measured on `reserved`, addendum 2026-09-30) |

## Planned, not started


- **Showing uncertainty:** narrowed by 0005 §4 (ensemble members, ghosts, no cones in the 3D
  scene) to what it does not cover: position clouds and how to display calibration live.
- **Untrusted stream text:** prompt injection through data, and constraining LLM proposals. Feeds
  System 2 before gate 3.
- **Kafka users' pain:** what people who run Kafka actually struggle with, in their own words.
  Feeds the launch, shared with fish.
