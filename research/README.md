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
- When a note changes a decision, the decision record (`docs/decisions/`) or the contract cites
  the note.

## Index

| # | Question | Feeds | Status |
|---|---|---|---|
| 0001 | Prior art: who already does parts of this, what is open, what to cite | Novelty claim, paper, launch framing, contract A10 (revert-model target) | in progress |
| 0002 | Discovering structure without an LLM: data profiling, key and dependency discovery, semantic type detection | Gate 3: the heuristics arm must be strong, or System 2 wins against a straw man | in progress |
| 0003 | The Rust substrate: Kafka client, event-log storage, SSE, incremental computation, MCP server | Gate 2 decision records | in progress |

## Planned, not started

- **Showing uncertainty:** forecast cones, fan charts, hypothetical outcome plots, how people
  read probabilities in a live dashboard. Feeds the dashboard after gate 2.
- **Untrusted stream text:** prompt injection through data, and constraining LLM proposals. Feeds
  System 2 before gate 3.
- **Kafka users' pain:** what people who run Kafka actually struggle with, in their own words.
  Feeds the launch, shared with fish.
