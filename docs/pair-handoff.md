# Pair handoff — S2W foundation (karpathy-dev window)

Written 2026-09-27 07:25 MST by karpathy (karpathy-dev pane) before a `/clear`. This is the
pair-mode handoff for Stream2Worlds; it deliberately does NOT use `~/lifeos/docs/handoffs/karpathy.md`,
which belongs to the main karpathy window's sprint loop.

## Where things are
- Repo: `daveremy/stream2worlds` (GitHub, `gh`, PRIVATE until launch). Bootstrap commit `e2ae240` on `main`.
- Working branch: `pair/foundation`, worktree `~/code/worktrees/stream2worlds-foundation`. Never commit to `main`.
- Design page (interactive artifact, v9): https://claude.ai/artifact/T6E4ueH83AR1kmJf6a2ohe — account **email@daveremy.com**.
  Source copy: `docs/design/stream2worlds-design.html`. To update the artifact from a new session:
  `Artifact action=read url=<above>` first, then publish with `url=` (a publish without `url` makes a new artifact).
  Run `bash ~/lifeos/scripts/artifact-lint.sh <file>` before every publish.
- Reviews: `docs/reviews/` — round 1 and 2, Codex (gpt-6-astra, BLOCK both times, round 2 mainly on scope) and Claude critic.
- README is written and renders on GitHub (banner light/dark, badges, roadmap checkboxes).

## Decisions from Dave (2026-09-27, AskUserQuestion + conversation)
- Foundation built in PAIR MODE in this window; then S2W becomes the PRIMARY track in the main sprint loop.
  ⛔ Never run a second sprint loop here (shared plan/handoff files and session-cycle with the main window).
- While S2W is primary: NL gets a slot every OTHER sprint; lifeos only breakage/savings.
- Cap: primary until gate 3 or 2026-10-11, whichever first; then re-decide on evidence.
- Recorded in `~/lifeos/.claude/agents/karpathy.md` (section "Stream2Worlds (S2W) is the PRIMARY sprint track…", pushed b316c6e8).
- First audience: Hacker News / X, especially Kafka users. Open source (permissive) at launch. NOT the KurrentDB crowd.
- Experiment posture: opinions held lightly.
- Rust from day one. Good architecture from the start, paid continuously (25% reserve per sprint, fitness functions gate PRs, circuit breakers).
- Every kept seam ships with TWO real implementations in the first slice (Dave, 07:07): Source (Wikipedia SSE + Kafka assign/no-group, stdin free), System 1 (rules + local embeddings, Jev third), System 2 (hosted LLM + client agent via MCP sampling/local), client (web view + read-only MCP), outcome source (in-stream revert tags + Wikimedia revert model), domain (Wikipedia + obfuscated copy + a private stream Dave owns).
- A runnable demo on live data every sprint; a real-value demo every third sprint.
- Dashboard visualization is as important as the LLM interface (ghosts, risk weather map, cones, scrub-past-now, world lanes, live calibration).
- Name: Stream2Worlds / `s2w` (crates.io names free as of 2026-09-27; not reserved).

## The first slice (from the design page)
1. **Gate 1 — evaluation contract (NEXT, judgment work with Dave).** Write `docs/evaluation-contract.md`:
   - the question: "a recognized revert within 30 minutes" of a Wikipedia edit; MediaWiki tags reverts asynchronously (`mw-reverted` via a job), so labels are collected late and absence at the horizon is not a negative;
   - eligible population (which wikis, namespaces, bot edits in or out), one issuance per edit at a declared cutoff, repeated-forecast weighting, label finalization window, abstention handling, censoring;
   - baselines: base rate, Wikimedia's revert-risk model (check target/inputs match; Automoderator may cause reverts — limits claims), a raw-sample LLM summary (for gate 3);
   - gate 3 comparison design: heuristics vs heuristics + one budget-capped System 2 pass, on Wikipedia, an obfuscated copy (renamed fields, hashed ids), and one private stream Dave owns and can label (candidate: lifeos event logs — Dave to confirm); mappings frozen, evaluated on a later window; metrics = identity/relationship/state accuracy vs labels, false-merge rate, abstention, correction minutes;
   - pass thresholds and kill criteria; the overall hours cap (Dave to set).
2. **Gate 2 — local harness in Rust.** Workspace (see below), Wikipedia SSE + Kafka (partition assign, `offsetsForTimes`, never commit) + stdin NDJSON, append-only log with source cursors and provenance, pure fold with golden replay, evidence table + simple graph view, `--json`, read-only MCP; first three fitness functions; per-crate AGENTS.md; decision records 0001+. Done when restart/duplicates/gaps/ordering verified and a fresh Claude Code session sets it up on a local topic from one sentence.
3. Gate 3 — does System 2 earn its place (vs heuristics AND raw-sample LLM baseline).
4. Gate 4 — one forecast ledger (issuance immutable; outcome observations appended/versioned).
5. Launch — split-screen obfuscation clip, forecasts as badges, one install path, Show HN. Local-first.

## Workspace plan (round-2 critic, adapted to two-implementations-per-seam)
```
s2w (bin CLI) → s2w-app (runtime, wiring, read-only MCP, local web view)
  s2w-sources · s2w-system1 · s2w-system2 · s2w-log · s2w-core  → all depend on s2w-model only
s2w-model (Event, Offset, EntityId, Repair, Proposal, Issuance; serde + thiserror only)
s2w-testkit (dev) · xtask (fitness checks)
```
First fitness functions: (1) per-crate dependency ALLOWLIST over `cargo metadata`; (2) replay determinism with expected answers, across partition interleavings/thread counts/restarts, golden files human-owned; clippy `disallowed_methods`/`disallowed_types` for wall clock, RNG, HashMap in core/model; (3) escape-hatch ratchet (per-crate counts of `#[allow]`, `unwrap`, `todo!`, `pub` items may only fall without a decision record). Lints: `unreachable_pub`, `allow_attributes_without_reason`.

## Review findings still to honour while building
- Forecast = immutable Issuance + appended OutcomeObservation; pruning never un-scores; censored ≠ refuted.
- Rules read registered forecasts, never "any world"; `forecast.ask` on an unregistered question → "unsupported".
- Proposals inert until explicitly accepted; nothing auto-accepts in the experiment; provisional repairs cannot trigger anything.
- Two replay histories ("as known then" / "reinterpreted now"); persist all model outputs (System 1 verdicts too); never re-run an LLM for replay.
- Dedupe must never merge identical payloads with different legitimate source identities.
- MCP read-only first; capabilities never expand from a good record; stream text is untrusted.
- Local by default; cloud only via an approved export manifest (deferred until after the slice).

## Gotchas
- `cargo` is at `~/.cargo/bin/cargo` (1.98.1), not on the Bash PATH. `CARGO_TARGET_DIR=~/.cache/cargo-target/stream2worlds/<branch>`.
- Never put the protected dir string (dot-claude) in Bash command text that writes; use the Write/Edit tools.
- Artifact links always name the account (email@daveremy.com; that Max plan ends 2026-10-17 — consider republishing under davidlremy@gmail.com before then).
- Pair skill: fast-follow Codex astra review per chunk (`scripts/codex-review.sh`, mint a seat id with bare `uuidgen` first); karpathy merges.
- Context: hand off again at ~120k (this file is the handoff target; update it, commit on the branch).

## Open for Dave
- Hours cap for S2W; which private stream to use for gate 3; the pass thresholds in the evaluation contract.
