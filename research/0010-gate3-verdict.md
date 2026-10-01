# 0010: Gate 3 verdict (s2w#374)

- **Question:** does System 2 on top of the heuristics (H+S2) beat H and the raw-sample baseline (B3) by
  the margins contract [B4](../docs/evaluation-contract.md) sets, on the obfuscated stream and on the
  private stream, each on its own?
- **Feeds:** gate 3, the kill criterion "stop and re-decide if gate 3 fails", decision
  [0032](../docs/decisions/0032-gate3-committed-mapping.md), epic #13.
- **Date:** 2026-10-01. **Provider:** `claude-sonnet-5-5` only, so any claim is for that provider.
- **Numbers:** every figure below is read from `research/h-measure/results/gate3/summary.md`, which
  `research/h-measure/gate3-summary.ts` generates from the 30 `--json` score reports and the 20
  committed mappings. Scoring is local and cost $0. The preconditions in the Verdict paragraph (no
  failure, reported model, ordering) are checked from the committed files, `ledger.md` and `git log`,
  not by the script.

## Verdict

**Gate 3 fails.** The obfuscated stream fails and the private stream fails, so the contract's "each
stream must pass on its own" is not met. No replicate was unmeasurable: all 20 committed files are
mappings (no failure), every reported model is the configured snapshot, and no test span was opened
before the predictions comment (issuecomment-5938191252) or before the 20 mappings reached `main`
(596ea37). Git ancestry orders it: pins, freezes (#430), mappings (#432), predictions, openings
(2842008), scores (a0a1237).

| B4 item | obfuscated | private |
|---|---|---|
| 1 primary: mean F1 gain over H >= 0.10, higher in all 5 | **fail**: H 0.6011, H+S2 0.3534, gain -0.2477, higher in 0 of 5 | pass: H 0.0044, H+S2 0.6346, gain 0.6303, higher in 5 of 5 |
| 2a false-merge <= 0.05 every replicate | pass (max 0.0111) | pass (0.0000) |
| 2b mean false-merge <= H + 0.02 | pass | pass |
| 2c mean relationship F1 no more than 0.05 below H | fail (H's is undefined) | fail (H's is undefined) |
| 3 mean entity recovery >= 0.60 | **fail**: 0.0705 | **fail**: 0.1982 |
| 4 mean F1 gain over B3 >= 0.05, higher in >= 4 of 5 | pass: gain 0.1017, 5 of 5 | **fail**: gain 0.0036, 2 of 5 |
| 5 budget (h-s2 + B3 <= $5 per replicate, no failure) | pass | pass |

How to read the rows:

- **Obfuscated, first failing item: 1.** H+S2 is below H in every replicate.
- **Private, first failing item: 2c by the plan's convention** (an undefined relationship F1 counts as
  a fail for the item that uses it). H's edge F1 is undefined on both streams (obfuscated: 44,564 predicted edges, none matches the key, so
  P and R are 0.0000; private: no edges predicted), so 2c cannot pass under that convention. Read an
  undefined edge F1 as 0 instead and 2c passes on both streams: the private first failing item
  becomes 3, clause 41 becomes a hit (Gate row 5 of 5, 3 of 10 rows hit overall). The gate verdict is
  the same either way. Items 3 and 4 fail on measured numbers
  regardless, so the verdict does not depend on the convention.
- **Not "insufficient headroom".** H's mean F1 is 0.6011 and 0.0044, both below 0.90, so the margin
  exists on both streams.
- The private stream's item 1 passes only because H scores 0.0044 there: H's private mapping reaches
  recall 0.0022 only. H+S2's F1 of 0.6346 is within 0.004 of B3's 0.6310, so on the private
  stream the model's mapping adds nothing over a raw sample (item 4).

## Results

Per replicate rows are in `research/h-measure/results/gate3/summary.md`; raw reports are
`results/gate3/<arm>.<stream>.rK.{md,json}`. Means over r2 to r6:

