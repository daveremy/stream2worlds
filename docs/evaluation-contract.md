# Evaluation contract (gate 1)

Status: **DRAFT v2, 2026-09-27.** v1 was reviewed by Codex (gpt-6-astra), verdict "revise before
sign-off" ([review](reviews/gate1-contract-round1-codex.md)). v2 answers every finding; the
change log at the end maps each one. Items marked **⟨Dave⟩** are his to set. Everything else is
a proposal he can overrule.

Once signed, this file is frozen. A change after sign-off is a new dated section with its reason,
never an edit in place. No gate-3 or gate-4 result counts unless it was measured under the
version of this contract in force when its test window opened.

The first slice asks two questions, and this contract fixes how each is graded:

- **Part A (gate 4):** can `s2w` issue forecasts that earn a graded record on a live stream?
- **Part B (gate 3):** does System 2 recover the structure of an unfamiliar stream better than
  heuristics alone, and better than simply showing an LLM raw events?

Part B tests structure recovery, not forecasting. Passing it says nothing about forecasting on
other streams.

**This contract is also a draft schema.** Sections A1 to A8 record what any registered forecast
question needs: the question, who is eligible, the cutoff, the horizon, the outcome sources,
what counts as censored, the baselines and the scoring. Gate 4's `forecast.register` should
capture those same fields, so a user registers a question in the shape we used to evaluate
`s2w` itself. Differences found while building gate 4 are recorded here as a dated section.

---

## Part A: the forecast question

### A1. The question

> **Q-revert-30m:** among English Wikipedia edits that are still unreverted 15 seconds after
> they are received, will the edit be reverted within 30 minutes of being made?

- **Reverted** is decided from **revert provenance**, not from tags. Every revision in the
  `mediawiki.page_change.v1` stream that MediaWiki recognizes as a revert carries a `revert`
  object: `method` (rollback, undo or manual), `is_exact`, `rev_original_id`,
  `rev_reverted_oldest_id` and `rev_reverted_newest_id`. Edit E is reverted by revision R when R
  is on E's page and E's revision id lies in R's reverted range. This names the exact reverting
  revision, so a revert of some other edit on the same page never counts. (Verified on the live
  stream, 2026-09-27.)
- **Within 30 minutes:** `R.rev_dt − E.rev_dt ≤ 30 min`.
- The provenance arrives with R itself, when the revert happens. The label therefore does not
  depend on MediaWiki's later `revertedTagUpdate` job, which can be skipped (deep reverts, a
  reverting edit that is itself reverted, approval rules). The `mw-reverted` tag is used only
  as an audit (A4).
