# round1 claude critic (independent Claude critic, 2026-09-27)

**1. Pre-mortem (most likely first)**

- **Types only looked good because the LLM already knew the domain.** Wikipedia, ADS-B and Polymarket all have public, documented schemas the model has read. On a private CDC stream, "System 2 names the domain" did much worse.
  - *Warning sign:* type names match the public docs word for word.
  - *Two-week test:* replay a Wikipedia sample with renamed fields and hashed ids, plus one private Postgres WAL. Score the typed world against a hand-labelled answer key.
- **Predictions beat base rate and nothing else.** "Beats base rate" is a low bar. A dedicated tool already does each demo's job better: Wikimedia's revert models (ORES/Lift Wing) for Wikipedia, FlightAware-style ETA models for flights.
  - *Warning sign:* Brier scores sit just under base rate, and the forks mostly repeat the base rate.
  - *Two-week test:* score "reverted in 30 min" against Lift Wing's damaging score on the same edits.
- **It grew past a hobby slot.** The plan has ten crates, a view, forks, rules, WebAssembly plugins, Tailscale and hosting, all under a capped-hobby rule.
  - *Warning sign:* the view (step 3) takes longer than steps 1 and 2 together, and time goes into polishing the mockup.
  - *Two-week test:* time-box steps 1 and 2 to one week. Cut scope if they miss it.
- **A demo, not a job.** Viewers said "cool graph" and then asked what they should do with it. Nobody's pain is "I can't see my org as a whole." Pains are specific, like stalled orders.
  - *Two-week test:* five conversations with ops people. Ask each for the last thing they wish they had seen coming. Check whether any of those could be found from a stream.
- **Acting on a prediction ruined its own grade.** In the demo, the win-back email makes Ben come back, and then the fork counts as "confirmed." Pruned forks are never graded at all. The scorecard looked honest and wasn't.
  - *Test:* write a holdout design before the first rule fires.
- **Silent wrong joins led to wrong actions.** Linking by "amount and time" merges the wrong customers, and a rule acts on the merge.
  - *Test:* on a labelled sample, measure how often auto-applied links are wrong at the 0.95 threshold.

**2. Skeptical buyers**

- **VP of operations, logistics**
  - *No:* "Our TMS/WMS and project44/FourKites already predict ETAs, trained on industry data. Our data sits in SAP/Oracle, EDI 214/856 files and email, not Kafka. Who is liable when a predicted exception pages someone at 3am? Where is your SOC 2?"
  - *Yes:* read-only, on-prem, and in the first hour it surfaces cross-system exceptions their TMS missed, with evidence and no IT project.
  - *Is the org its streams?* Partly. Status is event-like. Rates, contracts, SLAs and carrier terms are tables and documents.
- **City government CIO**
  - *No:* procurement and RFP rules, StateRAMP/CJIS, resident PII going to an LLM, and a hosted box joining the city network (non-starter). "Predictive" carries algorithmic-accountability baggage. It's a one-person vendor.
  - *Yes:* start on already-public 311 and GTFS feeds, with no PII. The replayable audit trail helps with records law.
  - *Is the org its streams?* Mostly false. The important facts are cases in Accela or Salesforce, GIS, budgets, ordinances and staffing. The streams are exhaust.

**3. Competitive reality**

- **Object-centric process mining** (Celonis Process Intelligence Graph and Action Engine, SAP Signavio, OCEL 2.0, PM4Py). This is the closest prior art: event logs become a graph of objects, plus actions.
  - They have: SAP connectors, business content, conformance checking.
  - S2W adds: live, zero-config, and forks.
- **Palantir Foundry/AIP Ontology.** "The org as objects, links and actions" is their pitch, delivered by forward-deployed engineers.
  - S2W's angle: minutes and cheap, versus months and expensive.
- **Microsoft Fabric Real-Time Intelligence (Activator), Flink CEP, Esper.** Rules on streams that trigger actions, with governance.
  - They lack: automatic modelling and graded predictions.
- **Streaming databases** (Confluent with Flink and Schema Registry, Materialize, RisingWave, Feldera). Correct incremental views.
  - They lack: automatic types and prediction.
- **Observability/AIOps** (Datadog, Splunk, Dynatrace Davis). Log-template clustering and anomaly detection are commodities there.
  - They lack: business entities.
- **Supply-chain control towers and digital twins** (Kinaxis, o9, Azure Digital Twins). Deep domain models and trained predictors.
  - They lack: zero setup.
- **Real wedge:** not the graph, which is crowded. It is the graded-fork loop (a public calibration record per predictor on your own entities) plus revocable, replayable repairs. The first buyer is the event-sourcing and Kurrent crowd Dave already knows, who have real streams today.

**4. Overstated claims, with safer wording**

- "Point it at any stream, tidy or not… see your organization as a living world" → "Point it at an event stream and within minutes see the entities it contains, typed, with repairs shown."
- "An organization is its streams" → "Much of what changes in an organization shows up in its streams."
- "Few-shot, at near-zero cost, from any stream, in minutes" → drop it. System 2 is a paid LLM budget.
- "No schema, no mapping, no event catalog" / "Skip the integration project" → "Start without a schema; the mapping builds up from your answers." (The page's own inbox and side sources contradict the original.)
- "Keep a perfect record… replay exactly why something happened" → "Every inference is logged with source and confidence, so you can see what the system knew when it acted." LLM output isn't deterministic, and side sources change.
- "honest probabilities" → "probabilities with a published calibration record."
- "a continuous, free label for every predictor" → "a label for every fork that ran to completion; forks that were acted on need a holdout."
- "Works for… a hospital's admissions… an agency's case system" → "Tested on three public streams."
- "The trial should take less time than a sales call" → "Setup takes minutes; your security review sets the real timeline."

**5. What's missing**

- A definition of "understood": who labels it, what it's measured against, and what counts toward the percentage.
- Baselines beyond base rate: dedicated domain models, simple heuristics.
- Intervention and holdout design for rules that act on predictions.
- Where predictors come from: hand-written, written by System 2, or learned?
- PII and redaction before anything reaches an LLM, and data residency for the hosted version.
- One named user and the one decision S2W changes for them.
- A test on a private or obfuscated stream.
- Kill criteria and an hours cap that match the hobby-slot pivot.
- A prior-art section. Object-centric process mining is the obvious gap.
- Log retention, and what happens to past replays when the world profile changes.

**6. Worth protecting**

- **The log as the single source of truth.** Repairs, proposals and predictions are revocable events, so scrubbing is exact and a bad repair can be undone.
- **Reality grades forks, and grades engines too.** This is the feedback loop PredictStream never had. Fix the intervention bias so the scorecard stays trustworthy.
- **System 2 never sits in the stream, and System 1 is a swappable trait with a bake-off.** Keep the first slice small too: Wikipedia first, one predictor, a webhook.
