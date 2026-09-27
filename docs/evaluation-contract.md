# Evaluation contract (gate 1)

Status: **DRAFT v3, 2026-09-27.** Reviewed twice by Codex (gpt-6-astra): v1 "revise"
([round 1](reviews/gate1-contract-round1-codex.md)), v2 "revise" ([round 2](reviews/gate1-contract-round2-codex.md)).
v3 answers round 2; the change log at the end maps each finding. Items marked **⟨Dave⟩** are
his to set. Everything else is a proposal he can overrule.

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

**Trust model, stated once.** The evaluator is our own code, run by us. It is a separate process
from every predictor, it enforces deadlines and decides eligibility, labels and censoring, and
it publishes everything needed to re-check it. Where a claim rests on trusting the evaluator
rather than on external evidence, this contract says so.

---

## Part A: the forecast question

### A1. The question

> **Q-revert-30m:** among English Wikipedia edits still unreverted at their cutoff (A3), will
> the edit be reverted within 30 minutes of being made?

Times: **T** is the edit's `rev_dt`. **Receipt** is when the ingress recorder first received the
edit. **C**, the cutoff, is receipt + 15 seconds. The recorder's clock is synchronized by NTP and
its offset is logged; `rev_dt` is Wikimedia's clock.

**Revert provenance.** Every revision in `mediawiki.page_change.v1` that MediaWiki recognizes as
a revert carries `revert: {method, is_exact, rev_original_id, rev_reverted_oldest_id,
rev_reverted_newest_id}`. Edit E is reverted by revision R when:

- R and E share `(wiki_id, page_id)`; titles are never used, since pages move;
- E lies in R's reverted range **inclusive**, where order is MediaWiki's own: by
  `(rev_timestamp, rev_id)`, not by `rev_id` alone, because imported histories break id order;
- any method counts: rollback, undo or manual, exact or not.

If several reverts cover E, the earliest by `(rev_timestamp, rev_id)` decides. A revert whose
provenance is missing or malformed, or a page whose history order cannot be established from the
recorder log and the Action API, censors the affected edits.

**Positive:** E is reverted by some R with `R.rev_dt − T ≤ 30 min`. Self-reverts count and their
share is reported. The label does not depend on MediaWiki's later `revertedTagUpdate` job, which
can be skipped; `mw-reverted` tags are an audit only (A4).

**Matcher fixtures, committed before the freeze**, each a recorded or constructed event
sequence with its expected label: a single-edit undo; a multi-edit rollback; an undo of an
unrelated edit on the same page; a self-revert; a revert of a revert; an edit covered by two
reverts; a range that crosses a moved page; a page with imported history.

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

- **Ingress recorder:** a separate process that records every raw stream event with its receipt
  time before any predictor sees it. A duplicate event (same event id) keeps its first receipt.
- **Provisional candidates:** every edit matching A2 that arrived within **60 seconds** of T.
  Every predictor must forecast every provisional candidate.
- **Commitment:** predictors send each probability to the **commitment receiver**, a part of the
  evaluator process that records its own receipt time and rejects anything received after C. A
  rejected or absent forecast is replaced by the fallback (A6). The commitment receiver is the
  proof of timeliness. Its trust assumption is the evaluator's (see the trust model above). A
  digest of commitments is pushed to a public repository **every minute**; that shows the log
  was not rewritten later, not that each forecast met its deadline.
- **Final eligibility** is decided at ascertainment (A4), using everything received by then.
  - A provisional candidate becomes **ineligible** if a revert of it has `R.rev_dt ≤ C`, even if
    that revert arrived after C. Late evidence can make an edit ineligible; it never makes one
    eligible.
  - If survival to C cannot be established, the edit is censored, never assumed to survive.
- So the question is conditional: risk among edits that survived unreverted to their cutoff.
  ClueBot NG often reverts within seconds, so this removes many of the easiest positives. The
  contract states it rather than hiding it.
