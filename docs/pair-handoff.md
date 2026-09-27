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

## Handed to the main sprint loop (2026-09-27 ~08:35)
- Research 0001-0004 all filed and dispositioned. Deferrals are issues.
- Roadmap: milestones 1-4 = Gate 2, Gate 3 (due 2026-10-11, the re-decide date), Gate 4, Launch;
  epics #12-#15; gate-2 children #6-#10, #16, #2; gate-3 #4, #17; gate-4 #5; launch #3, #1.
- PR #11 merges this branch into `main`. From then on S2W work is issue-driven in the main
  `karpathy` window; this pair branch is done.
- ⚠️ Gate-3 risk for Dave (research 0002): H may exceed 0.90 identity F1 on Wikipedia, making B4's
  0.10 margin unreachable. Build and measure H first (#4); re-decide the margin before the freeze.

## Standing decisions (from the earlier handoff, still true)
Foundation in pair mode here, then S2W becomes the primary track in the main sprint loop; NL every
other sprint; primary until gate 3 or 2026-10-11. First audience HN/X, Kafka users. Open source
(permissive) at launch. Every kept seam ships with two real implementations in the first slice.
Dashboard visualization as important as the LLM interface. Review findings still to honour while
building: see the design page and `docs/reviews/`.
