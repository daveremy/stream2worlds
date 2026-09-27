# Evaluation contract (gate 1)

Status: **SIGNED v4.1, 2026-09-27** (specification). Five Codex (gpt-6-astra) review rounds on
a narrowing list: [1](reviews/gate1-contract-round1-codex.md) (17 findings),
[2](reviews/gate1-contract-round2-codex.md) (10), [3](reviews/gate1-contract-round3-codex.md) (5
blockers, plus cheaper equivalents for this 60-hour scope, adopted),
[4](reviews/gate1-contract-round4-codex.md) (3 spot fixes), [5](reviews/gate1-contract-round5-codex.md):
**sign**. Dave approved every decision in the sign-off table. The change log at the end maps each
finding.

**Signed is not frozen.** The freeze prerequisites are built in gates 2 to 4 and must pass before
any test window opens: the A10 pilot, the A1 matcher fixtures, the B3 reference scorer and its
fixtures, the A6 simulation, and the frozen data manifests.

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
  It also archives every event it receives, so audits never depend on Wikimedia's retention.
- **Clock:** the recorder's clock is synchronized by NTP and its offset logged every minute. An
  hour in which the offset exceeds 1 second counts as recorder downtime.
- **Provisional candidates:** every edit matching A2 that arrived within **60 seconds** of T.
  Every predictor must forecast every provisional candidate.
- **Commitment:** predictors send each probability to the **commitment receiver**, a part of the
  evaluator process that records its own receipt time and rejects anything received after C. A
  rejected or absent forecast is replaced by the fallback (A6). The commitment receiver is the
  proof of timeliness, under the trust model above. A digest of the append-only commitment log
  is pushed to a public repository **every hour**; that shows the log was not rewritten later.
- **Final eligibility** is decided at ascertainment (A4), using everything received by then.
  - A provisional candidate becomes **ineligible** if a revert of it has `R.rev_dt ≤ C`, even if
    that revert arrived after C. Late evidence can make an edit ineligible; it never makes one
    eligible.
  - If survival to C cannot be established, the edit's **eligibility is unknown**, and it is
    handled by A4's censoring rules, never assumed to survive.
- So the question is conditional: risk among edits that survived unreverted to their cutoff.
  ClueBot NG often reverts within seconds, so this removes many of the easiest positives. The
  contract states it rather than hiding it.
- **Coverage of the window.** The **scheduled population** is every A2 edit with T inside the test
  window, whether or not it arrived live, reconstructed from the live recorder plus the 24-hour
  replay (A4). Reported per hour: recorder uptime, and the share of the scheduled population that
  arrived late (more than 60 seconds after T) or never arrived live. **The run is unmeasurable if
  the recorder is up less than 95% of the window, or if more than 5% of the scheduled population
  arrived late or never arrived.**
- **Exactly one issuance per edit per predictor**, immutable: question, edit, issue time,
  horizon, probability, predictor and version, evidence cutoff (recorder offset). Repairs,
  replays and restarts never change it.

### A4. Outcomes, ascertainment and censoring

- **Outcome source:** revert provenance (A1) received by the ascertainment deadline, from the live
  recorder plus replay.
- **Ascertainment deadline: T + 24 hours.** Scores are computed after the whole test window has
  been ascertained, so nothing is gained by labelling sooner. Before the deadline, the evaluator
  replays from the stream every interval in which the recorder had a gap or its lag (receipt −
  `rev_dt`) exceeded 5 minutes. EventStreams supports replay by timestamp, with 7 to 31 days of
  retention depending on the stream. A gap that replay cannot fill censors the edits whose horizons
  it overlaps.
- **Late-provenance audit at T + 72 hours** (inside the shortest retention, with margin). The
  evaluator replays the provenance again and counts every eligible edit whose **label or
  eligibility** would change: a revert inside the horizon, or before C, that was not received by
  the deadline. **If changes exceed 1% of eligible edits, the run is unmeasurable.** Below that,
  labels stay as ascertained and the changes are published. Tags are checked in the same pass,
  restricted to reverts inside the horizon; they come from the same MediaWiki system, so agreement
  shows consistent delivery, not truth, and disagreements change no label.