- **Coverage of the window.** The recorder's uptime and the share of edits arriving later than
  60 seconds are reported per hour. **The run is unmeasurable if the recorder is up less than
  95% of the scheduled window, or if more than 5% of A2 edits arrive late.**
- **Exactly one issuance per edit per predictor**, immutable: question, edit, issue time,
  horizon, probability, predictor and version, evidence cutoff (recorder offset). Repairs,
  replays and restarts never change it.

### A4. Outcomes, ascertainment and censoring

- **Outcome source:** revert provenance (A1) received by the ascertainment deadline, from the live
  recorder plus replay.
- **Ascertainment deadline: T + 24 hours.** Scores are computed after the whole test window has
  been ascertained, so nothing is gained by labelling sooner. Before the deadline, the evaluator
  replays from the stream every interval in which the recorder had a gap or its lag (receipt −
  `rev_dt`) exceeded 5 minutes. EventStreams supports replay by timestamp. A gap that replay
  cannot fill censors the edits whose horizons it overlaps.
- **Late-provenance audit at T + 7 days.** The evaluator replays the test window's provenance
  again and counts reverts with `R.rev_dt` inside a horizon that were not received by the
  deadline. **If they would flip more than 1% of positive labels, the run is unmeasurable.** Tags
  are checked in the same pass, restricted to reverts inside the horizon. Tags come from the same
  MediaWiki system, so agreement shows consistent delivery, not truth. Disagreements are reported
  by kind and change no label.
- **Censoring is owned by the evaluator**, decided from the recorder log and page state, never
  from forecasts. An edit is censored when the page or revision is deleted or hidden before its
  label can be established, when replay cannot fill a gap over its horizon, or when A1 cannot
  establish order or survival.
- **Censoring sensitivity, per claim.** For each comparison a gate rests on (predictor vs B0,
  predictor vs B1), the evaluator relabels the censored edits in the way that most reduces the
  predictor's advantage. Per case, the Brier contrast is
  `(p − y)² − (b − y)² = (p − b)(p + b − 2y)`, so choose y ∈ {0, 1} to maximize it, keeping any
  label already fixed by evidence. It then recomputes the gate.
  - If the gate survives, the pass is unqualified.
  - If it reverses, the result is reported as **"pass, sensitive to censoring"**, which does not
    count as a pass for gate 4.
- **The run is unmeasurable if more than 2% of eligible edits are censored**, and that check comes
  first.
- Censored and ineligible counts are broken down by hour, page activity, predictor score range and
  editor type.

### A5. Baselines

Each baseline is fitted or calibrated only on development-window edits ascertained before the
freeze.

| id | Baseline | What it controls for |
|---|---|---|
| B0 | Base rate: the development window's positive rate, frozen | Whether there is any skill over prevalence |
| B1 | Contextual: logistic regression on 4 features (anonymous/temporary account, account-age bucket, byte-size change, empty edit summary). Feature definitions, missing-value handling and regularization frozen at the freeze | Whether `s2w` does more than the obvious features |
| B2 | Wikimedia `revertrisk-language-agnostic` (v3 as of 2026-09-27), read from `mediawiki.page_revert_risk_prediction_change.v1`. Reported raw and recalibrated (isotonic fit on the development window) | A specialist revert model. Its training target is being checked (A10); it is not assumed to match this question |

**Input policy.** The primary `s2w` arm may **not** use B2, or any other moderation or revert
score, as an input. Otherwise `s2w` could pass by recalibrating Wikimedia's score, which would
show integration, not forecasting. An optional secondary arm that does use B2 is reported
separately and never counts toward the gate.

**B2 coverage.** The head-to-head with B2 uses exactly the edits where B2's score was received
before C. That subset is chosen by B2's availability alone, never by whether another predictor
answered.

### A6. Scoring

- **What is scored is the committed product: predictor plus fallback.** When the predictor
  abstains or misses the deadline, the fallback issues the B0 probability. Every eligible,
  labelled edit is scored. All abstentions together earn exactly zero skill, so abstaining cannot
  pass the gate by itself. Selective abstention can still help the score; that is a legitimate
  property of the product, so it is reported: the share of genuine answers, and skill on those
  answers alone.
