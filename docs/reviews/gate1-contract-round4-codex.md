V4 resolves the inference/stopping-rule blocker and removes the ceiling waiver. Remaining blockers are two B3 scoring conflicts, incomplete calibration sensitivity, and unfinished prerequisites.

“Resolved” below means resolved in specification; it does not certify unprovided implementation results.

| Round-3 blocker | Status | Reason |
|---|---|---|
| 1. Executable Part B scorer | **Partly** | Identity weighting and domain privacy are clear, but the twinless convention and relationship fixtures conflict with the stated metrics. |
| 2. Part A inference and stopping | **Resolved** | Equal expected loss replaces the false null; one interval decides; B1 is explicitly exempt; the window cannot extend. |
| 3. B4 ceiling waiver | **Resolved** | Insufficient headroom cannot pass or excuse regression against H. |
| 4. Complete outcome manifest | **Partly** | Eligibility, coverage, clock handling and audit rules are repaired; calibration still mishandles unknown eligibility. |
| 5. Unfinished prerequisites | **Partly** | Dave’s calibration approval is recorded; A10, B4 confirmation and the required frozen evaluation artifacts remain outstanding. |

| V4 change-log row | Status | Reason |
|---|---|---|
| B-cubed semantics; singletons; unmatched types | **Partly** | The explicit zero-credit rules contradict Cai–Strube handling; micro weighting and removal of identity type alignment are otherwise coherent. |
| All-singletons clears the floor | **Resolved** | A singleton cannot cover 90% of any entity having at least two mentions. |
| One large entity dominates | **Resolved** | Mention weighting is intentional, while the recovery floor weights repeated entities equally. |
| Relationship unit and endpoints | **Partly** | Majority mapping is defined, but contradicts the merged-endpoint fixture and leaves duplicate matches unresolved. |
| Bootstrap null | **Resolved** | Equal expected Brier scores give the required zero expected paired-loss contrast. |
| Power versus interval vetoes | **Resolved** | The additional vetoes are gone; sizing targets the deciding procedure. |
| Calibration extension | **Resolved** | Fixed duration and an inconclusive outcome replace the undefined extension. |
| B1 point estimate | **Resolved** | A9 explicitly requires no interval for condition 2. |
| Unknown eligibility and calibration sensitivity | **Partly** | Inclusion/exclusion repairs skill sensitivity, but observed-positive totals alone do not bound calibration. |
| Audit eligibility changes; retention | **Resolved** | The audit includes eligibility changes, uses a 72-hour schedule and specifies the eligible-edit denominator. |
| Coverage denominator; clock skew | **Resolved** | Scheduled population includes missed arrivals, with an explicit offset tolerance and downtime consequence. |
| B4 waiver | **Resolved** | Removed without replacing it with another passing exception. |
| Identifier-domain leakage | **Resolved** | Domains remain transformer-private until commitment. |
| Proportionality | **Resolved** | The cheaper equivalents preserve the controls needed for the narrowed claims. |

The remaining technical defects and minimum repairs are:

1. **B3 names a twinless convention that contradicts its own rules.** Cai–Strube inserts missing key mentions as singletons and discards spurious predicted singletons; that is incompatible with “missing scores zero recall” and “every predicted mention counts,” including spurious singletons. This is an executable difference, not just attribution. [Cai–Strube, §2.2.2 and Algorithm 1](https://aclanthology.org/W10-4305.pdf)

   **Repair:** retain the explicit zero-credit rules and replace the Cai–Strube reference with the unmodified-partition formulas in the [2014 reference scorer, §4.2](https://aclanthology.org/P14-2006.pdf). Freeze those formulas and a distinguishing fixture: key `{a,b,c}`, prediction `{a,b,d}` gives precision = recall = F1 = **4/9** under those rules.

   No type alignment is needed for this identity metric. The **90% coverage / 90% purity entity-recovery floor is sound**: it rejects all-singletons, prevents two entities sharing one recovered cluster, and prevents a large entity dominating the entity-level average.

2. **B3’s relationship fixture contradicts majority matching.** A cluster containing nine mentions of A and one of B maps to A. An edge from that cluster can therefore match a key edge from A, despite touching a merged cluster. The promised fixture says it cannot.

   **Repair:** keeping majority matching is acceptable; change the fixture to reject an endpoint **without a strict majority**, and explicitly demonstrate the permitted majority-merged case. Also freeze how multiple predicted edges that map to the same key edge are counted: each key edge can supply at most one true positive, with a stated deduplication or excess-prediction rule. Otherwise splitting an endpoint can produce ambiguous precision and recall.

3. **A4’s calibration extremes are insufficient when eligibility is unknown.** Calibration is \(O/E\), and inclusion changes **both** totals. Minimizing or maximizing \(O\) alone does not identify the adverse ratio.

   For example, suppose established cases have \(O=100,E=100\). Thirty unknown-eligibility candidates each have \(p=0.9\). Excluding them gives \(O/E=1\); an admissible completion including all thirty as negatives gives \(100/127\approx0.787\), outside the tolerance. Both have the same observed-positive total, and thirty censored candidates can remain below the 2% ceiling.

   **Repair:** bound the **observed/expected ratio and its bootstrap interval** over evidence-consistent eligibility and label assignments. A conservative implementation can compute minimum and maximum ratios within each shared bootstrap replicate, then require the lower envelope’s 5th percentile ≥0.8 and the upper envelope’s 95th percentile ≤1.25. Merely selecting two datasets by their unweighted observed-positive totals is insufficient.

   The skill-side repair is sound: using common nonnegative bootstrap weights, an optional candidate is adverse when its maximum permissible loss contrast is positive; otherwise exclusion is adverse.

**A6 introduces no further blocker in principle.** Equal expected Brier score is the correct null. The frozen simulation must actually exercise a nonzero-variance paired contrast with page/editor/day dependence; identical predictions would satisfy equality but make the check vacuous. The settings and result remain required freeze artifacts, not evidence already supplied.

Before signing, complete A10 and obtain Dave’s pending B4 confirmation. That confirmation must match the operative rule: **mean improvement ≥0.10 over H, with positive improvement in all five replicates**. The sign-off table currently says “+0.10 … in all 5,” which is stricter and inconsistent. Keep the existing requirement to freeze Part B’s manifests, scorer, fixtures and configuration before evaluated runs.

**Verdict: revise.**