- **Censoring is owned by the evaluator**, decided from the recorder log and page state, never
  from forecasts. An edit is censored when the page or revision is deleted or hidden before its
  label can be established, when replay cannot fill a gap over its horizon, or when A1 cannot
  establish order, survival or eligibility. The censoring rate's denominator is **all provisional
  candidates**, so edits of unknown eligibility are counted rather than presumed either way.
- **The run is unmeasurable if more than 2% of provisional candidates are censored**, and that
  check comes first.
- **Censoring sensitivity, per claim.** For each skill comparison a gate rests on (predictor vs
  B0, predictor vs B1), the evaluator assigns every censored candidate the treatment that most
  reduces the predictor's advantage, consistent with the evidence:
  - a candidate of unknown eligibility may be included or excluded;
  - an included candidate gets y = 0 when p > b and y = 1 when p < b, since the per-case Brier
    contrast is `(p − y)² − (b − y)² = (p − b)(p + b − 2y)`.

  It then recomputes the gate. If the gate survives, the pass stands. If it reverses, the result
  is **"pass, sensitive to censoring"**, which does not count as a pass for gate 4.
- **Calibration sensitivity.** Including a candidate changes both observed and expected totals,
  so the evaluator bounds the **ratio** directly. In each bootstrap replicate it computes the
  smallest and largest observed/expected ratio over all evidence-consistent choices of
  eligibility and label for the censored candidates. The calibration check (A9) passes only if
  the 5th percentile of the smallest ratios is at least 0.8 and the 95th percentile of the
  largest is at most 1.25.
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
- **The one interval that decides:** a multiway ("pigeonhole") bootstrap that resamples pages,
  editors and days jointly, each case weighted by the product of its three independently drawn
  factor weights (Owen and Eckles, arXiv 1106.2125): 2,000 replicates, percentile 95% interval.
  It is paired: every predictor is scored on the same resampled weights, and both sums are
  recomputed in each replicate. Fitted predictors stay fixed.
- **Sanity check before the freeze.** A simulation with page, editor and day effects sized from
  development data compares **two different predictors of equal expected Brier score** (a known zero
  contrast whose per-case differences are not all zero), 500 repeats. The interval should exclude zero, on either side, in no more than 7% of
  repeats. If it does worse, the method is replaced before the freeze. The simulation settings are
  frozen with the contract. This is a sanity check, not a proof of coverage.
- **Diagnostics, not gates:** one-way bootstraps by page, by editor and by day, published beside
  the primary interval. A large disagreement is reported and explained, but only the primary
  interval decides a gate.
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
- **Test length:** before the freeze, estimate from development data (using the A6 primary
  bootstrap) how many test days give **80% power to detect a BSS of 0.02 over B0**, and check the
  same window expects at least 20 positives for calibration. The test length is that number, at
  least 7 days, **fixed at the freeze and never extended**. If more than 21 days would be needed,
  the question is changed before the freeze (for example a longer horizon or a wider
  population), not run underpowered.
- **The freeze records:**
  - the exact test start and end, which start at least 24 hours after the freeze;
  - commit hash and config hash;
  - prompts, model identifiers and versions, preprocessing, calibrators, the fallback policy and
    the scoring code;
  - the B0 to B2 fits and the matcher fixtures' results;
  - the bootstrap sanity-check settings and result;
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

1. **Skill over prevalence.** The lower end of the primary 95% interval for BSS against B0 is
   above 0.
2. **More than the obvious features.** BSS against B1 > 0 on the point estimate; this condition
   uses no interval. The stronger claim "reliably beats B1" is made only if its primary interval
   also excludes 0.
3. **Calibrated overall (Dave, 2026-09-27).** The 90% interval for the ratio of observed to
   expected positives, from the primary bootstrap, lies **entirely inside [0.8, 1.25]**, an
   equivalence test. It must also pass the censoring bound (A4). If fewer than 20 positives are
   expected in the fixed window, the calibration check is **inconclusive**, and so is gate 4.
4. **B2 is reported, not required.** The gap to Wikimedia's model is published as measured.