- **Primary metric: Brier skill score against B0,**
  `BSS = 1 − Σ(pᵢ − yᵢ)² / Σ(bᵢ − yᵢ)²`, over the same edits.
- **Primary interval:** a multiway ("pigeonhole") bootstrap that resamples pages, editors and days
  jointly (Owen 2007; Bakshy and Eckles 2013): 2,000 replicates, percentile 95% interval. The
  bootstrap is paired: every predictor is scored on the same resampled edits, and both sums are
  recomputed in each replicate. Fitted predictors stay fixed.
- **Validation before the freeze:** on development data, run the procedure under the null (a
  predictor equal to B0 plus noise) and check that its 95% intervals exclude zero in no more than
  7% of repeats. If they exclude it more often, the method is replaced before the freeze.
- **Sensitivity intervals:** one-way bootstraps by page, by editor and by day. A gate needs the
  primary interval **and every sensitivity interval** to clear it. Disagreement fails the gate
  rather than being resolved in our favour.
- **Secondary metrics:**
  - log loss;
  - area under the precision–recall curve (discrimination, reported apart from BSS);
  - calibration-in-the-large: observed versus expected positives (A9);
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
- One wiki, one question, one conditional population (edits surviving the cutoff), during the
  hours the recorder was up. "Calibrated overall" means event totals match; opposing errors in
  different risk groups can cancel.
- Timeliness rests on the evaluator's commitment receiver, which we run.

### A8. Windows, freezing and runs

- **Development window:** explicit start and end dates, recorded at the freeze. Build, tune and
  fit here, using only labels ascertained before the freeze. Historical features are
  reconstructed as they stood at each edit's cutoff, never from later API state.
- **Power:** before the freeze, estimate from development data (using the A6 primary bootstrap)
  how many test days give **80% power to detect a BSS of 0.02 over B0**. The test length is that
  number, at least 7 days. **If more than 21 days are needed, the question is changed before the
  freeze** (for example a longer horizon or a wider population), not run underpowered.
- **The freeze records:**
  - the exact test start and end, which start at least 24 hours after the freeze;
  - commit hash and config hash;
  - prompts, model identifiers and versions, preprocessing, calibrators, the fallback policy and
    the scoring code;
  - the B0 to B2 fits and the matcher fixtures' results;
  - the bootstrap validation result;
  - the archived Automoderator and bot configuration;
  - the two non-English wikis for the later re-run.
- **Models that change underneath us.** Hosted models are called by their most specific
  snapshot id. If a provider changes or retires the model during the test, the run is
  unmeasurable and a new window is registered.
- **No parameter learning during the test.** World state keeps accumulating from the stream, as
  it would in use. Parameters, prompts and calibrators do not change.
- **Every run is registered.** A fix made after seeing test results needs a new test window, and
  the earlier run is kept and published.
- The test is issued **live**. A replay of the same days may be run to check determinism; the
  live run is the one that counts.

### A9. Gate-4 pass thresholds (Dave, 2026-09-27: report B2, don't require it)

1. **Skill over prevalence.** BSS against B0 > 0 by the A6 intervals (primary and every
   sensitivity lower bound > 0).
2. **More than the obvious features.** BSS against B1 > 0 on the point estimate. The stronger
   claim "reliably beats B1" is made only if its intervals also exclude 0.
3. **Calibrated overall (Dave, 2026-09-27).** The 90% interval for the ratio of observed to
   expected positives lies **entirely inside [0.8, 1.25]**, an equivalence test. The interval uses
   the A6 primary bootstrap. If fewer than 20 positives are expected, the run is underpowered for
   this check and is extended per A8.
4. **B2 is reported, not required.** The gap to Wikimedia's model is published as measured.

Pass requires 1, 2 and 3, and no "sensitive to censoring" qualifier on 1 or 2. An unmeasurable run
(A3, A4, A8) is neither a pass nor a fail.

