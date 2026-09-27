# Evaluation contract (gate 1)

Status: **DRAFT, 2026-09-27.** Items marked **⟨Dave⟩** are his to set. Everything else is a
proposal he can overrule. Once signed, this file is frozen: a change after sign-off is a new
dated section with its reason, never an edit in place. No gate-3 or gate-4 result counts unless
it was measured under the version of this contract in force when its test window opened.

The first slice asks two questions, and this contract fixes how each is graded:

- **Part A (gate 4):** can `s2w` issue forecasts that earn a graded record on a live stream?
- **Part B (gate 3):** does System 2 make an unfamiliar stream useful faster than heuristics alone?

---

## Part A: the forecast question

### A1. The question

> **Q-revert-30m:** will this edit be the target of a revert that MediaWiki recognizes, made
> within 30 minutes of the edit?

- **Recognized revert:** MediaWiki adds the `mw-reverted` tag to the edit. The tag covers undo,
  rollback and manual reverts (exact restoration of an earlier revision), up to
  `$wgRevertedTagMaxDepth` edits deep. The reverting edit itself is never tagged.
- **Revert time:** the `rev_timestamp` of the reverting revision, which is the first later
  revision on the same page tagged `mw-rollback`, `mw-undo` or `mw-manual-revert`. The tag
  arrives later, from a job (`revertedTagUpdate`), so tag arrival time is only an upper bound on
  revert time.
- **Horizon:** 30 minutes from the edit's `rev_timestamp`.

Measured 2026-09-27, all Wikipedias, 2.5-minute sample, n=55 newly tagged edits: 29 (53%) were
tagged within 30 minutes of the edit, and the tail ran to weeks. So 30 minutes captures about
half of eventual reverts and grades quickly. The sample is small, and the English-only
measurement below replaces it.

### A2. Eligible edits (Dave, 2026-09-27: English only)

**English Wikipedia, article namespace (0), edits to existing pages, performer not
flagged as a bot.**

- Excluding page creations matches Wikimedia's revert-risk model, which does not score first
  revisions.
- Excluding bot edits removes near-certain negatives that would inflate every predictor's score
  equally and tell us nothing.
- Anonymous and temporary-account edits stay in. They carry most reverts.
- **English only** keeps one set of patrol norms. Wikis that run Automoderator revert edits
  *because of* the revert-risk score, which would make that baseline partly predict its own
  effect. English Wikipedia's fast reverter is ClueBot NG, a different model; see A7.
  (To check before sign-off: that English Wikipedia still does not run Automoderator.)

**English only is a scoping choice for this measurement, not a limit of `s2w`.** `s2w`
prioritizes multilingual streams: System 1's embeddings and System 2's models must handle any
language, and Part B's Wikipedia stream already carries every language edition. After the
slice, Part A is re-run on at least two non-English Wikipedias under this same contract, and
any result we publish says the forecast was measured on English first.

### A3. Issuance: one forecast per edit, at a declared cutoff

- Every eligible edit gets **exactly one issuance** per predictor. No re-forecasting, so no
  entity gets extra weight from being asked about repeatedly.
- **Cutoff:** 15 seconds after `s2w` receives the edit event. Predictors may use evidence that
  arrived before the cutoff and nothing after it. The 15 seconds let Wikimedia's own score
  arrive: it trailed the edit by about 6 seconds in the 2026-09-27 sample.
- **Already reverted at cutoff:** if the revert lands before the cutoff (ClueBot NG often
  reverts within seconds), the edit is **ineligible**. It is counted and reported, not scored,
  because its outcome was visible at issuance. All predictors lose the same edits.
- The issuance record is immutable: question, edit, issue time, horizon, probability, predictor
  and version, evidence cutoff (log offset). Repairs, replays and restarts never change it.

### A4. Outcomes, finalization and censoring

- **Provisional label at T+60 min** (30-minute horizon plus 30 minutes for the tag job).
  Positive if the edit carries `mw-reverted` and the revert time is ≤ T+30 min. Otherwise
  provisionally negative.
- **Final label at T+24 h.** A tag that arrives late for a revert inside the horizon appends a
  corrected outcome observation. Nothing is edited in place, and both versions are reported.
