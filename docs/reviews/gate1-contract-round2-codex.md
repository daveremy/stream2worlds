**I would still revise before sign-off.** V2 fixes several major problems, especially answer-key filtering, selective abstention, and copying B2. The remaining blockers concern outcome ascertainment, proof of timely issuance, and statistical pass rules.

The revision-range proposal is broadly sound for ordinary page histories. **Wiki-global revision IDs do not themselves invalidate a page-qualified interval.** The qualification is that revision IDs do not always follow historical revision order.

For the original 17 findings:

| # | Status | Reason |
|---|---|---|
| 1 | **Partly resolved** | Provenance fixes unrelated-revert attribution, but ID intervals need history-order exceptions, explicit join keys, and validation fixtures. |
| 2 | **Resolved** | The outcome now concerns the recognized revert itself; later tagging is correctly separated from it. |
| 3 | **Partly resolved** | Recorder receipt and commitment deadlines are explicit; hourly digests still cannot independently establish those deadlines, and delayed cutoff evidence remains ambiguous. |
| 4 | **Resolved** | Scoring committed mappings before answer-key filtering removes the proposal-selection oracle. |
| 5 | **Partly resolved** | Ownership, reporting, and a censoring ceiling improve matters; “against each predictor” does not define the worst case for comparative skill. |
| 6 | **Resolved** | Predictor-plus-fallback is correctly identified as the product, and B2 availability alone determines its comparison subset. |
| 7 | **Resolved** | The primary arm explicitly excludes moderation scores; the specialist model’s target mismatch is acknowledged. |
| 8 | **Partly resolved** | Most freeze protections are present; development boundaries, mutable model handling, and Part B’s complete freeze protocol still need specification. |
| 9 | **Partly resolved** | Observed/expected totals fix the zero-prediction example, but interval overlap is not an equivalence test. |
| 10 | **Partly resolved** | Paired resampling and development-based sizing help; choosing the widest one-way interval does not establish coverage under crossed dependence. |
| 11 | **Partly resolved** | Pairwise identity is much clearer; entity mentions, aggregation, type matching, temporal relationships, and undefined cases remain unspecified. |
| 12 | **Resolved** | Weighted repair operations are now honestly presented as a proxy rather than human time. |
| 13 | **Partly resolved** | Pool, model, schema, and session controls improve comparability; paired transformations, execution assistance, and complete budget accounting remain open. |
| 14 | **Partly resolved** | A primary endpoint and safety margins replace the weak alternative route, but uncertainty and seed aggregation remain inadequate. |
| 15 | **Resolved** | Additive bot/human/negative loss contributions are correctly described as descriptive. |
| 16 | **Partly resolved** | The provenance claim is narrower; scalar hashing still does not preserve every real relationship or establish inferability. |
| 17 | **Partly resolved** | Claims and publication rules improve substantially, but the pilot is pending and independent issuance timing remains unproven. |

The new and remaining issues below are ranked by severity.

1. **CRITICAL — Hourly public digests do not prove commitment by the forecast deadline.**

   A forecast nominally issued at 12:00:15 can be constructed at 12:40, after its outcome, and included in the 13:00 digest. The public digest proves existence by 13:00. Its embedded local timestamp does not prove existence at 12:00:15.

   A separate ingress process establishes evidence receipt under the recorder’s trust assumptions; it does not independently attest when predictions were committed.

   **Required change:** have an independent commitment receiver record receipt by each deadline, or explicitly adopt a trusted evaluator that enforces and attests deadlines. Hourly publication can remain an audit mechanism, but must not be presented as independent proof of timely issuance.