| stream | arm | F1 | P | false-merge | recovery | edge F1 | usd |
|---|---|---|---|---|---|---|---|
| obfuscated | H | 0.6011 | 0.9913 | 0.0087 | 0.9819 | undefined | 0 |
| obfuscated | H+S2 | 0.3534 | 0.9894 | 0.0106 | 0.0705 | 0.2470 (4 of 5 defined) | 0.8150 |
| obfuscated | B3 | 0.2518 | 0.7223 | 0.2777 | 0.0391 | 0.3132 (1 of 5) | 1.7930 |
| private | H | 0.0044 | 1.0000 | 0.0000 | 0.0000 | undefined | 0 |
| private | H+S2 | 0.6346 | 1.0000 | 0.0000 | 0.1982 | 0.5527 (5 of 5) | 0.5073 |
| private | B3 | 0.6310 | 1.0000 | 0.0000 | 0.1096 | 0.5723 (5 of 5) | 1.0142 |

Observations (facts from the reports; causes untested):

- **H on the obfuscated stream scores 0.6011, not the 0.81 it scores on plain `reserved-6`.** Its
  per-type rows differ materially from the plain run in one type: `wiki` has recall 0.5598 on plain
  `reserved-6` and 0.0000 on every obfuscated replicate (page and revision differ by under 0.001),
  and none of H's 44,564 predicted relationship edges on the obfuscated stream matches the key (edge
  P and R 0.0000, r2) where the plain run had edge P 0.4643, R 0.2350. H is
  identical in all five obfuscated replicates (one mapping per freeze, same score), so this is
  not noise.
- **H+S2 recovery on the obfuscated stream is 0.0705**, against H's 0.9819. H+S2 reaches `page`
  recall 0.25 (H: 0.9987) and `revision` recall 0.0 (H: 1.0) in r2: the model's mapping dropped
  types H keeps.
- **Replicate spread is low.** H+S2 obfuscated F1 is 0.3295 in r2 to r5 and 0.4492 in r6; B3
  obfuscated is 0.2324 in r2 to r5 (P 0.6556, false-merge 0.3444) and 0.3295 in r6. The five
  replicates are less independent than five fresh runs suggest: four of five H+S2 files and four of
  five B3 files score identically.
