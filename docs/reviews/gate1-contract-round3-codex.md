**Revise, with a narrower set of blockers.** The trusted commitment receiver is acceptable for this experiment. Five paired replicates are also acceptable for the explicitly descriptive, two-case-study claim. I would not require independent timestamp infrastructure or formal confidence bounds for every B4 comparison.

| Round-2 finding | Status | Reason |
|---|---|---|
| 1. Timely commitment | **Resolved** | The receiver enforces deadlines under an explicit trusted-evaluator model; public digests no longer claim independent deadline proof. |
| 2. Eligibility and ascertainment | **Partly** | Retrospective survival and ascertainment are clear, but the late audit needs an eligibility-change rule, an exact denominator and a feasible replay schedule. |
| 3. Provenance matcher | **Resolved in specification** | Page-qualified ordering, inclusive ranges, malformed evidence and fixtures are specified; passing those fixtures remains an A10 prerequisite. |
| 4. Executable Part B scoring | **Partly** | Mentions and alignment are substantial improvements, but B-cubed’s singleton semantics are wrong and unmatched predictions still lack an executable aggregation rule. |
| 5. Replicate uncertainty | **Resolved for the narrowed claim** | Five paired runs with explicit aggregation support a descriptive engineering gate; they do not establish population-level safety or generalization. |
| 6. Calibration | **Resolved in principle** | Interval containment is a genuine equivalence criterion; Dave’s tolerance approval and a fixed stopping rule remain necessary. |
| 7. Dependence and power | **Partly** | Multiway resampling is appropriate to investigate, but the proposed null is not zero skill and sizing ignores the additional interval vetoes. |
| 8. Censoring sensitivity | **Partly** | The contrast correctly handles zero-skill comparisons, but unknown eligibility and calibration are not covered. |
| 9. Delivery-conditioned population | **Partly** | Coverage thresholds address the main objection; clock-skew handling and the denominator for late or never-received edits remain unspecified. |
| 10. Obfuscation and fairness | **Partly** | Domain hashing, canonicalization and shared assistance fix the earlier mechanics; clarify whether identifier-domain declarations are exposed to the arms. |

The substantive new problems and incomplete repairs are these.