2. **CRITICAL — Cutoff eligibility and negative ascertainment still have unresolved event-time versus observation-time semantics.**

   Consider E received at 12:00:00, with cutoff 12:00:15. A qualifying revert happens at 12:00:10 but arrives at 12:00:20. Is E excluded? A1’s question says yes; an eligibility manifest constructed solely from information available at the cutoff says no.

   This is repairable without changing the intended question: issue forecasts for provisional candidates, then determine actual survival to the cutoff using provenance received by the ascertainment deadline. Specify that late evidence can establish **ineligibility**, and that unresolved survival is not silently treated as survival.

   Separately, a connected recorder with no detected gap does not prove that every upstream revert event arrived. A revert occurring at T+29 minutes but delivered at T+46 becomes a false negative under the current rule unless another mechanism detects incompleteness. EventStreams supports historical replay, but that facility does not itself establish a 15-minute delivery bound. [EventStreams documentation](https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams_HTTP_Service#Historical_Consumption)

   **Required change:** define T, provisional versus finalized eligibility, the negative-label completeness rule, and what late primary provenance does to run validity. The later audit should inspect late provenance as well as tags. A tag seen at seven days can concern a revert *outside* the 30-minute horizon, so an unqualified tag/label comparison is insufficient.

3. **HIGH — The provenance matcher is a good replacement, but its claim of exactness is too broad.**

   For ordinary chronologically ordered revisions, this is correct:

   `same wiki ∧ same page_id ∧ oldest_id ≤ E.rev_id ≤ newest_id`

   Unrelated revisions interleaved on other pages do not matter. Wikimedia’s own analysis discussion recommends this page-qualified range join for multi-revision reverts. [Wikimedia analysis](https://phabricator.wikimedia.org/T429049)

   However, the premise “revision IDs are monotonic” needs qualification. MediaWiki documents exceptions involving imported histories. Its range implementation compares **revision timestamp, then revision ID**, rather than just numeric ID. [Revision-table documentation](https://www.mediawiki.org/wiki/Manual:Revision_table#rev_id), [RevisionStore implementation](https://doc.wikimedia.org/mediawiki-core/master/php/RevisionStore_8php_source.html)

   **Required change:** either reconstruct the relevant historical ordering or explicitly identify and handle exceptional histories. Excluding old imported revisions as forecast targets does not necessarily remove imported revisions from a target’s page history or revert-range endpoints.

   Also freeze:

   - `(wiki_id, page_id)` matching rather than page titles;
   - inclusive endpoints and treatment of non-exact undos;
   - deduplication and attribution when multiple reverts target E;
   - malformed/missing provenance handling;
   - fixtures for single-edit undo, multi-edit rollback, unrelated undo, self-revert, and revert-of-revert.

   “Verified on the live stream” demonstrates field availability. It does not demonstrate matcher correctness or completeness.

4. **HIGH — Part B still lacks an executable definition of its primary scoring population.**

   A record can mention a page, editor, revision, issue, PR, and review seat. “Pairwise same-entity links between records” does not specify which **entity mentions** are being compared.

   Aggregation can materially change the answer. An entity appearing 1,000 times contributes 499,500 true pairs; one appearing twice contributes one. A pooled pairwise F1 can therefore reward recovery of one frequently repeated entity while overlooking many types.

   **Required change:** freeze mention extraction, type alignment, admissible predicted mentions, micro/macro aggregation, treatment of omitted types and abstentions, and zero-denominator conventions. Extra predicted types and relationships must count as errors rather than disappearing during alignment. Relationship matching also needs temporal validity and endpoint-matching rules.

   The singleton statement needs its condition: singleton predictions have zero recall **when true links exist**. If there are no true links, recall is undefined without a convention.

5. **HIGH — Three seeds plus an unspecified “paired interval” cannot carry B4’s inferential claims.**

   Three stochastic runs can describe variability. They are not a defensible default basis for a generic 95% confidence claim. For illustration, with three independent, non-tied paired differences, even three positive signs give a one-sided exact sign-test p-value of \(1/8\). A parametric interval could behave differently, but its assumptions must be justified.

   B4 also leaves open whether thresholds apply to mean seed scores, pooled predictions, every seed, or a selected mapping. These are different tests. B3 superiority and the safety margins currently use point estimates without specified uncertainty.

   **Required change:** define the replicate and aggregation rule, share each transformation and data split across arms, and choose a justified number of independent runs and an interval method before testing. For inferential safety claims, use upper confidence bounds for false-merge limits and lower bounds for relationship non-inferiority. Give the B3 comparison paired uncertainty too.

   Repeated seeds on one dataset measure model randomness; they do not establish robustness across unfamiliar streams. V2’s case-study restriction correctly acknowledges that distinction.

6. **HIGH — The observed/expected calibration gate rewards imprecision.**

   Suppose \(O/E=1.20\).

   - Interval `[0.70, 1.70]`: **passes** because it overlaps `[0.9, 1.1]`.
   - Interval `[1.15, 1.25]`: **fails**, despite the same point estimate and much greater precision.

   The rule accepts uncertainty about calibration as evidence for calibration.

   **Required change:** choose an approved tolerance and require a prespecified confidence interval to lie **entirely inside it**, using an appropriate equivalence procedure. Define confidence level, dependence treatment, and zero-expected-positive handling.

   The ratio establishes calibration of total event counts only. Opposing errors across risk groups can cancel. That is acceptable for the narrow phrase “calibrated overall”; stronger calibration claims need more than the descriptive reliability chart.

7. **HIGH — “Use the widest interval” is not a validated dependence correction.**

   Page, editor, and day dependence can operate simultaneously. Separate one-way bootstraps can each omit part of the uncertainty; selecting the widest does not necessarily fix that. Multiway dependence can require a method that accounts for multiple dimensions jointly. [Bakshy and Eckles](https://arxiv.org/abs/1304.7406)

   There is an additional mechanical problem: the widest interval need not have the lowest lower bound. Selecting it could produce a pass while another reported interval crosses zero.

   **Required change:** prespecify and validate the primary inference method against plausible development-data dependence. Treat other intervals as sensitivity analyses, with an explicit disagreement rule. An envelope avoids selectively ignoring an adverse interval, but does not itself prove valid coverage.

   A8 also needs a target power level and a rule for a required duration exceeding 21 days. Capping an underpowered design at 21 days does not make it adequately powered. Seven day blocks remain seven blocks regardless of bootstrap replicate count.

8. **HIGH — The censoring “worst case” is not defined for the quantity being claimed.**

   Maximizing a predictor’s loss is not the same as minimizing its advantage over a baseline.

   With \(p=0.20\) and baseline \(b=0.01\), assigning a censored case \(y=1\) hurts the predictor’s absolute loss most—but hurts the baseline even more. That assignment actually **helps comparative skill**.

   For the sign of Brier improvement, the relevant per-case contrast is:

   \[
   (p-y)^2-(b-y)^2=(p-b)(p+b-2y).
   \]

   **Required change:** optimize the sensitivity analysis against each claimed comparison and gate, using labels consistent with already-known evidence. Specify what happens when the pass reverses.

   The 2% ceiling is not sufficient protection in a rare-event task: fewer than 2% missing cases can contain most positives. A sensitive result could still be reported, but should not automatically earn an unqualified pass merely because censoring stayed below the ceiling.

9. **MEDIUM — The 60-second arrival limit is reasonable, but creates a delivery-conditioned population.**

   With accurate clocks, it bounds issuance age at 75 seconds and leaves at least 28 minutes 45 seconds before the horizon. That addresses the worst delay loophole.

   It does **not** create a constant-age forecast, and it excludes edits during lag or outages. There is no ceiling on those exclusions, unlike censoring. A run could pass on a small, unusually healthy subset of its scheduled window.

   **Required change:** freeze clock-skew handling, deduplication and first-receipt rules; report latency strata and collection uptime; and specify a coverage requirement if the claim concerns operation across the scheduled live window. Pilot both eligibility-stream and outcome-provenance delivery delays. The pending pilot list currently omits the latter.

10. **MEDIUM — Obfuscation preserves literal equality, not necessarily real links.**

    Hashing numeric ID `42` and a URL containing `/42` produces unrelated hashes. Conversely, page ID `42` and user ID `42` may receive equal hashes despite naming different entities. The assertion that cross-field links therefore survive is too strong.

    **Required change:** publish the identifier-domain and canonicalization rules, specify field-name permutation, and use one transformation consistently across development and test within a replicate. Mark relationships destroyed by the transformation as unobservable. Fresh hash keys alone do not remove recognizable schema structure.

    Equal input-token budgets are a reasonable test of H+S2’s summarization advantage. They should be supplemented by frozen execution assistance, output limits, retry policy, and accounting for preprocessing and all model calls.

There are also two interpretation limits worth preserving. B4 can pass with low absolute identity recall and poor relationship quality if H and B3 are worse; it therefore establishes relative improvement unless you add absolute quality floors. Its fixed improvement margins can also make passing impossible when a baseline is already near perfect. Neither is inherently wrong, but both should be deliberate decisions.

**Sign-off remains blocked** by independently enforced issuance, explicit eligibility/ascertainment semantics, a validated provenance matcher, executable Part B scoring, defensible uncertainty rules, and the calibration equivalence fix. A10’s pilot, Dave’s calibration approval, and B4’s thresholds/budget are also explicitly unfinished. Freeze the missing operational details and the policy for externally changing model versions before opening any test window.

**Verdict: revise.**