- **B3's obfuscated false-merge rate is 0.3444 in four replicates.** B3 is not under any floor in
  B4 (item 2 is H+S2's), so this is reported, not graded.

## Predictions (issuecomment-5938191252), scored

Each clause of the posted prediction table is graded hit or miss in
`results/gate3/summary.md` ("Predictions, clause by clause", 44 clauses). Roll-up:

| prediction row | clauses hit | row |
|---|---|---|
| H obf | 3 of 6 | miss |
| H+S2 obf | 2 of 5 | miss |
| B3 obf | 3 of 5 | miss |
| H private | 5 of 5 | **hit** |
| H+S2 private | 2 of 3 | miss |
| B3 private | 0 of 2 | miss |
| No-match | 5 of 6 | miss |
| Spend | 2 of 5 | miss |
| Probe | 2 of 2 | **hit** |
| Gate | 4 of 5 | miss |

**28 of 44 clauses hit; 2 of 10 rows hit.** The gate point estimate (fail) was a hit, and so was
item 1 failing on the obfuscated stream and passing on the private one. The misses that matter:

- **H obf F1 0.6011 against [0.78, 0.84], and edge P/R 0.0 against [0.35, 0.55] / [0.15, 0.30].**
  The prediction took H's plain-`reserved-6` numbers as a proxy; obfuscation changes H's result, not
  only its notation.
- **H+S2 obf mean F1 0.3534 against [0.55, 0.85]; mean recovery 0.0705 against [0.60, 0.95].** Four of
  five replicates score F1 0.3295.
- **B3 private was below H+S2 in only 2 of 5 replicates** (higher in 2, tied in 1; the prediction
  was >= 3), and B3 private mean F1 0.6310 is above its band's top, 0.60.
- **Obfuscated item 2 fails, predicted to pass**, only because H's edge F1 is undefined; items 2a
  and 2b pass.
- **Spend was lower than predicted**: h-s2 $0.0814 to $0.1668 per replicate against $0.15 to $0.35,
  B3 $0.1729 to $0.4765 against $0.40 to $1.50, run total $4.2154 against $6 to $20. No miss here
  costs anything.
- **No-match:** h-s2 obfuscated had 0 pre-repair no-matches of 5 against the predicted 1 to 2.

Nothing was re-tuned. These are findings.

## Disclosed flows and weaknesses

- **Test-to-dev field-number count (ruling Q1, s2w#374 PR 1).** Each replicate's field table is
  ranked over the paths of both windows. `reserved-6` has **18** paths under `data.log_params` that
  `dev` lacks (17 leaves and the `log_params.restrictions` object), so 18 field numbers never occur
  in an obfuscated dev window. This reveals a count only: no names, no values. `dev-key-v3.json`
  declares `log_params` an unscored prefix, so no scored row touches these paths. Option B (a table
  that can gain fields) is a follow-up, not done.
- **Plain `reserved-6` was already open** (s2w#375 PR 2). The obfuscated test span's blindness rests
  on the unseen replicate key and field table, which the held-out metadata (`obfuscation/rK.meta.json`,
  committed by this PR after every mapping) provide, not on the underlying events being unseen.
- **Clean-session probe weakness (s2w#431).** The probe asks the model whether it can see anything
  besides its built-in prompt; all 20 answered `none`, but the probe cannot prove absence of what it
  does not observe. s2w#431 tracks the weakness; this run does not strengthen the claim.
- **Replicates are not independent fits.** Identical scores across r2 to r5 (above) mean the
  "consistency across all 5" rule saw fewer than five distinct outcomes for several arms.

## Spend

The ledger (`results/gate3/ledger.md`) gives a run total of **$4.2154** (ceiling $100): $4.1295 in the
20 committed files (h-s2 $1.3223, B3 $2.8072, committed probes included) plus $0.0859 of failed-probe
spend that sits in no committed file. The brief's $4.22 is the run total. Scoring: $0, no model call. No file is
above the $5 per-file gate.

## Publication checklist (contract §Publication)

| Item | Where |
|---|---|
| scoring code and public-data manifest | `crates/xtask` (`h-measure score`), `research/h-measure/corpora.toml`, `keys.toml` |
| every issuance with hourly digests | not applicable to gate 3 (gate 4's forecasts); `capture.log` records captures |
| eligibility manifest, exclusions, denominators | `corpora.toml` stanzas (counts, dropped rows), per-report record counts |
| every registered run, failed and unmeasurable included | `research/h-measure/committed/*.json` and transcripts (20 files, 0 failures, 0 unmeasurable); the s2w#402 dry runs in decision 0032 |
| matcher and scorer fixtures | `crates/xtask` tests, `research/h-measure/dev-key-*.json` |
| obfuscation and canonicalization rules, identifier domains | `obfuscation/recentchange.rules.toml`, `obfuscation/r{1..6}.meta.json` |
| sanitized private-stream fixtures | `research/h-measure/private/fixture/` |

## Design implications

1. **Gate 3 fails: stop and re-decide** (contract, kill criteria). The decision is Dave's. →
   deferred: karpathy to bring Dave; no code change here.
2. **H's `wiki` and edge behaviour under obfuscation** (F1 0.6011 against 0.8104 on plain data) is the
   biggest single number in this run and has no explanation yet. → deferred: follow-up to be
   filed by karpathy if Dave continues the slice.
3. **Obfuscated `recovery` for H+S2 (0.0705) and the dropped types** suggest the System 2 mapping
   prompt loses types H keeps. → deferred: same follow-up.
4. **Option B for Q1** (a table that can gain fields) → deferred: filed per ruling Q1 (karpathy
   tracks).
