# 0004: Revert pilot on English Wikipedia

- **Question:** what do the contract's Part A quantities look like on real data: base rate,
  revert and arrival delays, how many edits the cutoff removes, and how Wikimedia's revert-risk
  score (B2) behaves on our 30-minute question?
- **Date:** 2026-09-27 · **Researcher:** karpathy · **Status:** done
- **Feeds:** evaluation contract A10; the A8 test-length estimate; gate 4 design.

## Method

Replayed `mediawiki.page_change.v1` and `mediawiki.page_revert_risk_prediction_change.v1` for
12:00–13:05 UTC on 2026-09-27 (05:00–06:05 MST, a Sunday morning) with
[`scripts/eventstreams_replay.py`](scripts/eventstreams_replay.py). No reconnects were needed.
The cohort is English Wikipedia article-namespace edits to existing pages by non-bot editors with
`rev_dt` in 12:00–12:30 UTC; the 35 minutes after it cover every 30-minute horizon plus delivery.
Analysis: [`scripts/revert_pilot.py`](scripts/revert_pilot.py).

Pilot approximations, all replaced by the contract's matcher and recorder: receipt time is
approximated by `meta.dt`; revert ranges are compared by `rev_id` rather than
`(rev_timestamp, rev_id)`.

## Results

| Measure | Value |
|---|---|
| Revisions in the window (all namespaces) | 6,378, of which 331 reverts (149 undo, 102 rollback, 80 manual) |
| Cohort | 1,864 edits |
| Reverted before the 15-second cutoff (ineligible) | 10 |
| Eligible | 1,854 |
| **Positives (reverted within 30 min)** | **71, base rate 3.8%** |
| Temporary and anonymous editors | 256 eligible, 46 positive (18%) |
| Edit to revert | p25 0.8 min, median 1.8 min, p90 8.8 min |
| Reverts made by bot accounts, after the cutoff | 0 of 71 |
| Self-reverts | 8 of 71 |
| Edit arrival delay (`meta.dt − rev_dt`) | median 2.8 s, p99 20 s, max 38 s, none over 60 s |
| B2 coverage | 1,728 of 1,854 (93%), every one before the cutoff; lag median 3.1 s, p90 8.2 s |
| B2 raw | mean probability 0.40, Brier 0.204 vs 0.037 for the base rate |
| B2 discrimination | ROC AUC 0.888 |

One 30-minute window on a Sunday morning: these are orders of magnitude, not estimates for the
test. The development window measures them properly.

## What it shows

- **The question is well posed.** Reverts within 30 minutes are common enough (3.8%) to grade
  quickly, and most happen within 10 minutes, so the horizon captures the fast-revert process.
- **The cutoff removes few edits** (10 of 1,864). The conditional population in A3 is almost the
  whole population.
- **ClueBot NG did not appear after the cutoff in this window.** Bot reverts are either before the
  cutoff (the 10 ineligible edits) or rare here. A7's caveat that skill is "partly skill at
  predicting ClueBot NG" is not supported by this sample; the development window should measure
  it before the claim is repeated anywhere.
- **B2 ranks well and is badly calibrated for this question.** Raw, it averages 0.40 against a
  3.8% base rate. Its training target is evidently different (likely a longer revert window).
  Recalibrated, it will be a strong comparison: AUC 0.888 is the number to beat.
- **Power is not the constraint.** About 1,850 eligible edits per half hour gives on the order of
  10⁴ positives a week. The contract's 7-day minimum will bind, not the 21-day maximum.
- **The 60-second arrival limit excluded nothing here.** Live delivery may differ; the recorder
  measures it.

## Design implications

1. B2 must be recalibrated before any comparison; reporting only raw B2 would be a straw man.
   → adopted: contract A5 already reports B2 raw and recalibrated (v2).
2. A7's ClueBot NG caveat is unmeasured. → deferred: measure the bot share of within-horizon
   reverts in the development window, and amend A7's wording then (tracked in contract A10).
3. Temporary and anonymous editors are 14% of eligible edits and 65% of positives, so account
   type will dominate B1. → adopted: B1 already includes the account-type feature; the heuristics
   arm must beat B1, which this makes a real bar.
4. The test length will be the 7-day minimum. → adopted: A8 unchanged; the power estimate is still
   run on the development window.
5. Streams disconnected after about 6 minutes on a live connection earlier today (two of two
   collectors). → adopted: the contract's recorder must reconnect by Last-Event-ID and replay gaps
   (A3, A4); `scripts/eventstreams_replay.py` shows the reconnect pattern.