### A10. Pilot and open checks (filled before sign-off)

⟨pending⟩

- An English inception cohort from the 2026-09-27 collector: base rate, edit-to-revert delay,
  revert-to-provenance delivery delay, edit arrival delay, the share reverted before a 15-second
  cutoff, and B2's arrival delay.
- Wikimedia revert-risk model's training target (what counts as a revert, and the window), from
  its training repositories.
- The matcher fixtures (A1) passing.

---

## Part B: does System 2 earn its place?

### B1. Arms

Same streams, same replicates, same development event pool, same output schema:

- **H:** heuristics alone (System 1 rules and local embeddings).
- **H+S2:** the same heuristics plus **one budget-capped System 2 pass**.
- **B3, the raw-sample LLM baseline:** the same model, prompted competently for the same task.
  It receives raw events from the same development pool, sampled by a frozen rule, up to the same
  input-token budget H+S2's System 2 pass used, and must produce the same executable mapping.

Every arm produces **one committed mapping per replicate**: entity types, identity rule per type,
relationships, and state fields. Each mapping is committed before any contact with the answer
key.

**Shared across arms, frozen:**

- the harness that executes a mapping;
- the output schema and its limits;
- the retry policy;
- any execution assistance (for example, telling the model that its mapping failed to parse).

**Budget accounting** covers every model call, including preprocessing and retries.

Each System 2 provider is run and reported separately: a hosted API, and a local model or the
client agent via MCP sampling. Each run:

- uses a frozen model snapshot id;
- starts in a clean session with no access to the lifeos repository, memory or prior
  conversations;
- logs every call and retry;
- reports dollars, tokens, latency and local compute time.

### B2. Streams

1. **Wikipedia** (`recentchange`), plain. Reported, never counted: every model has read this
   schema.
2. **Wikipedia, obfuscated.** The rules are published with the result:
   - every field name becomes `f1…fN`, in a random order per replicate;
   - each identifier field is declared with an **identifier domain** (page, user, revision and so
     on);
   - values are replaced by a keyed hash **of domain and value**, so user 42 and page 42 differ
     while the same page id matches wherever it appears;
   - identifiers embedded in URLs and titles are extracted by published canonicalization rules
     and hashed the same way;
   - other names and free text are hashed;
   - timestamps shift by one secret constant;
   - other numbers and the event structure are unchanged;
   - each replicate gets a fresh key and field order, shared by all arms in that replicate and by
     its development and test windows.

   Relationships the transformation destroys are marked **unobservable** in the answer key and
   are not scored.
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

### B3. Answer keys and scoring

**Answer key**, written, adjudicated and committed before the test window opens:

- **Mentions:** every `(record, field path)` whose value identifies an entity, with the entity's
  type and identity. Every mention of an entity, in any field, is listed.
- Ambiguous or unobservable parts are marked as such and are not scored.
- Equivalent identity rules are all accepted; the key scores the clusters a rule produces, not
  which rule produced them.
- Scored up to renaming: the obfuscated stream is graded on structure, never on real names.

**Executing a mapping.** The frozen harness applies each committed mapping to the test window's
records. The result is predicted mentions with a predicted type and cluster, plus predicted
relationship instances.

**Type alignment.** Each predicted type is matched to at most one key type, maximizing shared
mentions (Hungarian assignment). A predicted type left unmatched counts entirely as error: its
mentions are false positives. A key type left unmatched scores zero recall.

**Identity (primary): B-cubed precision, recall and F1** per key type over its mentions, then
**macro-averaged across key types**, so every type weighs the same and one heavily repeated
entity cannot dominate.

- **False-merge rate:** 1 − B-cubed precision.
- A mapping that makes every mention its own entity has perfect precision and low recall
  wherever true repeats exist.
- A key type with no repeated entity is reported but excluded from the macro average, since its
  recall is undefined.

