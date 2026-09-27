**I would not sign this version.** It has a reasonable framework—live issuance, frozen fits, proper scoring, explicit failures—but the label attribution is unsound, issuance is not fully protected against delay, and Part B can reward access to an answer-key oracle.

I found **no explicit instruction to fit a baseline on test outcomes**; A5 correctly forbids that. The problems are operational loopholes and claims the proposed measurements do not establish.

Below, ranked by severity, **critical** means the issue can invalidate the main result; **high** means it can materially change whether the gate passes.

1. **CRITICAL — A1 assigns the wrong reverting revision.**

   > “the first later revision on the same page tagged `mw-rollback`, `mw-undo` or `mw-manual-revert`”

   A revert on the same page need not target this edit.

   Example: edit E occurs at 12:00; another edit F at 12:05; F is undone at 12:10; E is undone at 12:45. At finalization E carries `mw-reverted`. Your rule pairs it with the 12:10 undo and incorrectly labels E a 30-minute positive.

   **Fix:** require an explicit **target-revision → reverting-revision** association. MediaWiki’s tagging implementation stores `revertId` together with serialized `EditResult` information in tag parameters. That is the relevant provenance, not proximity in page history. [MediaWiki source](https://doc.wikimedia.org/mediawiki-core/master/php/RevertedTagUpdate_8php_source.html)

   Before sign-off, demonstrate that the evaluation’s accessible data source exposes this association. The ordinary revisions API documents tag names, but does not document returning those tag parameters. [Revisions API](https://www.mediawiki.org/wiki/API:Revisions)

   If exact provenance cannot be obtained, change the target to an explicitly reconstructible content-restoration definition and validate that detector. Do not silently approximate MediaWiki recognition with the next revert tag.

2. **CRITICAL — A1/A4 confuse a revert event with the later tagging process.**

   > “Recognized revert: MediaWiki adds the `mw-reverted` tag”  
   > “Final label at T+24 h.”

   Asynchrony does **not** inherently invalidate a label: a late observation can establish an earlier event. But the tag is not simply a delayed, complete receipt for every recognized revert.

   Tagging can depend on approval/patrolling. The update checks whether the reverting revision has itself been reverted or its text deleted; it can skip execution. A revert exceeding the depth limit can cause the update to be skipped, rather than merely tagging the nearest allowed number of edits. [MediaWiki manual](https://www.mediawiki.org/wiki/Manual:Reverts), [tagging implementation](https://doc.wikimedia.org/mediawiki-core/master/php/RevertedTagUpdate_8php_source.html)

   Consequently, events **after the 30-minute horizon** can affect whether its supposed outcome becomes observable. Both sources can agree on “no tag” while a qualifying revert occurred.

   **Fix:** choose and name one estimand:

   - A qualifying revert **occurred within 30 minutes**, determined from exact revert provenance independently of eventual tagging; or
   - A qualifying revert occurred within 30 minutes **and its target tag was observed by a fixed ascertainment deadline**.

   The second is measurable but predicts a mixture of editing behavior and tagging/approval behavior. Twenty-four hours is an administrative deadline, not demonstrated completeness. Use a prespecified later audit and report label changes.

   Also correct:

   > “The reverting edit itself is never tagged.”

   It is not tagged as its **own** target; it can subsequently receive `mw-reverted` when another edit reverts it.

3. **CRITICAL — A3 permits moving the effective forecast time and does not clearly require timely commitment.**

   > “15 seconds after `s2w` receives the edit event”  
   > “Predictors may use evidence that arrived before the cutoff”

   Receipt is controlled partly by your infrastructure. Buffering, reconnecting, or defining receipt after a processing queue can move the cutoff substantially beyond the edit time. An edit arriving after its entire horizon is particularly problematic: already-reverted edits disappear, while surviving negatives remain.

   There is also no explicit maximum interval between evidence cutoff and immutable forecast commitment. An evidence cutoff alone does not prove that a probability was selected without subsequent information.

   **Fix:** use a separate ingress recorder and specify:

   - First receipt at that recorder, not predictor dequeue time.
   - A fixed maximum event age and minimum remaining forecast horizon.
   - Probability committed **by the deadline**, with late output treated as missing.
   - Logged arrival times for every stream and API response; backfilled data never becomes retrospectively available.
   - A common, predictor-independent eligibility manifest.

   The existing pre-cutoff exclusion can be legitimate, but it changes the question to **risk among edits surviving unreverted to the cutoff**. State that conditional population explicitly. A revert’s existence also does not establish that it was visible to the predictor; delayed tags make that distinction essential.

4. **CRITICAL — Part B can reward an oracle that selects correct proposals.**

   > “one scripted evaluator accepts or rejects proposals against the answer key”

   H+S2 could propose every plausible identity key and relationship. The evaluator then selects correct proposals, yielding an excellent accepted model with little actual inference skill.

   This is especially severe if false merges and F1 are measured **after** answer-key rejection. The oracle can prevent every proposed false merge.

   **Fix:** separate two evaluations:

   - **Unaided output:** score a committed mapping or ranked proposal set before answer-key filtering. Count false proposals, omissions, contradictions and abstentions.
   - **Assisted workflow:** measure the work needed to inspect, accept, reject and repair proposals under the same assistance policy for every arm.

   Do not feed test-key decisions back into mappings. If development-key feedback is allowed, disclose it as supervision and give every arm the same access. Freeze the mapping before evaluating later events.

5. **HIGH — A4’s common censoring does not make censoring harmless.**

   > “Censored edits are excluded from scores, counted, and reported per predictor, so a predictor cannot gain from censoring.”

   That conclusion is false. A common exclusion can still disproportionately remove cases where one predictor performs badly. Deletion, suppression, busy pages, stream gaps and tag disagreements are plausibly related to both outcomes and prediction errors.

   Moreover, the stream and Action API are two observations of substantially the **same underlying tagging system**, not independent ground-truth mechanisms. Agreement validates delivery consistency, not label correctness.

   **Fix:** make censoring a single evaluator-owned decision, independent of forecasts. Use reconciliation to recover labels where possible; do not automatically discard an API-confirmed outcome merely because the stream missed it. Distinguish unrecoverable labels from predictor failures.

   Report missingness by time, page activity, predictor score range and available edit characteristics. Prespecify a maximum unresolved fraction and a tipping-point or worst-case analysis: could plausible labels for excluded edits reverse the claimed improvement? Include failures discovered after the horizon but before ascertainment, which the present deletion rule misses.

6. **HIGH — A6’s abstention claim is mathematically wrong, and A5 creates a selection loophole.**

   > “an abstention is replaced by the B0 probability, so abstaining can never raise skill.”

   It can raise skill relative to issuing a poor forecast. With B0 = 0.01, a predictor’s 0.9 forecast on a negative scores 0.81; abstaining scores 0.0001.

   All-abstention earns exactly zero BSS, so it cannot by itself pass a strictly positive gate. But selective fallback can substantially improve the system’s score. That is acceptable **if the evaluated product is explicitly a predictor-plus-fallback policy**.

   > “head-to-head comparison with B2 uses only edits where every predictor has a forecast”

   If abstention removes a row here, a predictor can choose the B2 comparison population.

   **Fix:** score the committed fallback policy over all eligible, labeled edits. Define the B2 subset solely by **B2’s timely availability**, never by other arms’ willingness to answer. Apply fallback consistently to comparable metrics, report genuine-answer coverage, and prespecify any minimum coverage requirement.

7. **HIGH — A3/A5 do not exclude simply copying B2.**

   > “Predictors may use evidence that arrived before the cutoff”  
   > “The 15 seconds let Wikimedia’s own score arrive”

   As written, `s2w` can ingest Wikimedia’s score, recalibrate it, and potentially pass the gate. That demonstrates integration of a specialist predictor, not independent forecasting from a learned world model.

   **Fix:** declare the input policy. If the claim concerns independent skill, exclude B2 and other downstream moderation scores from `s2w`’s primary inputs. If they are legitimate product inputs, report separate results with and without them.

   Keeping B2 as a reported comparator rather than a mandatory hurdle is defensible. But remove “a model built for this exact job”: its target is not established as identical to this contract’s target. The official model card also links training/data repositories, so describe the precise unresolved target information rather than broadly declaring it unpublished. [Model card](https://meta.wikimedia.org/wiki/Machine_learning_models/Production/Language-agnostic_revert_risk)

8. **HIGH — A8’s freeze leaves substantial opportunities for leakage and selection.**

   > “Development window: any data before the freeze”  
   > “7 consecutive days starting at least 24 hours after the freeze”

   An edit timestamp before freeze does not mean its mature label was available before freeze. Historical API responses can also contain later account state, tags or page metadata that were unavailable at the historical forecast time.

   The test start date is unspecified, as are online learning, mutable LLM memory, provider changes, repeated attempts and model selection.

   **Fix:** freeze exact development and test dates, ascertainment cutoffs, feature extraction, prompts, model versions, preprocessing, calibrators, fallback policies and scoring code. Training labels must actually have been available before fitting. Reconstruct historical features as of their issuance cutoffs.

   Choose either frozen parameters or a fully specified online update policy using only outcomes available at each update. Ordinary accumulation of past stream events can remain allowed. Register every attempted run; fixes after seeing test results require a new test period, with the previous attempt retained.

9. **HIGH — ECE ≤ 0.02 is easy to achieve without useful rare-event forecasts.**

   > “Expected calibration error ≤ 0.02”  
   > “10 equal-count bins”

   For positive-event probabilities,

   \[
   \mathrm{ECE}=\sum_b\frac{n_b}{n}\left|\bar p_b-\bar y_b\right|.
   \]

   A predictor that always outputs zero has ECE equal to prevalence. **If the revert rate is 1%, it passes with ECE = 0.01 despite never assigning any chance to a revert.** A constant correct base rate is perfectly calibrated in population and has no discrimination.

   Those examples do not necessarily pass the BSS requirements; they show that this calibration threshold adds little protection. Equal-count bins can also average away severe miscalibration within the highest-risk decile. Binning and sample size affect the estimate. [Roelofs et al.](https://proceedings.mlr.press/v151/roelofs22a.html)

   **Fix:** retain proper scores as primary. Define ECE on positive-event probabilities, including ties and fallback handling. Add observed versus predicted event totals, calibration-in-the-large, and reliability estimates with uncertainty in prespecified operational risk ranges. Choose calibration tolerances from development prevalence and intended use. If calibration is a pass gate, use an uncertainty-aware equivalence criterion, not just a point estimate below 0.02.

10. **HIGH — Page clustering is sensible but does not cover all dependence; seven days is not a power calculation.**

    > “95% interval from a bootstrap clustered by page”  
    > “7 consecutive days”

    Page clustering addresses edit wars on one page. It misses editors acting across pages, patroller/bot behavior across pages, and common time shocks. Single-dimension clustering can understate uncertainty with crossed dependence. [Bakshy and Eckles](https://arxiv.org/abs/1304.7406)

    Seven days might contain enough information for a narrow comparison. It does not automatically contain enough independent information for small improvements, calibration tails or temporal generalization.

    **Fix:** use development data to estimate power for a prespecified useful improvement, respecting dependence. Freeze the duration before testing. Prespecify page-based inference plus justified editor/time dependence analyses; longer collection may be necessary.

    Bootstrap **paired losses** on the same observations for all predictors. Define:

    \[
    \mathrm{BSS}_j =
    1-\frac{\sum_i(p_i-y_i)^2}{\sum_i(b_{ji}-y_i)^2}.
    \]

    Recompute both sums within each bootstrap sample, holding fitted predictors fixed. Specify bootstrap method, replicate count, weighting and interval type. An edit-weighted score and an average of page-level scores answer different questions.

11. **HIGH — Part B’s answer key and metrics are insufficiently defined.**

    > “entity types, identity key per type, relationships”  
    > “field-role accuracy; identity precision and recall; false-merge rate”

    These are metric names, not executable definitions. Identity precision could mean correct key selection, pairwise coreference, or entity-cluster accuracy. Different definitions can reverse rankings.

    A false-merge rate over **all possible entity pairs** can be tiny despite catastrophic merges. Singleton clusters avoid false merges while failing identity recall. Field-role accuracy can be dominated by abundant easy fields.

    There may also be several equally valid identity keys. Matching the author’s chosen field is not the same as identifying entities correctly.

    **Fix:** freeze evaluation units, denominators, matching rules, aggregation and treatment of undefined cases. Score induced identity behavior on later records, allow equivalent keys, and report both merge precision and recall. Specify relationship direction, type and temporal validity.

    Establish an adjudicated key with ambiguous/unobservable cases marked explicitly. Committing an answer key establishes timing, not correctness.

12. **HIGH — Part B’s correction-minute estimate does not establish “useful faster.”**

    > “the time a human would spend on each correction is estimated with the same rule in both arms”

    Applying the same arbitrary weights does not make them valid. A single schema repair can fix thousands of records; one false merge may require extensive investigation. Proposal inspection, error discovery, rejection and verification consume time even when no correction is made.

    An answer-key evaluator knows instantly what a human must first discover.

    **Fix:** either call this a **weighted repair-operation proxy**, or validate its weights with a small, blinded, counterbalanced human study using a common interface. Include inspection and verification time, uncertainty, and a stopping criterion based on usable quality. Report total setup time, including S2 latency and manual preparation.

    A claim of “30% fewer estimated operations” is supportable without such validation; “30% less human time” is not.

13. **HIGH — B1/B3 do not yet define a fair arm comparison.**

    > “the same model, given N raw events in one prompt”  
    > “Run on both System 2 providers”

    N is unspecified. H+S2 may receive summaries of thousands of events while B3 sees a tiny raw sample. H+S2 may produce executable mappings while B3 is merely asked for descriptions. Differences would then confound reasoning machinery, information quantity and output requirements.

    The client-agent condition could also inherit repository knowledge, prior conversations or the private stream’s schema.

    **Fix:** give arms the same development event pool and freeze sampling, ordering, access permissions, initial state and output schema. Provide the same execution/scoring harness and assistance policy. B3 should be competently prompted for the actual mapping task.

    Freeze each provider/model separately, clear prior session state, log all calls and count retries. Report dollar cost, tokens, latency and local compute assumptions; a nominally free local model is not meaningfully constrained by a $5 API-spend cap.

14. **HIGH — B4’s thresholds allow passing through a weak metric while quality deteriorates.**

    > “F1 … by at least 0.10 absolute, **or** cuts correction minutes by at least 30%”  
    > “false-merge rate is no higher than H’s”  
    > “beats B3 on F1 or correction minutes”

    Under the effort route, identity or relationship quality could decline materially. A mean F1 improvement can hide one component becoming worse. Any positive difference against B3 counts, however tiny or noisy.

    “No higher” on an observed rate is neither a statistical noninferiority test nor an absolute safety standard. Matching an unsafe H is still unsafe. Two observed zero rates do not establish equal risk.

    **Fix:** choose one primary benefit endpoint and add quality floors and noninferiority margins for the others. Prespecify a meaningful B3 improvement, not merely `> 0`. Report paired uncertainty across independent runs/tasks; use a prespecified repeated-seed protocol where sampling is stochastic.

    State whether both providers must pass or whether claims are provider-specific. The obfuscated and private streams must each pass; do not average them into success. Two stream families remain case studies regardless of how many events they contain.

15. **MEDIUM — The bot sensitivity slice does not bound dependence on bot decisions.**

    > “excluding reverts made by bots … shows how much of the score is really predicting another model’s decision”  
    > “The A6 sensitivity slice bounds that share.”

    Removing bot-positive cases creates an outcome-selected population. Its score difference is not a causal attribution or a bound. Bots also change which edits remain available for humans to revert.

    **Fix:** report additive paired-loss contributions for observed bot positives, human positives and negatives. Call the exclusion analysis descriptive.

    For a separate “human revert” endpoint, define bot and human reverts as competing events and specify their handling before fitting baselines. Do not present that endpoint as the counterfactual result in a world without bots.

    Also, English Wikipedia’s absence from the current official Automoderator deployment list supports the scoping choice, but archive actual configuration at freeze. The official list is not a guarantee about the test week. [Deployment list](https://www.mediawiki.org/wiki/Moderator_Tools/Automoderator#Usage)

16. **MEDIUM — Obfuscation and privacy do not guarantee an unfamiliar, inferable task.**

    > “every identifier … replaced by a salted keyed hash”  
    > “Numbers, timestamps and structure unchanged”  
    > “one no model has seen”

    Numeric identifiers make the obfuscation rule contradictory. Hashing a URL and a numeric ID separately can destroy a real relationship; leaving numeric IDs unchanged can preserve recognizable signatures. Fully hiding semantics can also make several answer keys observationally indistinguishable.

    Private data may already have appeared in Dave’s agent sessions or tools. Conversely, a model need not have seen the raw events to know GitHub-style issues and pull requests.

    **Fix:** publish exact transformation rules, including numeric identifiers, namespace handling and cross-field equality. Keep the same transformation across development/test within a run, and use fresh sealed transformations for independent runs. Score structural recovery up to equivalent renamings rather than unknowable semantic labels.

    Replace “no model has seen” with an auditable claim about nonpublic provenance and what the evaluation session could access. Include a genuinely withheld schema or independent owner’s stream before claiming general unfamiliar-stream capability.

17. **MEDIUM — Several supporting claims and publication rules invite avoidable dismissal.**

    > “29 (53%) were tagged within 30 minutes … So 30 minutes captures about half of eventual reverts”

    That does not follow. Tag delay combines time-to-revert and subsequent tagging delay. Tags arriving after 30 minutes may correspond to very fast reverts. Sampling newly tagged edits during 2.5 minutes is also not a cohort of newly issued edits, and batch jobs can make the 55 observations highly dependent.

    **Fix:** follow a prespecified inception cohort and separately measure edit-to-revert and revert-to-tag delays. The pilot supports only its observed tagging-delay statement.

    > “BSS against B1 > 0 (point estimate). We beat the obvious features.”

    A point-estimate advantage supports “lower observed loss this week,” not reliable superiority. Freeze B1’s feature definitions, missing-value handling, regularization and tuning. Require paired uncertainty for the stronger claim.

    > “B0 … Whether there is any skill at all”

    Beating a stale development prevalence can reflect better base-rate tracking without edit-level discrimination. Report discrimination separately and consider a stronger prespecified causal base-rate baseline if `s2w` adapts online.

    > “Nothing here generalizes to other streams without Part B.”

    Part B tests schema/identity recovery, not forecast transfer. Passing it does not establish forecast generalization.

    Finally, preregister the two non-English wikis, retain every failed run, and publish the scoring implementation, public-data manifest, forecasts, exclusions, label provenance and full denominators. An immutable local ledger is not independent evidence of issuance timing; externally timestamped commitments would strengthen that claim. For the private stream, provide reproducible sanitized fixtures or independent verification where feasible.

**Verdict: revise before sign-off.** The minimum blockers are exact revert attribution, an honest ascertainment definition, independently enforced issuance deadlines, and removal of answer-key selection from Part B’s unaided score. After those repairs, this could support a credible narrow forecast result and two useful schema-inference case studies. As written, a pass would not reliably establish either intended claim.