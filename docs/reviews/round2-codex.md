# round2 codex review (gpt-6-astra, 2026-09-27)

The core experiment is substantially stronger. Immutable forecasts, separate replay histories and an observational first test address real defects. **I would still block the first slice as currently scoped:** it has accumulated a setup product, an agent platform and a hosted service before testing whether System 2 helps. Several examples also contradict the safeguards described elsewhere.

This needs a smaller execution plan and a few explicit contracts, not another architecture document.

**A. The 12 earlier findings**

1. **Evaluation contract — partly resolved.** Forecast/display separation and censoring are correct; missing are issuance cadence, eligible population, repeated-forecast weighting, label finalization, abstention handling and a quantitative pass threshold.
2. **World identification / false merges — partly resolved.** Owner labels and abstention help; confidence ≥0.95 is not demonstrated merge precision, and obfuscation cannot make genuinely unidentifiable relationships recoverable.
3. **Actions corrupting grades — resolved for the first slice.** Observational operation removes S2W’s intervention problem; subsequent automation still needs an explicit causal evaluation design.
4. **Proposal authority / injection — partly resolved.** Credential isolation helps, but automatic acceptance at 0.95 and agent trust tiers reopen authority paths; revoking a repair cannot undo an email already sent.
5. **Branch probabilities — partly resolved.** Bounded forecasts are the right contract, but the DSL, MCP example and rules prose still trigger on `any world (p >= …)`; those contradictions must disappear.
6. **Two replay histories — resolved at the design level.** Both histories and persisted model outputs are specified; implementation must also preserve mapping versions, source cursors, clock inputs and external enrichment used at issuance.
7. **Scaling costs — partly resolved.** Resource metrics are named, but workload limits, backpressure, retained forecasts, checkpointing and replay cost remain unspecified; re-folding is not cheap merely because it is a projection.
8. **Premature Rust interfaces — partly resolved.** “Seams when earned” is sound, but ten crates, an engine router, plugin traits and early public-API controls still anticipate contracts the experiment has not established.
9. **Five-minute value — partly resolved.** Five-minute evidence and thirty-minute verified answers are defensible hypotheses; the headline and sixty-second setup example still imply verified semantic understanding.
10. **Buyer too broad — partly resolved.** Kafka users are a useful recruiting pool; HN/X are distribution channels, and “agents” are clients—none identifies the recurring human job being improved.
11. **Slice tests too much — not resolved.** The new setup, MCP, redaction and cloud work has expanded it again; its launch deliverable requires features explicitly deferred until after the slice.
12. **Metrics reward appearance — partly resolved.** Accuracy and correction time improve the design; a calibration curve and strangers connecting topics can still pass without useful skill, acceptable coverage or repeat value.

**B. New risks, most severe first**

1. **Agent setup can turn routine local access into unauthorized data export.**  
   Reading configuration and possessing credentials do not establish permission to send events to S2W or its model providers. The example jumps from preview to upload without recording the user’s selection or export authorization. An agent seeing a dry-run is not the same as the user seeing it.

   **Fix/test:** Default the experimental slice to local operation. Later, bind cloud connection to an explicitly authorized export manifest: topic, destination, fields, transformations, retention and model destinations. Enforce that manifest on every event; stop or quarantine unexpected fields. Test nested secrets, schema changes and an agent attempting to broaden the export. Non-interactive commands can execute previously authorized policy.

2. **PII hashing creates an unjustified safety claim.**  
   Ordinary hashes of phone numbers or emails can be guessed; deterministic identifiers remain linkable. Free text, addresses, location and combinations of innocuous fields can disclose identity. A sampled preview cannot show everything future events will contain.

   **Fix/test:** Use field allowlists and removal first; use keyed, workspace-scoped pseudonyms where joins require stable identifiers. Describe the result as pseudonymization, not anonymity. Keep raw samples and discovery secrets out of CLI output, telemetry and model prompts. Test what actually crosses the network. Include the external MCP client’s model provider in the data-flow model: a local S2W process does not imply local agent inference.