**Relationships:** typed and directed instances, scored by precision, recall and F1. An instance
matches when its type aligns and both endpoint mentions fall in the correct key clusters.
A relationship holds from the record that asserts it until a later record ends it, if the key
defines an end. Extra predicted relationship types count as errors.

**Field roles:** accuracy averaged per field, so abundant easy fields do not dominate.

**Abstention:** the share of fields, types and relationships each arm declined to map. An
abstained mention simply has no prediction: it lowers recall and cannot lower precision.

**Repair operations (secondary):** starting from each committed mapping, the number of weighted
edits needed to reach the key. The weights are frozen in advance: 1 per field-role fix, 3 per
identity-rule fix, 2 per relationship fix. Reported as a **repair-operation count, not human
minutes**. A claim about human time needs a small blinded study, planned for after the slice.

**No answer-key feedback into any mapping during the test.** Development-window keys may guide
development, identically for every arm, and are disclosed.

### B4. Gate-3 pass thresholds (Dave, 2026-09-27: accepted as proposed)

Each provider is judged separately, and **the obfuscated stream and the private stream
must each pass on their own.** Each stream runs **5 replicates** (a fresh obfuscation key and
field order for the obfuscated stream, a fresh model seed for all). All arms share a replicate.
Five replicates show variability; they are not a formal significance test, so the rule below
asks for consistency across all of them rather than a confidence interval.

1. **Primary:** H+S2's identity F1 exceeds H's by at least 0.10 on the mean over replicates, and
   is higher than H's in **all 5** replicates.
2. **Safety floor:**
   - H+S2's false-merge rate is at most 0.05 in every replicate;
   - it is no more than 0.02 above H's on the mean;
   - relationship F1 is no more than 0.05 below H's on the mean.
3. **Absolute floor:** H+S2's identity F1 is at least 0.60 on the mean. Beating weak baselines is
   not enough.
4. **Beats the raw-sample baseline:** H+S2's identity F1 exceeds B3's by at least 0.05 on the
   mean, and is higher in at least 4 of 5 replicates.
5. **Budget:** within $5 of API spend per stream per replicate, or 30 minutes of local compute
   for a local model.

If H already scores above 0.90, the 0.10 margin in 1 is impossible to meet; in that case the gate
is judged on 2 to 5 alone and the report says so. A provider that passes earns a claim for that
provider only.

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
- every issuance, with the minute digests;
- the eligibility manifest, exclusions, censoring with its sensitivity analysis, and full
  denominators;
- every registered run, failed and unmeasurable ones included;
- the matcher fixtures, the obfuscation and canonicalization rules;
- sanitized private-stream fixtures.

## Sign-off

| Item | Decision | By, when |
|---|---|---|
| Eligible population (A2) | English Wikipedia, with a multilingual re-run after the slice | Dave, 2026-09-27 |
| Gate-4 thresholds (A9) | Wikimedia's model reported, not required; calibration by equivalence test, the 90% interval for observed/expected positives inside [0.8, 1.25] | Dave, 2026-09-27 |
| Private stream (B2.3) | lifeos dev-worker and sprint log (default; swappable) | karpathy default, 2026-09-27 |
| Gate-3 thresholds and budget (B4) | As proposed: +0.10 identity F1 over H in all 5 replicates, +0.05 over B3, false merges ≤ 0.05 per replicate, identity F1 ≥ 0.60, $5 per stream per replicate | Dave, 2026-09-27 |
| Overall hours cap | 60 hours of sprint time | Dave, 2026-09-27 |
| Pilot and open checks (A10) | ⟨pending⟩ | |

## Change log

**v3 (2026-09-27)**, answering the Codex round-2 review (numbers are round-2 finding numbers).