Pass requires 1, 2 and 3, and no "sensitive to censoring" qualifier on 1 or 2. An unmeasurable or
inconclusive run is neither a pass nor a fail.

### A10. Pilot and open checks

**Pilot, 2026-09-27** ([research note 0004](../research/0004-revert-pilot.md)): one 30-minute
cohort of 1,864 English edits. Base rate 3.8%; median edit-to-revert 1.8 minutes (p90 8.8); 10
edits reverted before the cutoff; edit arrival p99 20 s, none over 60 s; B2 present for 93% of
edits, all before the cutoff, with ROC AUC 0.888 but a raw mean of 0.40, so recalibration is
essential. Orders of magnitude only; the development window measures them properly.

Still open before the freeze:

- The bot share of within-horizon reverts, measured on the development window. None appeared
  after the cutoff in the pilot, so A7's ClueBot NG caveat is unmeasured and is amended when the
  number exists.
- ~~Wikimedia revert-risk model's training target~~: resolved by research note 0001 — its
  label has no time window, so its raw scores answer a different question than Q-revert-30m;
  A5's mandatory recalibration stands.
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
- the retry policy, and a rule that counts a failed execution as that replicate's result;
- any execution assistance (for example, telling the model that its mapping failed to parse).

**Budget accounting** covers every model call, including preprocessing and retries.

**One System 2 provider is evaluated first:** a hosted API, called by a frozen model snapshot id.
The second implementation (a local model, or the client agent via MCP sampling) is built in the
slice and evaluated after it; claims are per provider. Each run:

- starts in a clean session with no access to the lifeos repository, memory or prior
  conversations;
- logs every call and retry;
- reports dollars, tokens and latency.

### B2. Streams

1. **Wikipedia** (`recentchange`), plain. Reported, never counted: every model has read this
   schema.
2. **Wikipedia, obfuscated.** The rules are published with the result:
   - every field name becomes `f1…fN`, in a random order per replicate;
   - each identifier field is declared with an **identifier domain** (page, user, revision and so
     on). The domains are the transformer's private metadata: no arm ever sees them, and they are
     published only after every mapping is committed;
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
   are not scored. Hashing by domain does reveal that two values in the same domain are equal, which
   is exactly what the real stream reveals through its field semantics.
3. **A private stream: the lifeos dev-worker and sprint log** (legs, issues, pull requests,
   review seats, merges; from the status database and git). This is a default chosen
   2026-09-27, and Dave may swap it. The claim is **non-public provenance**: the events have
   never been published, and the evaluation session cannot read the repository or memory they
   come from. It is not a claim that no model knows what an issue or a pull request is.

   The test corpus is **bounded and frozen**: a fixed held-out span of events. Its answer key is
   generated from the source systems' own identifiers where possible (issue and pull-request
   numbers, commit hashes, leg ids), then a sample is inspected by hand. The claim is limited to
   that corpus.

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
  identity. Every mention of an entity, in any field, is listed. Each entity has a type.
- Ambiguous or unobservable parts are marked as such and are not scored.
- Equivalent identity rules are all accepted: the scorer compares the clusters a mapping
  produces, never which rule produced them.
- Scored up to renaming: the obfuscated stream is graded on structure, never on real names.

**Executing a mapping.** The frozen harness applies each committed mapping to the test corpus.
The result is **predicted mentions**, each placed in a predicted cluster (an entity), and
**predicted relationship edges** between clusters.

**Identity, primary: B-cubed F1** over mentions, computed on the unmodified key and predicted
partitions with the formulas of the reference coreference scorer (Pradhan et al. 2014, §4.2). No
mention is added or dropped to make the two partitions match.

- Every key mention counts in recall and every predicted mention counts in precision, singletons
  included. A key mention the mapping missed scores zero recall. A predicted mention not in the key
  is spurious and scores zero precision.
- Averaged over all mentions (micro). Clusters are compared as sets of mentions, so a cluster
  mixing two types, or two real entities, is a false merge whatever it is called. No type
  alignment is involved in this metric.
- **False-merge rate:** 1 − B-cubed precision.
- B-cubed weighs every mention equally, so large entities count in proportion to their mentions.
  That is intended; the entity-level floor in B4 guards the other side.
