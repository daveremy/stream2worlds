# Pair handoff — S2W foundation (karpathy-dev window)

Updated 2026-09-27 ~08:12 MST by karpathy (karpathy-dev pane). Pair-mode handoff for
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

## Done this session (all on the branch)
- **Gate 1 signed:** `docs/evaluation-contract.md` v4.1, five Codex (gpt-6-astra) rounds, last
  verdict "sign"; reviews in `docs/reviews/gate1-contract-round{1..5}-codex.md`. Dave approved:
  English-only population (multilingual re-run after the slice, stated as a choice); B2 reported
  not required; calibration = 90% interval of observed/expected inside [0.8, 1.25]; gate-3
  thresholds incl. the v4 fixes (entity-recovery floor ≥ 60%, no high-baseline waiver); 60 h
  sprint-time cap from the first gate-2 sprint. Private stream = lifeos dev-worker/sprint log
  (karpathy default, swappable). Signed ≠ frozen: prerequisites in the contract's status block.
- **Key technical find:** `mediawiki.page_change.v1` carries exact revert provenance
  (`revision.revert.{method,is_exact,rev_original_id,rev_reverted_oldest_id,rev_reverted_newest_id}`);
  the label is built on it, not on `mw-reverted` tags. Wikimedia also streams its revert-risk
  scores (`mediawiki.page_revert_risk_prediction_change.v1`). Live SSE connections drop after
  ~6 min: reconnect by Last-Event-ID; `since=` replay works (skip `examplewiki` canary events).
- **Research folder** `research/` (conventions + index; every note ends with Design implications,
  each dispositioned adopted/rejected/deferred — Dave: research must reach the design).
  0004 revert pilot done (base rate 3.8%, B2 AUC 0.888 but raw mean 0.40). 0001 prior art,
  0002 structure discovery without LLMs, 0003 Rust substrate: **sagan subagents launched ~07:48–07:53,
  status UNKNOWN after a /clear — check `research/` for the files before relaunching.** 0001's
  output goes to the scratchpad path in its brief (`.../scratchpad/sagan-prior-art.md`), not the
  repo — copy it to `research/0001-prior-art.md`. Then apply each note's implications to the design.
- **README:** Technical architecture table (checked by xtask), Evaluation section, research link,
  gate 1 ticked. Repo `CLAUDE.md`: doc scrub at every sprint wrap (6 items).
- **Gate 2 skeleton** (`5e12c00`): 9 crates + xtask per the layer plan; `cargo xtask check` =
  dependency allowlist, README stack-table check, AGENTS.md presence, escape-hatch ratchet;
  clippy disallowed methods/types in s2w-core/s2w-model; decision record 0001; CI workflow
  (GitHub-hosted, untested on GitHub yet). All five checks shown to fire.
  **Codex review of the skeleton was running at handoff** (output
  `/tmp/claude-1000/-home-dave-lifeos/058d8576-1408-449c-9d01-beddd4dabaae/scratchpad/review6-astra.md`) —
  read it and fix findings; its log already flagged ratchet-scanner gaps and a thin purity denylist.
- Issue daveremy/stream2worlds#1: plan an academic paper after gate 4 (positioning comment added).
- lifeos def edit `1e585357` (doc scrub at S2W sprint wrap + 60 h cap).

## Next
1. Skeleton review findings → fix → re-review the delta.
2. Research notes 0001–0003 → apply implications (decision records 0002+ for Kafka client, SSE,
   log storage, MCP, web view; design page updates; strengthen the heuristics-arm plan).
3. Gate 2 proper: sources (Wikipedia SSE with reconnect/replay, Kafka assign + offsetsForTimes,
   stdin), the log, the pure fold with golden replay (fitness function #3: replay determinism),
   evidence table + simple graph view, `--json`, read-only MCP. Gate 2 is done when
   restart/duplicates/gaps/ordering are verified and a fresh Claude Code session sets it up on a
   local topic from one sentence. Demo every sprint.
4. Design page: carry the contract's corrections (outcome-source seam: MediaWiki history, not the
   revert model; revert provenance) into the artifact.

## Standing decisions (from the earlier handoff, still true)
Foundation in pair mode here, then S2W becomes the primary track in the main sprint loop; NL every
other sprint; primary until gate 3 or 2026-10-11. First audience HN/X, Kafka users. Open source
(permissive) at launch. Every kept seam ships with two real implementations in the first slice.
Dashboard visualization as important as the LLM interface. Review findings still to honour while
building: see the design page and `docs/reviews/`.