3. **The client agent becomes the privileged injection target.**  
   System 2 lacking credentials does not protect a coding agent that reads poisoned evidence and also has shell, email or deployment tools. Quoting stream text reduces ambiguity but does not enforce isolation. A history of accurate inbox answers also does not justify authority to activate rules.

   **Fix/test:** Start with read-only MCP, separate investigation from execution, and make permissions explicit and task-scoped. Reputation may affect recommendation ranking, never automatically expand capabilities. Test malicious text in events, entity names and explanations. For remote MCP, enforce authentication, resource-bound tokens and authorization on every tool and evidence lookup; follow the protocol’s [security guidance](https://modelcontextprotocol.io/docs/2025-11-25/tutorials/security/security_best_practices).

4. **Temporary cloud workspaces create a security and operations product.**  
   No signup must not mean public access or possession of a friendly URL being sufficient to claim company data. Short retention also conflicts with a 48-hour forecast in a workspace that expires after 24 hours.

   **Fix/test:** Before accepting external data, test tenant isolation, separate read/write/claim credentials, expiry, revocation and deletion of derived data as well as raw events. Bound ingestion, storage and model spend per workspace and globally. Either retain an explicitly authorized evaluation window or censor forecasts when observation ends. Defer this entire service until the local experiment passes.

5. **The launch demo can silently grade its own interpretation.**  
   Aircraft positions do not by themselves establish flight identity, scheduled destination or a confirmed landing. If the same airport-assignment heuristic generates both prediction and “truth,” a solid ghost proves consistency, not accuracy. OpenSky’s arrival endpoint is updated by a nightly batch, so it cannot supply the promised immediate landing confirmation. Its API also has access and rate constraints. [OpenSky API documentation](https://openskynetwork.github.io/opensky-api/rest.html)

   **Fix/test:** Choose and verify the feed and outcome source before promising the clip. Define precisely what `p 0.81` predicts and what the time interval means. Label inferred touchdown separately from independently confirmed arrival. Mark accelerated replay explicitly, persist the hour needed for scrubbing, and publish grades for the complete eligible cohort rather than selected successful landings.

6. **Read-only ingestion can still disrupt a source; agents can amplify resource use.**  
   A two-hour backfill adds broker load. Retries can duplicate ingestion or workspace creation. `wake` actions, queries and audit events can create feedback loops.

   **Fix/test:** Enforce topic/group permissions, stable retry identities, bandwidth limits, bounded buffers and a defined overflow policy. Crash-test cursor persistence and reconnects. “Read-only” is not itself an offset guarantee: Kafka authorizes offset commits through group/topic read permissions. [Kafka authorization documentation](https://kafka.apache.org/34/security/authorization-and-acls/) Keep audit events outside the default business projection and subscriptions; cap wake depth and execution budgets.

7. **Open-source launch commitments can harden the wrong product.**  
   A public DSL, plugin API, world format, installer and hosted trial create compatibility and support obligations. `npx`, Cargo and Docker add separate distribution work. An application licence does not establish redistribution rights for demo data or bundled models.

   **Fix/test:** Release one supported installation path, one tested source and explicitly experimental interfaces. Version the persisted format, provide a reproducible local example and document data/model provenance. Track consenting users who complete a named task and return to it; stars, clips and connection counts are acquisition evidence.

There is also a semantic trap in `forecast.ask`: an arbitrary sentence must not manufacture a probability or borrow another target’s track record. Resolve requests to registered, versioned questions with defined horizons, or return “unsupported.”

**C. Will continuous architecture prevent the old burden?**

**It will reduce some kinds of drift, but it will not prevent the main burden visible here: too many commitments before the contracts are understood.** Ten correctly layered crates can implement the wrong abstraction perfectly.

The strongest parts are the functional core, persisted decisions, delayed generalization and regular simplification. Keep those. Change the mechanisms that confuse architectural activity with architectural improvement.

- **Make the core’s purity explicit.** Pass time, randomness, accepted mappings and external results as inputs. A dependency check cannot prevent `std::fs`, wall-clock access or hidden global state. The compiler enforces dependency boundaries; it does not establish semantic purity.
- **Keep a simple reference fold.** Use it to check checkpointed or incremental implementations later. Do not require every production operation to reconstruct the entire world.
- **Separate issuance from outcome records.** `Forecast` is described as immutable but contains a changing `outcome`. Store immutable issuance, then append outcome observations and adjudications with their own versions. Define how late evidence affects grades without rewriting issuance.
- **Design persisted compatibility before public API stability.** Stored events and profiles outlive internal Rust signatures. Replay requires supported format migrations or pinned interpreters, not just an unchanged function signature.
- **Give deletion a defined meaning.** Exact replay can only be promised over retained evidence. Data expiry must produce an explicit replay limitation, not silently preserve sensitive data indefinitely.
- **Use fewer boundaries initially.** One core crate and one application/adapters crate may suffice. Modules can express the rest until an actual second implementation earns a seam.
- **Measure change difficulty.** Track how long representative changes take, how many unrelated modules they touch and which defects recur. Crate count and lines deleted are diagnostic signals, not success measures.

The proposed fitness functions have quite different value:

| Function | Value for S2W | What I would change |
|---|---|---|
| Replay determinism | Highest | Test both histories, crash recovery and checkpoint/full-fold equivalence; include expected answers because a stable hash can consistently encode the wrong result. |
| Forecast immutability | Highest | Also prohibit future-evidence leakage and require complete issuance accounting across pruning, restart, censoring and abstention. |
| Authority boundaries | Missing; highest once writes/cloud exist | Test that provisional state cannot activate rules and that tenants/agents cannot access unauthorized data or capabilities. |
| Resource bounds | High | Measure end-to-end p99 lag, queue growth, retained state, replay time and model spend under a declared workload and slowdown. |
| Decoder/repair/parser fuzzing | High | Prioritize malformed inputs, identifier collisions, repeated events and repair/revocation sequences; defer DSL fuzzing until a DSL exists. |
| Layering | Useful | Keep a small allowed dependency graph; avoid creating crates just to satisfy the checker. |
| Hygiene | Useful baseline | Keep linting and dependency review; `forbid(unsafe_code)` does not certify dependencies or prevent semantic errors. |
| Public API drift + ADR for every change | Low early | Delay until external consumers exist; otherwise it makes necessary discovery expensive. |
| Fixed 10% benchmark regression gate | Weak as written | Use stable measurement and absolute budgets; tiny noisy changes and slow cumulative deterioration both defeat this rule. |
| LOC/crate/dependency reduction | Lowest as a target | Reward reduced change cost and removed obligations, not shrinking counts. |

A particularly valuable additional property: **identical payloads with different legitimate source identities must not automatically be deduplicated.** Content hashing alone can erase real repeated business events.

On cadence, continuous attention is preferable to routinely alternating feature and architecture sprints. But **25% should be a provisional capacity reserve, not a quota**. Some weeks need no separate architecture project; others expose a contract error that deserves immediate attention. Required correctness work belongs in the feature’s completion criteria rather than competing for the architecture allowance.

A debt ceiling based on summed estimates is easy to game and hard to interpret. Give consequential items an observed cost, owner, next affected task and expiry or trigger. Use concrete circuit breakers: broken replay invariants, unbounded growth, repeated boundary-crossing changes or a persistent increase in delivery time. Allow a focused architecture sprint when those triggers justify one.

Fresh-eyes reviews should use actual changes and failures as evidence. Their output may be “delete this feature” or “make no change”; requiring new decision records and priced debt encourages paperwork. The stated explanation of previous failures—architecture treated as a phase—is also a hypothesis. Review those projects for abandoned features, unstable semantics and premature abstractions before prescribing the cure.

**D. Is this the smallest first slice?**

No. Step 2 is already several projects, and step 5 has a dependency contradiction: the ADS-B clip requires the map, forecasts and branching presentation that are deferred until afterward.

I would replace the slice with four gates:

1. **Specify one identification experiment and its limits.**  
   Obtain an owner-labelled private sample and a predefined operational question before building integration machinery. Define evidence available to each method, acceptable false merges, required coverage, correction-time accounting and the minimum useful improvement. Use Wikipedia as a reproducible fixture and obfuscation as a robustness test. Do not demand recovery of information deliberately removed.

2. **Build a local comparison harness.**  
   Read a captured event file, preserve provenance, run heuristics and one budget-capped System 2 pass, and display an evidence table plus editable mappings. Freeze the mapping before evaluating a later held-out window. Give both methods equivalent evidence and human-help budgets. A graph is optional; a general DSL, router and plugin framework are unnecessary.

3. **Test whether the model saves work.**  
   Have the owner answer the same question using their existing method and S2W, with controlled task order or matched tasks. Record total effort, errors and abstentions. Require an accuracy floor; accept either better accuracy at comparable effort or less effort at comparable accuracy. Include an inexpensive raw-sample LLM comparison if practical—it checks whether the persistent world contributes anything beyond a good summary.

4. **Only after that passes, test one forecast ledger.**  
   One target, one issuance per eligible unit at a declared cutoff, independent outcome collection, temporal holdout and matched baselines. Report skill, coverage, censoring and uncertainty. A calibration curve alone is not a pass.

For Wikipedia, specify whether the target is a recognized revert occurring within thirty minutes, and allow delayed label collection. MediaWiki applies `mw-reverted` through an asynchronous job with detection limits, so absent tags at the horizon are not automatically negative labels. [MediaWiki revert documentation](https://www.mediawiki.org/wiki/Manual:Reverts)

Also verify that the chosen Wikimedia model’s target and inputs match the comparison; calibrate any conversion to a thirty-minute probability using training data only. Existing moderation may use that model to cause reverts. That does not invalidate prediction of observed moderation, but it limits claims about independent edit quality. [Wikimedia Automoderator documentation](https://www.mediawiki.org/wiki/Moderator_Tools/Automoderator)

Then add **one** live source and a small usability test. If Kafka users are the recruitment target, replay the same fixtures through local Kafka. If agent usability is essential, add a thin read-only MCP wrapper after the underlying query works.

Cut cloud workspaces, discovery across multiple configuration systems, automatic MCP configuration, generalized redaction, trust promotion, wake actions, branching, ADS-B and launch packaging from the first experiment. Keep `--json`; it is cheap and useful. Set an actual hours cap before starting.

Before implementation, make three changes: adopt the bounded local slice, make explicit acceptance and forecast-based rule semantics consistent throughout the page, and complete the evaluation contract with decision thresholds. These are sufficient prerequisites; production cloud security and the full architecture programme can wait until their features are justified.

VERDICT=BLOCK