1. **B3 combines reasonable components into a metric that still cannot be implemented unambiguously.**

   Standard B-cubed includes the mention itself. For a correctly detected mention \(m\), recall is \(|P(m)\cap G(m)|/|G(m)|\). Consequently, a true singleton has defined recall of **1**, even if its predicted cluster incorrectly merges it with others. Missing and spurious mentions also need explicit scoring conventions. These are established concerns in the [reference scoring paper](https://aclanthology.org/P14-2006.pdf).

   That creates three concrete problems in v3:

   - Excluding types with no repeated entities because “recall is undefined” is mathematically incorrect. Excluding them from precision too can hide catastrophic merging of, for example, unique revision entities.
   - All-singleton predictions on gold clusters consisting entirely of pairs score precision 1, recall 0.5 and F1 **0.667**. Thus the 0.60 absolute floor does not itself establish successful identity linking.
   - Macro-averaging by type prevents one **type** dominating. It does not prevent one frequently repeated **entity within a type** dominating. Correctly recovering one 1,000-mention entity while splitting 100 two-mention entities gives that type B-cubed F1 about **0.957**.

   Hungarian alignment of **types** is legitimate and does not turn the identity metric into CEAF. But “unmatched predicted types count as false positives” needs an actual denominator: a macro-average solely over key types has no obvious place to charge an extra predicted type containing only spurious mentions. Alignment ties also need a deterministic rule.

   **Minimum repair:** freeze exact formulas or a reference scorer, including missing/spurious mentions, unmatched types, empty outputs and macro aggregation. Keep singleton-only types in precision assessment. Add small scoring fixtures demonstrating that extra types hurt, omitted types hurt, and false merges cannot disappear through alignment. Correct the claim about entity dominance; changing the weighting scheme is optional if mention weighting is intentional.

   Relationship F1 also affects the gate, so freeze its evaluation unit—such as unique typed edges at specified record checkpoints—and endpoint matching. “Falls in the correct key cluster” is insufficient if an endpoint’s predicted cluster merges several key entities.

2. **The proposed bootstrap validation null is wrong; the all-interval rule is conservative but changes the power calculation.**

   Let \(p=b+\epsilon\), with independent, mean-zero noise small enough to keep probabilities valid. Then

   \[
   \mathbb E[(p-y)^2-(b-y)^2]=\mathbb E[\epsilon^2]>0.
   \]

   The noisy predictor is worse than B0 on average. A valid interval may therefore exclude zero on the negative side far more than 7% of the time. Checking only positive exclusions would instead test an easier, negative-skill case rather than the zero-skill boundary.

   **Minimum repair:** use a small prespecified simulation with a known zero expected paired-loss contrast and plausible page/editor/day dependence. Freeze the simulation settings and repeat count. It is a sanity check, not proof of coverage for every possible stream.

   Pigeonhole resampling is a defensible candidate, but its supporting results concern particular crossed-dependence conditions; they do not automatically validate percentile BSS intervals with seven day levels. The multifactor formulation uses products of independently drawn factor weights. [Owen and Eckles](https://arxiv.org/abs/1106.2125)

   Requiring every sensitivity interval to clear zero **does not create the usual multiple-testing false-positive inflation**: passing is an intersection of requirements. It can reduce power, and it cannot repair an invalid primary interval. A8 currently estimates power for the primary interval alone, so it does not establish 80% power for the actual B0 pass rule.

   Either size against that actual conjunction or make the one-way intervals diagnostic. Also explicitly exempt A9’s B1 point-estimate requirement from A6’s blanket wording.

   Finally, “extended per A8” has no defined extension procedure. Prefer a fixed development-sized window; insufficient expected positives at its end means the calibration check is inconclusive. Any extension rule must be frozen before testing.

3. **The Brier sensitivity repair is correct for the stated zero thresholds, but “unqualified pass” reaches further than the calculation.**

   For fixed eligibility, the adverse assignment is \(y=0\) when \(p>b\), and \(y=1\) when \(p<b\), subject to evidence. It maximizes the loss contrast case by case.

   This also works for the **sign** of the bootstrap BSS gate: with the same nonnegative resampling weights and a positive baseline-loss denominator, each resample’s pass/fail sign is governed by that contrast. There is no need for a complicated optimization of the BSS ratio merely because its denominator changes.

   Two omissions remain:

   - Some censored candidates have **unknown eligibility**, not merely unknown outcomes. Specify permissible inclusion/exclusion as well as labels, and define the censoring denominator without presuming those candidates eligible or ineligible.
   - A9’s calibration criterion also determines passage. Adverse Brier labels need not be adverse calibration labels. For known eligible cases with fixed predictions, calibration sensitivity is cheap: evaluate the evidence-consistent minimum and maximum possible observed-positive totals.

   Alternatively, explicitly restrict the calibration claim to the uncensored cohort. The current “unqualified” language should not suggest robustness that was checked only for the two skill comparisons.

4. **B4’s ceiling exception can pass a System 2 regression.**

   Suppose every replicate has:

   \[
   F1_H=0.95,\qquad F1_{H+S2}=0.90,\qquad F1_{B3}=0.80,
   \]

   with the precision, relationship and budget floors satisfied. Because H exceeds 0.90, condition 1 disappears, and H+S2 passes despite losing to H.

   The exception also leaves “H already scores above 0.90” undefined across replicates.

   **Minimum repair:** remove the waiver. A ceiling-limited case can report “insufficient headroom to demonstrate the specified added value.” If Dave wants a smaller ceiling-adjusted margin, define it before testing and retain an explicit improvement requirement over H.

   The **five-replicate consistency rule itself is proportionate**. Keep it, count failed executions under a frozen rule, and describe the safety floor as observed performance in those five runs. I withdraw round 2’s demand for formal upper confidence bounds on every B4 safety comparison under this narrower claim.

5. **The new audit and information-boundary rules need a few operational decisions.**

   A late-discovered revert before C can change an edit from eligible to ineligible even when its positive label would remain positive. The seven-day audit must therefore examine **eligibility changes as well as negative-to-positive label changes**. Define the 1% denominator explicitly and state what audit evidence does to run validity.

   Confirm the chosen stream’s retention or use a rolling archive/audit. EventStreams documents roughly **7–31 days depending on configuration**, so replaying at exactly seven days has little margin at the lower end; replaying the whole window after its conclusion is more demanding. [EventStreams historical consumption](https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams_HTTP_Service#Historical_Consumption)

   Define late-arrival coverage over the reconstructed scheduled population, including edits never received live. Freeze a clock-offset tolerance and what happens when it is exceeded.

   Finally, identifier domains should be **transformer-internal metadata** unless supplying them is deliberately part of the task. Showing an arm that fields belong to “page,” “user” and “revision” domains supplies part of the type and identity structure it is supposed to recover. Publish that metadata after commitment, or narrow the claim accordingly.

For the 60-hour scope, I would reduce these requirements:

| Requirement | Cheaper equivalent |
|---|---|
| Public digest pushes every minute | Hourly publication of an append-only commitment log’s digest. Under the accepted receiver trust model, minute publication buys little deadline credibility. |
| Primary bootstrap plus three mandatory interval vetoes | One prespecified multiway primary procedure, a small correct simulation check, and one-way diagnostics. This preserves a clear inferential decision without a separate power requirement for every veto. |
| Mandatory hosted and local/client-provider tracks | Evaluate one frozen provider first; add another after the slice. The contract already makes claims provider-specific. |
| Exhaustive private-stream annotation over an unrestricted corpus | Freeze a bounded held-out event corpus, generate keys from source identifiers where possible, and manually inspect representative cases. Keep the claim limited to that corpus. |
| Minimum weighted repairs to reach the answer key | Count discrepancies under a fixed canonical edit procedure, or omit this secondary metric. Optimizing a repair sequence buys little without a human-time claim. |
| General historical-order reconstruction | Resolve the required range endpoints and exceptional cases; censor unresolved histories under the existing ceiling. Do not build a general MediaWiki history engine. |

The matcher fixtures, executable scorer, held-out data boundary, commitment enforcement and publication of failed runs are worth their cost. They determine whether the result means what it says.

The remaining **sign-off blockers**, ranked, are:

1. **An executable Part B scorer:** correct B-cubed semantics, actual penalties for unmatched predictions, relationship matching, and a clear boundary around domain metadata.
2. **A coherent Part A inference and stopping rule:** replace the false null, reconcile power with the chosen interval rule, and remove unspecified extensions.
3. **Removal or repair of the B4 ceiling waiver:** passing must retain evidence that System 2 adds value over H.
4. **A complete outcome manifest policy:** uncertain eligibility, censoring/calibration scope, audit eligibility changes, coverage denominators and clock handling.
5. **The explicitly unfinished prerequisites:** A10 pilot and passing matcher fixtures, Dave’s calibration approval, and B4 threshold/budget approval. Freeze Part B’s data manifests and scoring configuration before its evaluated runs.

**Verdict: revise.**