| # | Finding | Change |
|---|---|---|
| 1 | Hourly digests do not prove timely commitment | A3: a commitment receiver in the evaluator rejects late forecasts; a trust model stated up front; digests every minute, described as tamper evidence only |
| 2 | Cutoff and negative-label semantics | A1: T, receipt and C defined. A3: provisional candidates, late evidence can make an edit ineligible, unresolved survival is censored. A4: 24-hour ascertainment with replay; 7-day late-provenance audit with a 1% flip limit |
| 3 | Provenance matcher claimed too much | A1: `(wiki_id, page_id)`, `(rev_timestamp, rev_id)` order, inclusive ranges, earliest revert wins, malformed provenance censors, eight committed fixtures |
| 4 | Part B scoring not executable | B3: mention-level key, Hungarian type alignment, B-cubed macro-averaged by type, relationship matching and validity, undefined cases |
| 5 | Three seeds cannot carry interval claims | B4: 5 replicates, consistency-across-all rule instead of an interval; per-replicate safety floor |
| 6 | Calibration gate rewarded imprecision | A9: equivalence test, the 90% interval inside [0.8, 1.25]; minimum expected positives |
| 7 | "Widest interval" is not a dependence correction | A6: multiway (pigeonhole) bootstrap as primary, validated under the null before the freeze; every sensitivity interval must also clear; A8: 80% power, and change the question if more than 21 days are needed |
| 8 | Censoring worst case was wrong | A4: relabel per comparison using the Brier contrast; a reversal becomes "sensitive to censoring" and does not pass |
| 9 | 60-second limit conditions the population | A3: uptime ≥ 95% and late arrivals ≤ 5% or the run is unmeasurable; A7 states the scope |
| 10 | Obfuscation links, B3 fairness | B2: identifier domains, domain-keyed hashes, URL canonicalization, per-replicate key and field order, unobservable relations; B1: shared harness, assistance, retries and full budget accounting |
| — | Relative-only claims; impossible margin | B4: absolute floor of 0.60; rule for a baseline above 0.90 |
| — | Models changing underneath | A8: snapshot ids; a provider change makes the run unmeasurable |

**v2 (2026-09-27)**, answering the Codex round-1 review. Numbers are round-1 finding numbers.

| # | Finding | Change |
|---|---|---|
| 1 | Revert matched to the wrong reverting edit | A1: exact provenance from `page_change.v1` reverted ranges |
| 2 | Revert event confused with the later tagging job | A1: the label comes from provenance, not tags. A4: tags are an audit only |
| 3 | Cutoff could drift; commitment not enforced | A3: ingress recorder, 60-second arrival limit, commit-by-deadline, a shared eligibility manifest |
| 4 | Part B rewarded an answer-key oracle | B1/B3: one committed mapping per arm before key contact; no key feedback during test |
| 5 | Censoring can still bias | A4: evaluator-owned censoring, breakdowns, sensitivity analysis, 2% ceiling |
| 6 | Abstention claim wrong; B2 subset selectable | A6: score predictor plus fallback; B2 subset chosen by B2 availability only |
| 7 | `s2w` could copy B2 | A5: input policy excludes B2 and other moderation scores from the primary arm |
| 8 | Freeze left leakage and selection | A8: labels ascertained before freeze, as-of features, full freeze list, no test-time learning, all runs registered |
| 9 | Expected calibration error ≤ 0.02 is weak | A9: observed/expected ratio; reliability chart including the top-risk decile |
| 10 | Clustering and power | A6: paired bootstrap with dependence checks. A8: power-based test length |
| 11 | Part B metrics undefined | B3: identity, merges, relationships defined |
| 12 | Correction minutes unvalidated | B3: renamed a repair-operation count with frozen weights |
| 13 | Arms not comparable | B1: same pool, equal token budget for B3, same output schema, clean sessions |
| 14 | Thresholds passable through a weak metric | B4: one primary endpoint, safety floors, each stream must pass, claims per provider |
| 15 | Bot slice is not a bound | A6: additive decomposition, labelled descriptive |
| 16 | Obfuscation and "unseen" claims | B2: exact rules; the claim becomes non-public provenance |
| 17 | Pilot claim, B1 wording, publication | A1/A10: inception-cohort pilot; A9: B1 claim wording; Publication section |