- **Two outcome sources**, one per implementation of the outcome seam: (1) the in-stream
  `revision-tags-change` events, pushed; (2) the MediaWiki Action API revision history for the
  page at T+24 h, pulled. They should agree. Disagreements are counted, and an edit the two
  sources disagree on is censored.
  *Correction to the design page:* it names Wikimedia's revert model as the second outcome
  source. That model is a predictor, so it belongs among the baselines (A5), not the outcomes.
- **Censored, not negative:** the page or revision is deleted or hidden within the horizon; a
  stream gap covers the horizon and resuming from the last event id cannot fill it; or the two
  outcome sources disagree. Censored edits are excluded from scores, counted, and reported per
  predictor, so a predictor cannot gain from censoring.
- **Missing labels are not negatives** until the edit reaches T+60 min.

### A5. Baselines

Each baseline is fitted or calibrated only on the **development window**, never on the test
window.

| id | Baseline | What it controls for |
|---|---|---|
| B0 | Base rate: the development window's positive rate | Whether there is any skill at all |
| B1 | Contextual: logistic regression on 4 features (anonymous/temporary account, account age bucket, byte-size change, empty edit summary) | Whether `s2w` does more than the obvious features |
| B2 | Wikimedia `revertrisk-language-agnostic` (currently v3), read from the `mediawiki.page_revert_risk_prediction_change.v1` stream. Reported raw and recalibrated (isotonic fit on the development window), because its training target (revert window, label definition) is not published and may differ from 30 minutes | A dedicated model built for this exact job |

Coverage matters: B2 is missing when its score has not arrived by the cutoff. Missing scores are
reported. The head-to-head comparison with B2 uses only edits where every predictor has a
forecast.

### A6. Scoring

- **Primary metric: Brier skill score (BSS) against B0**, with a 95% interval from a bootstrap
  clustered by page. Edits on one page are correlated (edit wars, vandalism sprees), and treating
  them as independent would make the intervals look narrower than they are.
- Secondary metrics: log loss; area under the precision–recall curve (reverts are rare);
  a reliability chart and expected calibration error (10 equal-count bins); coverage; the
  censored and ineligible counts.
- **Abstention:** a predictor may abstain. For the primary score, an abstention is replaced by
  the B0 probability, so abstaining can never raise skill. Its accuracy on the edits it did
  answer, and the share it answered, are reported separately.
- **Sensitivity slice:** the same metrics excluding reverts made by bots (for example ClueBot NG),
  which shows how much of the score is really predicting another model's decision.

### A7. Limits on what a result may claim

- On English Wikipedia a large share of 30-minute reverts come from ClueBot NG. Skill there is
  partly skill at predicting ClueBot NG. The A6 sensitivity slice bounds that share.
- `mw-reverted` misses partial reverts and reverts deeper than the max depth. The label is
  "MediaWiki recognized a revert", not "the edit was bad".
- One wiki, one question. Nothing here generalizes to other streams without Part B.

### A8. Windows and freezing

- **Development window:** any data before the freeze. Build, tune and fit baselines here.
- **Freeze:** a commit hash, a config hash and the B0 to B2 fits recorded in the ledger.
- **Test window:** 7 consecutive days starting at least 24 hours after the freeze, issued **live**.
  Forecasts are made as the edits arrive, so no result can use future data. A replay of the
  same days may be run to check determinism; the live run is the one that counts.

### A9. Gate-4 pass thresholds (Dave, 2026-09-27: report B2, don't require it)


1. BSS against B0 > 0, and the lower bound of the 95% interval > 0.
2. BSS against B1 > 0 (point estimate). We beat the obvious features.
3. Expected calibration error ≤ 0.02.
4. **B2 is reported, not required.** A model built for this one job should win on its home
   stream. The claim under test is that `s2w` earns a record on a stream with little setup, not
   that it beats Wikimedia. We report the gap and do not spin it.

Fail if 1 or 3 is missed. If 2 is missed, report it as a fail of the "more than obvious
features" claim.

---

## Part B: does System 2 earn its place?

### B1. The comparison

Same streams, same windows, two arms:

- **H:** heuristics alone (System 1 rules and local embeddings).
- **H+S2:** the same heuristics plus **one budget-capped System 2 pass** that proposes types,
  identity keys, relationships and repairs. Each proposal is inert until accepted. For this
  test, one scripted evaluator accepts or rejects proposals against the answer key, and the time
  a human would spend on each correction is estimated with the same rule in both arms.
- **B3, a raw-sample LLM baseline:** the same model, given N raw events in one prompt, asked to
  describe entities, identity keys and relationships. If `s2w` cannot beat this, the machinery
  is not earning its keep.

Run on both System 2 providers (a hosted API; the client agent via MCP sampling or a local
model), with each run's token cost recorded.

### B2. Streams

1. **Wikipedia** (the `recentchange` stream), plain.
2. **Wikipedia, obfuscated:** every field name renamed to `f1…fN`, and every identifier, name,
   URL, title and free-text value replaced by a salted keyed hash that stays consistent within
   the run. Numbers, timestamps and structure unchanged. The salt and mapping are sealed until
   scoring.
3. **A private stream Dave owns and can label:** the lifeos dev-worker and sprint log (legs,
   issues, pull requests, review seats, merges; from the status database and git). Default
   chosen 2026-09-27; Dave may swap it. It must be one no model has seen, with at least two
   entity types and one relationship.

   *Why private when `s2w` is open source:* the question is what the model has already read,
   not whether our code is public. Wikipedia's streams and schemas are in LLM training data.
   Even obfuscated, their structure, value ranges and timing can be recognized rather than
   inferred. A stream that was never public is the only fair test of the product claim, since
   a user's own Kafka topic is private too. We publish the results and a summary of the answer
   key, never the raw events.

### B3. Answer keys and metrics

- An **answer key** per stream, written and committed before the test window opens: entity types,
  identity key per type, relationships, and the fields that carry state.
- Metrics per arm per stream:
  - field-role accuracy;
  - identity precision and recall;
  - **false-merge rate** (distinct real entities merged into one; the headline safety metric);
  - relationship precision and recall;
  - abstention rate;
  - estimated correction minutes to reach the answer key;
  - System 2 cost in dollars.
- Mappings are **frozen after a development window**, then scored on a later test window
  (≥ 24 hours later), so a mapping cannot be tuned to the data it is graded on.

### B4. Gate-3 pass thresholds ⟨Dave⟩

Proposed. System 2 earns its place if, on **the obfuscated stream and the private stream**:

1. H+S2 improves mean identity and relationship F1 over H by at least 0.10 absolute, **or** cuts
   correction minutes by at least 30%;
2. and H+S2's false-merge rate is no higher than H's;
3. and H+S2 beats B3 on F1 or correction minutes;
4. within a System 2 budget of **$5 per stream per pass**.

The plain Wikipedia stream is reported but does not count: the model has read its schema.

---

## Kill criteria and the time cap

- **Stop and re-decide** if gate 3 fails: System 2 does not beat heuristics and B3 on accuracy
  or effort.
- **Stop and re-decide** if the launch does not get people to run it on their own streams.
- **Time box:** gate 2 in one week of sprints. Overall cap for the slice: **60 hours of sprint time** (about 30 sprints; Dave, 2026-09-27), counted from the first gate-2 sprint. This
  sits alongside the standing cap (primary until gate 3 or 2026-10-11, whichever comes first).
- A failed gate is a result, not a reason to relax this contract.

## Sign-off

| Item | Decision | By, when |
|---|---|---|
| Eligible population (A2) | English Wikipedia, with a multilingual re-run after the slice | Dave, 2026-09-27 |
| Gate-4 thresholds (A9) | As proposed; Wikimedia's model reported, not required | Dave, 2026-09-27 |
| Private stream (B2.3) | lifeos dev-worker and sprint log (default; swappable) | karpathy default, 2026-09-27 |
| Gate-3 thresholds and budget (B4) | ⟨pending⟩ | |
| Overall hours cap | 60 hours of sprint time | Dave, 2026-09-27 |
| Base rate and cutoff (A3, A5) | ⟨pending: measured English sample⟩ | |