- Worked check, frozen as a fixture: key `{a, b, c}`, prediction `{a, b, d}` gives precision,
  recall and F1 of 4/9 each.

**Entity recovery, the floor metric:** the share of key entities with at least two mentions that
are **recovered**: some predicted cluster holds at least 90% of the entity's mentions, and at least
90% of that cluster's mentions belong to the entity. Each entity counts once, whatever its size,
so a mapping that links nothing scores 0 and one huge entity cannot carry the score.

**Relationships:** the unit is a unique typed, directed edge between two key entities within the
test corpus.

- A predicted edge's endpoints are mapped to key entities by majority: a predicted cluster maps to
  the key entity holding **strictly more than half** of its mentions. A cluster with no such entity
  cannot match anything, so any edge touching it is false. A cluster that merges a small part of
  another entity still maps to its majority entity; the merge is already penalized by identity.
- Each key edge yields at most one true positive. Further predicted edges that map to the same key
  edge (for example, from an entity split across clusters) are false positives.
- Predicted relationship types are aligned to key types one-to-one, maximizing matched edges
  (Hungarian assignment; ties broken by the lexicographic order of type names). Edges of an
  unaligned predicted type are all false. Edges of an unaligned key type are all missed.
- Scored as precision, recall and F1 over edges, micro-averaged.

**Field roles:** accuracy averaged per field, so abundant easy fields do not dominate.

**Abstention:** the share of fields, types and relationships each arm declined to map. An
abstained mention is simply missing: it lowers recall and cannot lower precision.

**Degenerate outputs:** an empty or unparseable mapping scores 0 on every metric. A metric whose
denominator is zero for a given replicate is reported as undefined, and any gate condition using
it fails for that replicate.

**Reference scorer and fixtures, committed before the test.** The scorer is code in the repo, run
identically on every arm. Its fixtures show, at minimum, that: an extra spurious type lowers
precision; an omitted type lowers recall; a false merge lowers precision whatever the mapping
calls the merged cluster; an all-singletons mapping scores 0 entity recovery; the B-cubed
4/9 case above; an edge touching a cluster with no strict majority entity is false; an edge
touching a 9-to-1 merged cluster maps to the majority entity; two predicted edges mapping to one
key edge give one true positive and one false positive.

**No answer-key feedback into any mapping during the test.** Development-window keys may guide
development, identically for every arm, and are disclosed.

### B4. Gate-3 pass thresholds (Dave, 2026-09-27, including the v4 fixes)

Each provider is judged separately, and **the obfuscated stream and the private stream must each
pass on their own.** Each stream runs **5 replicates** (a fresh obfuscation key and field order for
the obfuscated stream, a fresh model seed for all). All arms share a replicate. A failed execution
counts as that replicate's result. Five replicates show variability; they are not a formal
significance test, so the rule asks for consistency across all of them, and the safety results
describe those five runs, not a guaranteed rate.

1. **Primary:** H+S2's identity F1 exceeds H's by at least 0.10 on the mean over replicates, and
   is higher than H's in **all 5** replicates.
2. **Safety floor:**
   - H+S2's false-merge rate is at most 0.05 in every replicate;
   - it is no more than 0.02 above H's on the mean;
   - relationship F1 is no more than 0.05 below H's on the mean.
3. **Absolute floor:** H+S2 recovers at least 60% of repeated entities (entity recovery, B3) on
   the mean. Beating weak baselines is not enough.
4. **Beats the raw-sample baseline:** H+S2's identity F1 exceeds B3's by at least 0.05 on the
   mean, and is higher in at least 4 of 5 replicates.
5. **Budget:** within $5 of API spend per stream per replicate.

There is no waiver. If H's identity F1 is so high that 0.10 of headroom does not exist, the result
is reported as **"insufficient headroom to show added value"**, which is not a pass. A provider
that passes earns a claim for that provider only.

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
- the eligibility manifest, exclusions, censoring with its sensitivity analysis, and full
  denominators;
- every registered run, failed and unmeasurable ones included;
- the matcher fixtures, the reference scorer and its fixtures, the obfuscation and
  canonicalization rules, and the identifier domains;
