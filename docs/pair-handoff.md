# Pair handoff — S2W foundation (karpathy-dev window)

Updated 2026-09-27 ~08:22 MST by karpathy (karpathy-dev pane). Pair-mode handoff for
Stream2Worlds; deliberately NOT `~/lifeos/docs/handoffs/karpathy.md` (the main window's sprint
loop). ⛔ Never run a second sprint loop here.

## Where things are
- Repo `daveremy/stream2worlds` (GitHub, `gh`, PRIVATE until launch). Branch `pair/foundation`,
  worktree `~/code/worktrees/stream2worlds-foundation`, pushed to origin. Never commit to `main`;
  karpathy merges the branch via PR when the foundation is done.
- Design page (artifact v9): https://claude.ai/artifact/T6E4ueH83AR1kmJf6a2ohe — account
  **email@daveremy.com** (that Max plan ends 2026-10-17). Source: `docs/design/stream2worlds-design.html`.
  Read the artifact before republishing with `url=`. `bash ~/lifeos/scripts/artifact-lint.sh` first.
- `cargo` = `~/.cargo/bin/cargo` (1.98.1). `CARGO_TARGET_DIR=~/.cache/cargo-target/stream2worlds/pair-foundation`.
- ⚠️ 2026-09-27 ~08:05: the Bash tool's SANDBOX started failing every command (even `echo`),
  exit 1, no output; `dangerouslyDisableSandbox: true` works. Cause unknown; `/tmp` 68% full,
  not the cause. Retry sandboxed after a restart; if it persists, file a lifeos issue.

## Done this session (all on the branch, pushed; last commit after b91fc01)
- **Gate 1 signed:** `docs/evaluation-contract.md` v4.1 (five Codex rounds, last "sign"; reviews
  in `docs/reviews/gate1-contract-round{1..5}-codex.md`). Dave approved every sign-off row
  (English-only with multilingual re-run; B2 reported not required; calibration equivalence
  [0.8, 1.25]; gate-3 bar incl. entity-recovery floor and no waiver; 60 h cap). Signed ≠ frozen.
- **Revert provenance** is in `mediawiki.page_change.v1` (`revision.revert`); live SSE drops after
  ~6 min (reconnect by Last-Event-ID); `since=` replay works (skip `examplewiki` canaries).
- **Research:** `research/` with conventions (every note ends with Design implications, each
  adopted/rejected/deferred — Dave: research must reach the design). 0004 revert pilot done.
- **Gate 2 skeleton APPROVED** after four Codex rounds (`docs/reviews/skeleton-round{1..4}-codex.md`):
  9 crates + xtask; `cargo xtask check` = dependency allowlist by identity (crates.io only),
  README stack table by exact token, AGENTS.md everywhere, `[lints] workspace = true`, no
  `[patch]`/`[replace]`/`paths` overrides. The text-scanning ratchet was DROPPED (unbounded bypass
  space, lifeos#1034): escape-hatch lints are `forbid` (unwrap, expect, todo, unimplemented, dbg,
  unsafe, unreachable_pub). Decision 0001 amended. CI calls xtask directly (not the alias).
  CI has not yet run on GitHub — check the first run.
- Codex account switched to **email@daveremy.com** at 08:14 (davidlremy walled until 09:11).
- Issue daveremy/stream2worlds#1 (paper, after gate 4) with positioning comment.
- lifeos def edit `1e585357` (doc scrub at S2W sprint wrap + 60 h cap).

## Research in flight (sagan subagents, launched 07:48-07:53) — status UNKNOWN after a /clear
- **0001 prior art: WRITTEN** at `/tmp/claude-1000/-home-dave-lifeos/058d8576-1408-449c-9d01-beddd4dabaae/scratchpad/sagan-prior-art.md`
  (60 KB, 08:18). Copy to `research/0001-prior-art.md`, add the header block and a Design
  implications section. Implications seen so far (disposition each):
  1. Biggest threat **Zep/Graphiti** (arXiv 2501.13956): name it ourselves in README/launch; the
     real differences are LLM off the hot path, discovered identity keys, replay, forecasts,
     obfuscation test.
  2. **"Possible worlds"** collides with probabilistic-database semantics (Dalvi & Suciu): keep in
     the pitch, define in one sentence, use "sampled future worlds" for the mechanism.
  3. **Export OCEL 2.0** (object-centric process-mining standard): adopt its vocabulary (object
     type, E2O, O2O) in s2w-model; decision record.
  4. **Rebmann et al. BPM 2022** (discovers object types/relations from flat logs): add as a
     gate-3 comparison or cite as the closest academic prior art; contract amendment if added.
  5. Claim the intersection and the evaluation; attribute every component.
- **0002 structure discovery without LLMs** → `research/0002-structure-discovery-without-llms.md`
  (check it exists). Feeds the gate-3 heuristics arm.
- **0003 Rust substrate** → `research/0003-rust-substrate.md` (check it exists). Feeds decision
  records 0002+ (Kafka client, SSE, log storage, MCP, web view, deterministic testing).
- If a file is missing and no agent is running, relaunch that sagan brief (they are in this
  session's transcript; the briefs' questions are also summarised in `research/README.md`).

## Next
1. Research 0001-0003 → notes in `research/` with dispositioned implications → decision records,
   README (name Graphiti; "possible worlds" definition), design page, contract amendments if any.
2. Gate 2 proper: sources (SSE with reconnect/replay per research/scripts, Kafka assign +
   offsetsForTimes, stdin), the log, the pure fold with golden replay (replay-determinism fitness
   function), evidence table + graph view, `--json`, read-only MCP. Demo every sprint.
3. Design page artifact: carry the contract corrections, the ratchet removal, "possible worlds"
   wording, and the Graphiti positioning.

## Standing decisions (from the earlier handoff, still true)
Foundation in pair mode here, then S2W becomes the primary track in the main sprint loop; NL every
other sprint; primary until gate 3 or 2026-10-11. First audience HN/X, Kafka users. Open source
(permissive) at launch. Every kept seam ships with two real implementations in the first slice.
Dashboard visualization as important as the LLM interface. Review findings still to honour while
building: see the design page and `docs/reviews/`.