- Self-reverts (R by E's own performer) count, since MediaWiki recognizes them. Their share is
  reported.
- Measuring the pilot (to replace v1's 2.5-minute sample): an inception cohort of English edits,
  with edit-to-revert and revert-to-tag delays measured separately. Results go in A10 before
  sign-off.

### A2. Eligible edits (Dave, 2026-09-27: English only)

**English Wikipedia, article namespace (0), edits to existing pages, editor not flagged as a
bot.**

- Excluding page creations matches Wikimedia's revert-risk model, which does not score first
  revisions.
- Excluding bot edits removes near-certain negatives that would inflate every predictor's score
  equally and tell us nothing.
- Anonymous and temporary-account edits stay in. They carry most reverts.
- English Wikipedia is absent from the official Automoderator deployment list (checked
  2026-09-27). Automoderator reverts edits *because of* the revert-risk score, which would make
  that baseline partly predict its own effect. The deployment list and English Wikipedia's
  anti-vandalism bot configuration are archived at the freeze.

**English only is a scoping choice for this measurement, not a limit of `s2w`.** `s2w`
prioritizes multilingual streams: System 1's embeddings and System 2's models must handle any
language, and Part B's Wikipedia stream already carries every language edition. After the
slice, Part A is re-run on two non-English Wikipedias under this same contract. **The two wikis
are named at the freeze**, and any result we publish says the forecast was measured on English
first.

### A3. Issuance

- **Ingress recorder.** A separate process records every raw stream event with its first-receipt
  time before any predictor sees it. Receipt time means that recorder's clock, never a
  predictor's dequeue time.
- **Eligibility manifest.** The evaluator builds one list of eligible edits from the recorder's
  log, identical for every predictor and independent of their outputs. An edit is eligible only
  if it arrived **within 60 seconds of its `rev_dt`**. Late arrivals (stream lag, reconnects) are
  ineligible for everyone and counted.
- **Cutoff and deadline.** Predictors may use evidence the recorder received up to 15 seconds
  after the edit's receipt. Each probability must be **committed by that same moment**. A
  commitment after the deadline counts as missing and is replaced by the fallback (A6).
- **Already reverted by the cutoff:** the edit is ineligible for everyone, and counted. So the
  question is conditional: risk among edits that survive unreverted to the cutoff. ClueBot NG
  often reverts within seconds, so this removes many of the easiest positives. The contract
  states it rather than hiding it.
- **Exactly one issuance per edit per predictor.** The issuance record is immutable: question,
  edit, issue time, horizon, probability, predictor and version, evidence cutoff (recorder
  offset). Repairs, replays and restarts never change it.
- **Public timestamps.** Every hour, a digest of that hour's issuances is committed and pushed to
  a public repository. A local ledger alone is not independent evidence of when a forecast was
  made.

### A4. Outcomes, ascertainment and censoring

- **Primary outcome source:** revert provenance from `mediawiki.page_change.v1` (A1).
- **Ascertainment deadline:** T+45 min (30-minute horizon plus 15 minutes for stream delivery).
  The label is fixed at the deadline.
- **Gap recovery.** If the recorder has a gap overlapping an edit's horizon, the evaluator
  re-consumes that interval from the stream (EventStreams can replay recent history by
  timestamp) before the deadline. A gap that cannot be refilled censors the affected edits.
- **Audit, not a second truth.** At T+7 days, the evaluator checks `mw-reverted` tags (stream
  and Action API) against the provenance labels. Tags and provenance come from the same MediaWiki
  system, so agreement shows delivery was consistent, not that the label is true. Disagreements
  are reported by kind (a tag without provenance, provenance without a tag). They change no
  label.
- **Censoring is owned by the evaluator**, decided from the recorder log and page state, never
  from forecasts. An edit is censored when the page or revision is deleted or hidden before
  its label can be established, or when a recorder gap cannot be refilled.
- **Censoring can still bias results**, even though it is applied to every predictor alike: it
  may remove the cases one predictor gets wrong. So the evaluator reports:
  - censored and ineligible counts by hour, by page activity, by predictor score range and by
    editor type;
  - a worst-case analysis: every censored edit labelled against each predictor in turn, and
    whether the headline result survives.

  **If more than 2% of eligible edits are censored, the run fails as unmeasurable** rather than
  passing or failing on skill.

### A5. Baselines

Each baseline is fitted or calibrated only on development-window edits whose labels were
ascertained before the freeze.

| id | Baseline | What it controls for |
|---|---|---|
| B0 | Base rate: the development window's positive rate, frozen | Whether there is any skill over prevalence |
| B1 | Contextual: logistic regression on 4 features (anonymous/temporary account, account-age bucket, byte-size change, empty edit summary). Feature definitions, missing-value handling and regularization frozen at the freeze | Whether `s2w` does more than the obvious features |
| B2 | Wikimedia `revertrisk-language-agnostic` (v3 as of 2026-09-27), read from `mediawiki.page_revert_risk_prediction_change.v1`. Reported raw and recalibrated (isotonic fit on the development window) | A specialist revert model. Its training target (revert definition, time window) is in its linked training repositories, not yet checked; it is not assumed to match this question |

**Input policy.** The primary `s2w` arm may **not** use B2, or any other moderation or revert
score, as an input. Otherwise `s2w` could pass by recalibrating Wikimedia's score, which would
show integration, not forecasting. An optional secondary arm that does use B2 is reported
separately and never counts toward the gate.

**B2 coverage.** The head-to-head with B2 uses exactly the edits where B2's score arrived before
the cutoff. That subset is chosen by B2's availability alone, never by whether another predictor
answered.

### A6. Scoring

- **What is scored is the committed product: predictor plus fallback.** When the predictor
  abstains or misses the deadline, the fallback issues the B0 probability. Every eligible,
  labelled edit is scored. All abstentions together earn exactly zero skill, so abstaining
  cannot pass the gate by itself. Selective abstention can still help the score, and that is a
  legitimate property of the product, so it is reported: the share of genuine answers, and
  skill on those answers alone.
- **Primary metric: Brier skill score against B0,**
  `BSS = 1 − Σ(pᵢ − yᵢ)² / Σ(bᵢ − yᵢ)²`, over the same edits.
- **Intervals:** a paired bootstrap on the same edits for every predictor, clustered by page,
  2,000 replicates, percentile 95% intervals. Fitted predictors stay fixed. Both sums are
  recomputed in every replicate.
- **Dependence checks**, reported alongside: the same bootstrap clustered by editor, and a
  day-block bootstrap. The headline interval is the widest of the three.
- **Secondary metrics:**
  - log loss;
  - area under the precision–recall curve (discrimination, reported separately from BSS);
  - calibration-in-the-large: observed versus expected positives, with an interval;
  - a reliability chart with intervals, including the top-risk decile on its own.
- **Bot-revert decomposition (descriptive):** paired loss differences split additively into
  bot-reverted positives, human-reverted positives and negatives. This describes where each
  predictor's advantage comes from. It is not a causal estimate of a world without bots.

### A7. Limits on what a result may claim

- On English Wikipedia, many fast reverts come from ClueBot NG. Skill here is partly skill at
  predicting ClueBot NG. The A6 decomposition shows how much of the gain comes from bot-reverted
  positives.
- The label is "MediaWiki recognized a revert of this edit", not "the edit was bad". Partial
  reverts that MediaWiki does not recognize count as negatives.
- One wiki, one question, one conditional population (edits surviving the cutoff).

### A8. Windows, freezing and runs

- **Development window:** data before the freeze. Build, tune and fit here, using only labels
  ascertained before the freeze. Historical features are reconstructed as they stood at each
  edit's cutoff, never from later API state.
- **Power:** before the freeze, estimate from development data how many test days give a 95%
  interval narrow enough to detect a BSS of 0.02 over B0. The test length is that number,
  **at least 7 and at most 21 days**, fixed at the freeze.
- **The freeze records:**
  - the exact test start and end, which start at least 24 hours after the freeze;
  - commit hash and config hash;
  - prompts, model identifiers and versions, preprocessing, calibrators, the fallback policy and
    the scoring code;
  - the B0 to B2 fits;
  - the archived Automoderator and bot configuration;
  - the two non-English wikis for the later re-run.
- **No parameter learning during the test.** World state keeps accumulating from the stream, as
  it would in use. Parameters, prompts and calibrators do not change.
- **Every run is registered.** A fix made after seeing test results needs a new test window, and
  the earlier run is kept and published.
- The test is issued **live**. A replay of the same days may be run to check determinism; the
  live run is the one that counts.

### A9. Gate-4 pass thresholds (Dave, 2026-09-27: report B2, don't require it)

1. **Skill over prevalence.** BSS against B0 > 0, and the lower end of its 95% interval > 0.
2. **More than the obvious features.** BSS against B1 > 0 on the point estimate. The stronger
   claim "reliably beats B1" is made only if its interval also excludes 0.
3. **Calibrated overall.** The ratio of observed to expected positives lies in [0.8, 1.25], and
   its interval overlaps [0.9, 1.1]. (v1 used expected calibration error ≤ 0.02, which a
   predictor that always says zero passes at a 1% base rate.)
4. **B2 is reported, not required.** The gap to Wikimedia's model is published as measured.

Pass requires 1, 2 and 3. Censoring above 2% makes the run unmeasurable, not a pass or a fail.

### A10. Pilot measurements (filled before sign-off)

⟨pending: English inception cohort from the 2026-09-27 collector, which gives the base rate,
edit-to-revert and revert-to-tag delays, the share reverted before a 15-second cutoff, and B2's
arrival delay.⟩

---

## Part B: does System 2 earn its place?

### B1. Arms

Same streams, same windows, same development event pool, same output schema:

- **H:** heuristics alone (System 1 rules and local embeddings).
- **H+S2:** the same heuristics plus **one budget-capped System 2 pass**.
- **B3, the raw-sample LLM baseline:** the same model, prompted competently for the same task.
  It receives raw events from the same development pool, sampled by a frozen rule, up to the same
  input-token budget H+S2's System 2 pass used, and must produce the same executable mapping.

Every arm produces **one committed mapping**: entity types, identity rule per type,
relationships, and state fields. Each mapping is committed before any contact with the answer
key.

Each System 2 provider is run and reported separately: a hosted API, and a local model or the
client agent via MCP sampling. Each run:

- uses a frozen model id;
- starts in a clean session with no access to the lifeos repository, memory or prior
  conversations;
- is sampled with **3 seeds**;
- logs every call and retry;
- reports dollars, tokens, latency and local compute time.

### B2. Streams

1. **Wikipedia** (`recentchange`), plain. Reported, never counted: every model has read this
   schema.
2. **Wikipedia, obfuscated.** The rules are published with the result:
   - every field name becomes `f1…fN`;
   - every identifier value is replaced by a keyed hash (numeric ids included), and equal values
     hash equally across fields, so real cross-field links survive;
   - names, URLs, titles and free text are hashed the same way;
   - timestamps shift by one secret constant;
   - other numbers and the event structure are unchanged;
   - a fresh key is used for each independent run.
3. **A private stream: the lifeos dev-worker and sprint log** (legs, issues, pull requests,
   review seats, merges; from the status database and git). This is a default chosen
   2026-09-27, and Dave may swap it. The claim is **non-public provenance**: the events have
   never been published, and the evaluation session cannot read the repository or memory they
   come from. It is not a claim that no model knows what an issue or a pull request is.

   *Why a private stream when `s2w` is open source:* the question is what the model has already
   read, not whether our code is public. Wikipedia's streams and schemas are in LLM training
   data. Even obfuscated, their structure, value ranges and timing can be recognized rather than
   inferred. A stream that was never public is the fair test of the product claim, because a
   user's own Kafka topic is private too. We publish the results, a summary of the answer key,
   and sanitized fixtures that reproduce the scoring, never the raw events.

Passing on these streams makes two case studies, not a general result. A claim about unfamiliar
streams in general needs a stream from an independent owner, planned for after the slice.

### B3. Answer keys and metrics

- **Answer key:** written, adjudicated, and committed before the test window opens. Ambiguous or
  unobservable parts are marked as such. Equivalent identity rules are all listed, so any correct
  one is accepted. The key is scored up to renaming: the obfuscated stream is graded on
  structure, never on guessing real names.
- **Identity (primary):** each mapping's identity rules are applied to the test window's records
  and scored as pairwise same-entity links against the key's links.
  - **Merge precision:** of the links a mapping asserts, the share that are true. Its complement
    is the false-merge rate.
  - **Merge recall:** of the true links, the share the mapping asserts.
  - **Identity F1** combines the two.
  - Singletons avoid false merges but score zero recall, so neither half can be gamed alone.
- **Relationships:** typed and directed, scored as precision, recall and F1 over the test
  window's relationship instances.
- **Field roles:** accuracy averaged per field, so abundant easy fields do not dominate.
- **Abstention:** the share of fields, types and relationships each arm declined to map.
- **Repair operations (secondary):** starting from each committed mapping, the number of
  weighted edits needed to reach the key. The weights are frozen in advance: 1 per field-role
  fix, 3 per identity-rule fix, 2 per relationship fix. Reported as a **repair-operation count,
  not human minutes**. A claim about human time needs a small blinded study, planned for after
  the slice.
- **No answer-key feedback into any mapping during the test.** Development-window keys may guide
  development, identically for every arm, and are disclosed.

### B4. Gate-3 pass thresholds ⟨Dave⟩

Proposed, per provider. **The obfuscated stream and the private stream must each pass on their
own.**

1. **Primary:** H+S2's identity F1 exceeds H's by at least 0.10 absolute. The paired interval
   across seeds must exclude 0.
2. **Safety floor:**
   - H+S2's false-merge rate is at most 0.05;
   - it is no more than 0.02 above H's (non-inferiority margin);
   - relationship F1 is no more than 0.05 below H's.
3. **Beats the raw-sample baseline:** H+S2's identity F1 exceeds B3's by at least 0.05.
4. **Budget:** within $5 of API spend per stream per pass, or 30 minutes of local compute for a
   local model.

A provider that passes earns a claim for that provider only.

---

## Kill criteria and the time cap

- **Stop and re-decide** if gate 3 fails: System 2 does not beat heuristics and B3.
- **Stop and re-decide** if the launch does not get people to run it on their own streams.
- **Time box:** gate 2 in one week of sprints. Overall cap for the slice: **60 hours of sprint
  time** (about 30 sprints; Dave, 2026-09-27), counted from the first gate-2 sprint. This sits
  alongside the standing cap (primary until gate 3 or 2026-10-11, whichever comes first).
- A failed gate is a result, not a reason to relax this contract.

## Publication

Published with any result:

- the scoring code and the public-data manifest;
- every issuance, with the hourly digests;
- the eligibility manifest, exclusions, censoring with its worst-case analysis, and full
  denominators;
- every registered run, failed ones included;
- the obfuscation rules;
- sanitized private-stream fixtures.

## Sign-off

| Item | Decision | By, when |
|---|---|---|
| Eligible population (A2) | English Wikipedia, with a multilingual re-run after the slice | Dave, 2026-09-27 |
| Gate-4 thresholds (A9) | Wikimedia's model reported, not required (Dave). v2 replaces the calibration check: expected calibration error ≤ 0.02 becomes an observed/expected ratio — **needs Dave's re-approval** | Dave, 2026-09-27 (part) |
| Private stream (B2.3) | lifeos dev-worker and sprint log (default; swappable) | karpathy default, 2026-09-27 |
| Gate-3 thresholds and budget (B4) | ⟨pending⟩ | |
| Overall hours cap | 60 hours of sprint time | Dave, 2026-09-27 |
| Pilot measurements (A10) | ⟨pending⟩ | |

## Change log

**v2 (2026-09-27)**, answering the Codex round-1 review. Numbers are the review's finding
numbers.

| # | Finding | Change |
|---|---|---|
| 1 | Revert matched to the wrong reverting edit | A1: exact provenance from `page_change.v1` reverted ranges |
| 2 | Revert event confused with the later tagging job | A1: the label comes from provenance, not tags. A4: tags are an audit only. Wording on reverting edits fixed |
| 3 | Cutoff could drift; commitment not enforced | A3: ingress recorder, 60-second arrival limit, commit-by-deadline, a shared eligibility manifest, hourly public digests |
| 4 | Part B rewarded an answer-key oracle | B1/B3: one committed mapping per arm before key contact; no key feedback during test |
| 5 | Censoring can still bias | A4: evaluator-owned censoring, breakdowns, worst-case analysis, 2% ceiling |
| 6 | Abstention claim wrong; B2 subset selectable | A6: score predictor plus fallback; B2 subset chosen by B2 availability only |
| 7 | `s2w` could copy B2 | A5: input policy excludes B2 and other moderation scores from the primary arm |
| 8 | Freeze left leakage and selection | A8: labels ascertained before freeze, as-of features, full freeze list, no test-time learning, all runs registered |
| 9 | Expected calibration error ≤ 0.02 is weak | A9: observed/expected ratio with an interval; reliability chart including the top-risk decile |
| 10 | Clustering and power | A6: paired bootstrap by page, plus editor and day-block checks. A8: power-based test length (7 to 21 days) |
| 11 | Part B metrics undefined | B3: pairwise links, merge precision and recall, equivalent keys, typed and directed relationships |
| 12 | Correction minutes unvalidated | B3: renamed a repair-operation count with frozen weights; a human-time claim needs a study |
| 13 | Arms not comparable | B1: same pool, equal token budget for B3, same output schema, clean sessions, 3 seeds |
| 14 | Thresholds passable through a weak metric | B4: one primary endpoint, safety floors and margins, each stream must pass, claims per provider |
| 15 | Bot slice is not a bound | A6: additive decomposition, labelled descriptive |
| 16 | Obfuscation and "unseen" claims | B2: exact rules incl. numeric ids and cross-field equality; the claim becomes non-public provenance |
| 17 | Pilot claim, B1 wording, publication | A1/A10: inception-cohort pilot; A9: B1 claim wording; Publication section |