- sanitized private-stream fixtures.

## Sign-off

| Item | Decision | By, when |
|---|---|---|
| Eligible population (A2) | English Wikipedia, with a multilingual re-run after the slice | Dave, 2026-09-27 |
| Gate-4 thresholds (A9) | Wikimedia's model reported, not required; calibration by equivalence test, the 90% interval for observed/expected positives inside [0.8, 1.25] | Dave, 2026-09-27 |
| Private stream (B2.3) | lifeos dev-worker and sprint log (default; swappable) | karpathy default, 2026-09-27 |
| Gate-3 thresholds and budget (B4) | As proposed: identity F1 at least 0.10 above H on the mean and higher than H in all 5 replicates; at least 0.05 above B3 on the mean; false merges ≤ 0.05 per replicate, $5 per stream per replicate. v4 fixes two defects found in review (round 3): the floor now uses entity recovery ≥ 60% instead of identity F1 ≥ 0.60, and the high-baseline waiver is removed | Dave, 2026-09-27 (v4 fixes confirmed) |
| Overall hours cap | 60 hours of sprint time | Dave, 2026-09-27 |
| Pilot and open checks (A10) | ⟨pending⟩ | |

## Change log

**v4.1 (2026-09-27)**, answering the Codex round-4 delta review: B-cubed on unmodified partitions
(reference scorer) with a 4/9 fixture; strict-majority endpoints and one true positive per key edge;
calibration sensitivity bounds the observed/expected ratio per bootstrap replicate; the equal-skill
simulation must use two different predictors; the B4 sign-off row states the actual rule.

**v4 (2026-09-27)**, answering the Codex round-3 review and adopting its cheaper equivalents for a
60-hour scope.

| Round-3 item | Change |
|---|---|
| B-cubed semantics wrong; singletons; unmatched types | B3: standard B-cubed (Bagga–Baldwin, Cai–Strube for twinless mentions), micro over mentions, no type alignment for identity; degenerate outputs; reference scorer with fixtures |
| All-singletons scores 0.667 and clears the 0.60 floor | B3/B4: floor moved to entity recovery (each repeated entity counts once; all-singletons scores 0) |
| One large entity can dominate within a type | B3: stated as intended for B-cubed; entity recovery guards the other side |
| Relationship unit and endpoints undefined | B3: unique typed directed edges, majority endpoint mapping, deterministic type alignment |
| Bootstrap null was not zero skill | A6: simulation of two equal-skill predictors, frozen settings, sanity check only |
| Power ignored the interval vetoes | A6: one deciding interval; one-way intervals become diagnostics |
| Calibration extension undefined | A8: fixed length, never extended; A9: fewer than 20 expected positives is inconclusive |
| B1 point estimate caught by the interval rule | A9: condition 2 explicitly uses no interval |
| Unknown eligibility and calibration not in sensitivity | A4: include/exclude treatment for unknown eligibility; calibration at both censoring extremes |
| Audit missed eligibility changes; retention | A4: audit at T+72 h counts label and eligibility changes against all eligible edits; the recorder archives everything |
| Coverage denominator; clock skew | A3: scheduled population includes edits never received live; offset over 1 s counts as downtime |
| B4 waiver could pass a regression | B4: waiver removed; "insufficient headroom" is not a pass |
| Identifier domains could leak structure | B2: domains are private to the transformer, published after commitment |
| Proportionality | Hourly digests; one deciding interval; one provider evaluated first; bounded private corpus keyed from source ids; repair-operation metric dropped |

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

## Dated notes after sign-off

### 2026-09-27: what arm H contains (pointer, no change in meaning)

B1 defines **H** as "heuristics alone (System 1 rules and local embeddings)". What "System 1 rules"
contains, and the disposition of Rebmann, Rehse and van der Aa (BPM 2022) as a component of H
rather than a fourth arm, is fixed in [decision 0010](decisions/0010-gate3-h-arm.md). Reason: the
arm needed a concrete definition before anyone builds it (s2w#4). This note changes no arm, metric,
threshold or stream; B1's text stands as signed.
