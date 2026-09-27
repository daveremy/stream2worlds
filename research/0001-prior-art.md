# 0001: Prior art

- **Question:** who already does parts of Stream2Worlds, what is genuinely open, and what to cite?
- **Date:** 2026-09-27 · **Researcher:** sagan (Fable, with research subagents) · **Status:** draft — Design implications pending disposition (next session)
- **Feeds:** the novelty claim, the paper (stream2worlds#1), launch framing, contract A10 (revert-model target).

# Stream2Worlds — prior-art map

Research date: 2026-09-27. Sagan (research agent), with four research subagents. Opinion only — nothing edited, nothing sent.

Conventions: every row cites the page used. **UNVERIFIED** = confirmed only from a search snippet, not a fetched page. "Threat" = how much the work erodes the stated novelty claim (my judgment). Facts and judgments are labelled where they could be confused.

---

## Verdict

- **Genuinely open (as of 2026-09-27):** the *composition* — identity-key + relationship + entity-type discovery from an arbitrary symbolic event stream, compiled into rules a microsecond path applies with the LLM never on the hot path, with the system's own forecasts graded against the live stream as the training signal. No fetched paper or product does all of it; the 2026 Text-World-Models survey ([arXiv 2606.09032](https://arxiv.org/html/2606.09032v1)) states no cited work builds a world model from real event logs/streams or does automatic schema induction. Gate 3's **obfuscated-stream condition** (fields renamed, ids hashed) is a novel evaluation design — the strongest existing "discover object types from a flat log" method (Rebmann 2022) leans on attribute *names*, so obfuscation is exactly the test that separates semantics-from-names from structure-from-data.
- **Already done:** each of the three claimed components exists on its own. (1) Discovering object types, identity attributes and object relationships from an unlabeled event log is a solved offline problem in process mining (Rebmann/Rehse/van der Aa BPM 2022; Lichtenstein 2021; Toyoda 2023) and key/FK discovery with "statistics first, LLM adjudicates" shipped in three 2026 database papers (Tursio, LLM-FK, DBAutoDoc). (2) "LLM writes rules/heuristics offline, fast code runs online" is CodeAD, FunSearch/EoH, LILAC, Snorkel-with-LMs; "background teacher continuously labels a live stream for a production student" is Google's ranking-KD paper and RouteNLP. (3) Graded probabilistic forecasts on live streams with late resolution is Complex Event Forecasting (Wayeb, VLDBJ 2021), ForecastBench, Prophet Arena, GJ Open.
- **Single biggest novelty threat:** **Zep/Graphiti** ([arXiv 2501.13956](https://arxiv.org/abs/2501.13956), [repo](https://github.com/getzep/graphiti)) — LLM-extracted entities and relationships, LLM entity resolution, bi-temporal edges, JSON episode ingestion, incremental, README offering "let structure emerge from your data (learned)". It is what HN will paste under the launch. The honest differences are real but narrower than the pitch: Graphiti puts the LLM on every episode's path, resolves identity by name similarity rather than a discovered key attribute, has no replayable projection, no forecasting, no obfuscation test. **Academic equivalent:** Rebmann 2022 (below) already claims "automatically uncovers object-related information in flat event data."
- **Gate 4 reality check:** Wikimedia already publishes a per-edit revert probability live on EventStreams (`revertrisk-language-agnostic`, AUC ≈ 0.865) — but its training label is an identity revert with **no time window at all** (verified in the refinery source), so "reverted within 30 min" is a strict subset nobody has forecast or published a base rate for. Our own read-only measurement: ≈ 6% of human mainspace edits are ever reverted, ≈ 53% of those within 30 min → **≈ 3% (2.6–3.7%) 30-minute base rate**; ~22% for anonymous edits. Use the revert-risk stream as the strong baseline and climatology at p≈0.03 as the floor (§Q5).
- **Wording risk:** "possible worlds" has a precise, 20-year-old meaning in probabilistic databases (a distribution over *present* database instances — Dalvi & Suciu). s2w's use (sampled *futures*) collides with it in front of exactly the VLDB/SIGMOD audience a paper would target. Keep the phrase for the pitch, define it in one sentence, and use "rollout"/"sampled future world" for the mechanism (§Q2).
- **Recommended stance for the HN post and the paper:** claim the intersection and the evaluation, attribute every component, and name the three or four nearest systems yourself before a commenter does. "Nobody infers schema from event data with an LLM" is false as of 2024 and very false as of 2026.

---

## Q1. Closest prior art to the core claim

### Q1a. Object-centric process mining (OCPM) and event-log structure discovery

This is the academic field whose *output data model* is what s2w produces, and it has a decade of work on getting there automatically.

| Work | Year | What it does | Schema given or discovered? | LLM? | What s2w adds | Threat |
|---|---|---|---|---|---|---|
| [OCEL 2.0 specification](https://arxiv.org/abs/2403.01975) (Berti, Koren, Adams, Park, …, van der Aalst; [ocel-standard.org](https://www.ocel-standard.org/2.0/ocel20_specification.pdf)) | 2023/24 | Standard for object-centric event logs: events link to many typed objects; object attribute *changes*; qualified event-to-object (E2O) and object-to-object (O2O) relationships; SQLite/XML/JSON exchange | Given (a log format) | No | s2w's world model is essentially an OCEL 2.0 instance produced live — **export OCEL 2.0** and you get every OCPM tool as a downstream consumer and every OCPM benchmark as a comparison | Medium (data model precedent; cite and adopt, do not reinvent the vocabulary "object type", "E2O", "O2O") |
| [Rebmann, Rehse, van der Aa, "Uncovering Object-Centric Data in Classical Event Logs for the Automated Transformation from XES to OCEL"](https://doi.org/10.1007/978-3-031-16103-2_25), BPM 2022 (19 citations per [Semantic Scholar](https://api.semanticscholar.org/graph/v1/paper/DOI:10.1007/978-3-031-16103-2_25?fields=title,authors,year,venue,citationCount)) | 2022 | "Automatically uncovers object-related information in flat event data" and emits OCEL, by "combining semantic analysis of textual attributes with data profiling and control-flow-based relation extraction". Evaluated on the public order-handling OCEL log: 22,367 events, 11,522 object instances of 5 types (orders, items, products, customers, packages). *Details from the abstract as summarised in search results; full PDF was paywalled/404 — method details UNVERIFIED beyond the abstract.* | **Discovers** object types, instances and relations from a flat log with no engineer | Pre-LLM language model for attribute semantics | Live streams; LLM proposals; identity keys as first-class across arbitrary domains (not just process logs); state and forecasting; **obfuscation robustness** — this method's semantic-analysis leg reads attribute names, which Gate 3's f1..fN condition removes | **High** for claim (1) as literally stated; it is the paper a BPM reviewer will cite first. Use it as a Gate 3 baseline. |
| [Lichtenstein, Bano, Weske, "Attribute-Driven Case Notion Discovery for Unlabeled Event Logs"](https://doi.org/10.1007/978-3-030-94343-1_9), BPM Workshops 2021 ([code](https://github.com/t-lichtenstein/attribute-driven-case-notion-discovery)) | 2021 | Takes an unlabeled event log (no case id) and discovers a data model — entity classes and their identifying attributes — via Event Log Subdivision → Entity Class Instance Estimation → Trace Generation. *Abstract elided by publisher; description from search summary — UNVERIFIED.* | Discovers entity classes + identifiers | No | Streams, relationships, LLM, forecasts | Medium-High |
| [Toyoda et al., "Identifying Key Attributes in an Unlabeled Event Log"](https://arxiv.org/abs/2301.12829), IEEE TSC 2023 | 2023 | Supervised ML narrows candidates for case-id, activity, timestamp; picks the combination that yields the best process model; ~20 s on 14 datasets | **Discovers the identity (case) key** | No | N entity types, relationships, streaming, LLM | Medium-High on "identity key" |
| [Nooijen, van Dongen, Fahland, "Automatic Discovery of Data-Centric and Artifact-Centric Processes"](https://research.tue.nl/en/publications/automatic-discovery-of-data-centric-and-artifact-centric-processe/), DAB@BPM 2012 | 2012 | From a relational DB (validated on a retailer's production ERP): discovers central data objects, their case identifiers and interrelations, and a lifecycle model per object | Discovers from DB schema + data | No | Event streams (no schema), LLM, forecasts | Medium |
| [Berti, Park, Rafiei, van der Aalst, generic OCEL extraction from SAP ERP](https://dl.acm.org/doi/10.1007/s10844-023-00799-9), JIIS 2023 | 2023 | Builds a Graph of Relationships over SAP tables, analyst designs a "blueprint" with domain knowledge, extracts OCEL | Semi-automatic; domain knowledge required | No | Removes the analyst | Low-Medium |
| [Rebmann & van der Aa, "Extracting Semantic Process Information from the Natural Language in Event Logs"](https://arxiv.org/abs/2103.11761), CAiSE 2021 | 2021 | Semantic role labeling of event attributes: actions, business objects, actors, up to 8 roles per event, via a language model + attribute classification | Discovers semantics from attribute text | Pre-LLM LM | Same obfuscation caveat: names-dependent | Medium |
| [Buss, Kecht, Kratsch, Röglinger, Sadeghianasl, Wynn, "Process mining between the lines"](https://www.sciencedirect.com/science/article/pii/S030643792600027X), Information Systems 140 (2026) ([ERef](https://eref.uni-bayreuth.de/id/eprint/96604/)) | 2026 | gpt-5-mini extracts OCEL components incl. E2O and O2O relationships from unstructured text; F1 0.39 → 0.58 with heavier prompting | LLM discovers object-centric structure from text | Yes, offline | Symbolic event input; streaming; fast path | Medium (proves LLMs recover OCEL structure; also shows how hard it is — F1 ≤ 0.58) |
| [Seidel, Winkler, Gianola, Montali, Weske, "To bind or not to bind?"](https://arxiv.org/abs/2508.18231) (2025); [Gianola et al., "Detecting Dynamic Relationships in OCELs"](https://arxiv.org/abs/2604.13053) (2026) | 2025–26 | Formalise stable many-to-one and time-varying object relationships in OCELs | Types given | No | Discovery | Low (but the formal vocabulary for "relationship that changes over time" is here — reuse it) |
| [OCPM² methodology](https://arxiv.org/abs/2503.10735) (Miri et al. 2025); [Berti, Montali, van der Aalst, OCPM systematic review](https://arxiv.org/abs/2311.08795) (2023) | 2023–25 | Analyst-driven methodology for OCED extraction; SLR of the field | Manual | No | — | Low (the SLR is the right citation for "state of OCPM") |
| Celonis [Object-Centric Data Model](https://www.celonis.com/blog/celonis-object-centric-data-model-single-source-of-truth-for-process-intelligence) / [Process Sphere](https://www.celonis.com/blog/celonis-announces-next-generation-mri-process-mining-technology-with-process-sphere); [docs](https://docs.celonis.com/en/data-extractions-and-transformations-for-object-centric-process-mining.html) | 2022– | Commercial OCPM; "you can use transformation tasks to convert that data into objects, events, changes, and relationships" — engineered SQL transformations | **Engineered** | No (per docs) | Discovery | Low-Medium (market vocabulary overlaps; a Celonis person will ask "isn't this our object model, inferred?") |
| Microsoft Power Automate [object-centric process mining](https://learn.microsoft.com/en-us/power-platform/release-plan/2025wave2/power-automate/analyze-processes-using-object-centric-process-mining) | 2025 | OCPM in Power Platform | Engineered (UNVERIFIED — release note only) | — | — | Low |

**Streaming process mining** — van Zelst, van Dongen, van der Aalst, "Event stream-based process discovery using abstract representations" (KAIS 2018, UNVERIFIED); Burattin, "Streaming Process Mining" chapter (2022, UNVERIFIED); [AVOCADO: The Streaming Process Mining Challenge](https://arxiv.org/abs/2510.17089) (Imenkamp, …, Burattin, 2025/26) — a standardised evaluation framework for streaming algorithms (accuracy, MAE, RMSE, latency, robustness) that "does not address object-centric data or schema discovery". Fit: low threat, but AVOCADO is the closest existing *evaluation harness* for "online structure recovery from an event stream" and its authors are the natural reviewers for a Gate 3 protocol.

**Object-centric predictive process monitoring** — [Galanti et al., "Object-Centric Predictive Process Monitoring"](https://link.springer.com/chapter/10.1007/978-3-031-26507-5_3) (2023); [HOEG](https://arxiv.org/abs/2404.05316) (CAiSE 2024, heterogeneous GNN over event+object graph); [Object-centric graph embeddings](https://arxiv.org/pdf/2507.15411) (2025); [comparative analysis OCEL vs classical PPM](https://link.springer.com/article/10.1007/s10115-025-02461-y) (KAIS 2025); [EHHN](https://arxiv.org/pdf/2607.01785) (2026). All forecast on OCELs (remaining time, next activity) with the schema given, offline. Medium threat to claim (3): forecasts over object-centric state already exist; s2w's difference is *live, immutable, graded* forecasts over a *discovered* schema.

### Q1b. Complex event processing and complex event forecasting

| Work | Year | What it does | Schema | What s2w adds | Threat |
|---|---|---|---|---|---|
| [Cugola & Margara, "Processing flows of information: from data stream to complex event processing"](https://dl.acm.org/doi/10.1145/2187671.2187677), ACM CSUR 44(3) 2012 ([PDF](https://cugola.faculty.polimi.it/Papers/cep_survey.pdf)) | 2012 | Canonical survey; rules registered, engine matches patterns continuously | **Given** (rules and event types written by engineers) | Discovery of types and rules | Low (the citation for "what CEP is") |
| [Margara, Cugola, Tamburrelli, iCEP — "Learning from the past: automated rule generation for CEP"](https://margara.faculty.polimi.it/papers/iCEP_debs14.pdf), DEBS 2014 | 2014 | Learns CEP rules from historical traces labelled with when the situation occurred; evaluated on synthetic + traffic traces | Event types given; **rules discovered** | Entities/state, unlabeled streams, LLM | Medium (precedent for "rules learned from history, executed in stream") |
| [Mousheimish, Taher, Zeitouni, autoCEP](https://doi.org/10.1007/978-3-319-46295-0_38), ICSOC 2016 (abstract elided — UNVERIFIED) | 2016 | Automatic learning of *predictive* CEP rules | Given types | — | Low-Medium |
| [Alevizos, Artikis, Paliouras, "Complex Event Forecasting with Prediction Suffix Trees"](https://link.springer.com/article/10.1007/s00778-021-00698-x), VLDB J 2021 ([tech report](https://arxiv.org/pdf/2109.00287)); Wayeb tool; [Run-time adaptation of CEF](https://dl.acm.org/doi/10.1145/3701717.3730539), DEBS 2025; [Symbolic register automata for CER/CEF](https://arxiv.org/pdf/2110.04032) | 2021–25 | Forecasts *when* a pattern will complete over a stream, with probabilistic guarantees; online adaptation | Patterns given | Discovered state; forecasts about entities not patterns; immutable graded ledger | **Medium-High for claim (3)** — "forecast on a live stream, evaluated as outcomes arrive" is this field's definition |
| [Engel & Etzion, "Proactive event processing in action"](https://dl.acm.org/doi/10.1145/2488222.2488274), DEBS 2013; predictive CEP framework (Fülöp et al., UNVERIFIED) | 2013–14 | Forecast composite events to act before they happen | Given | — | Low-Medium |
| [Benzin & Rinderle-Ma, "A Survey on Event Prediction Methods from a Systems Perspective"](https://arxiv.org/abs/2302.04018) (2023, rev. 2025) | 2023 | Unifies event prediction across CEP, PPM, disaster forecasting; systems requirements | — | — | Low (good citation for the fragmented landscape) |
| [DEBS Grand Challenge](https://debs.org/grand-challenges/) ([2025: L-PBF defect detection](https://dl.acm.org/doi/10.1145/3701717.3735578); [2026 Call for Challenges](https://2026.debs.org/call-for-grand-challenges/)) | yearly | Streaming benchmark scored on correctness, throughput, latency | Given | — | None — an **opportunity**: propose the Wikipedia-revert stream as a future GC (§Q7) |

### Q1c. Event sourcing, stream-table duality, streaming databases

All engineered-schema; low threat; cite as the foundation s2w stands on rather than as a contribution. [Event Sourcing pattern](https://learn.microsoft.com/en-us/azure/architecture/patterns/event-sourcing) (append-only log, state = projection); [Marten "Live Aggregation" projections](https://martendb.io/events/projections/); [Kafka Streams KTable / stream-table duality](https://www.confluent.io/blog/kafka-streams-tables-part-3-event-processing-fundamentals/) and [ksqlDB materialized views](https://docs.confluent.io/platform/current/ksqldb/concepts/materialized-views.html) (require a typed schema); [Rama depots → PStates](https://redplanetlabs.com/docs/~/pstates.html) ("replayable projection of an immutable log into state of any shape" — s2w's phrasing almost verbatim; acknowledge it); [Restate](https://restate.dev/), [Pathway](https://pathway.com/framework) (Rust + Kafka + LLM RAG), Arroyo, Bytewax, Fluvio SDF — Rust stream processors with user-defined state. **Judgment:** the "replayable projection" half of the pitch is standard; the novelty is only in *who writes the projection*.

### Q1d. LLM schema / entity / relationship inference from logs, JSON, tables, streams

(Subagent D's full map is condensed here; Graphiti and the DB-profiling wave are the two that matter.)

| Work | Year | What it does | Discovered? | LLM in-loop or background? | What s2w adds | Threat |
|---|---|---|---|---|---|---|
| [Zep / Graphiti](https://arxiv.org/abs/2501.13956) ([HTML](https://arxiv.org/html/2501.13956), [repo](https://github.com/getzep/graphiti), [custom types](https://help.getzep.com/graphiti/core-concepts/custom-entity-and-edge-types), [ingestion](https://help.getzep.com/graphiti/core-concepts/adding-episodes)) | 2025– | Per episode (message, text, **or JSON**): LLM entity extraction with reflexion; LLM fact/edge extraction; resolution = name embedding + full-text search + LLM entity-resolution prompt; **bi-temporal** edges (`t_valid/t_invalid`, `t_created/t_expired`), contradiction invalidation; communities. README: define types via Pydantic "or let structure emerge from your data (learned)" | Entities + relationships + temporal validity, discovered or given | **LLM on every episode's path** (async, "seconds to minutes") | LLM off the hot path; field-level identity keys (Graphiti matches by name similarity); replayable projection; forecasting and grading; obfuscation eval; Kafka-scale throughput (Graphiti publishes none) | **High — the top threat** |
| [Tursio, "Scalable Join Inference for Large Context Graphs"](https://arxiv.org/abs/2603.04176) (Mar 2026); [LLM-FK](https://arxiv.org/abs/2603.07278) (ACL Findings 2026); [DBAutoDoc](https://arxiv.org/abs/2603.23050) (Mar 2026); [Nexus](https://arxiv.org/abs/2602.08186) (Microsoft, Feb 2026) | 2026 | Primary-key candidates + inclusion dependencies found **statistically, LLM adjudicates**; DBAutoDoc: deterministic pipeline gives "a 23-point F1 improvement over LLM-only FK detection" | Identity keys + relationships discovered on tables | Offline; statistics-first | Streams, temporal state, obfuscation, forecasts | **High on keys/relationships** — and DBAutoDoc's finding pre-empts half of Gate 3 (heuristics+LLM > raw LLM) |
| [HoPF](https://link.springer.com/article/10.1007/s10844-019-00562-z) (Jiang & Naumann, JIIS 2020); [Abedjan, Golab, Naumann, profiling survey](https://dspace.mit.edu/bitstream/handle/1721.1/106176/778_2015_Article_389.pdf) (VLDB J 2015); HyUCC/Metanome | 2015–20 | Unique-column-combination = key discovery; PK/FK scoring (~88%/91%) | Keys discovered | No | Streaming, N types, LLM naming | Medium-High (the "why do you need an LLM at all?" objection) |
| [Alhammad, Bogatu, Paton, "Towards Schema Inference for Data Lakes"](https://arxiv.org/abs/2206.03881) | 2022 | Clusters datasets into candidate entity types, then infers relationships between types | Types + relationships, no LLM | No | Streams, keys, state | Medium-High |
| [Jia & Anaissi, "Semantic Layer Induction from Raw Telemetry via Hierarchical LLM and RAG Abstraction"](https://arxiv.org/abs/2609.19615) | **17 Sep 2026** | Hierarchical LLM over unlabeled key-value tracking events → business features/actions → SQL-like mapping rules; Airflow batch; expert score 82.3 vs 51.6 | Business taxonomy from event telemetry; **no entities, keys or relationships** | Offline batch | Entities, keys, relationships, state, fast path, forecasts | Medium-High (same input class, ten days old) |
| [LILAC](https://arxiv.org/abs/2310.01796) (FSE 2024; [code](https://github.com/logpai/LILAC)); [LLMParser](https://arxiv.org/abs/2404.18001) (ICSE 2024); [Loghub-2.0](https://arxiv.org/abs/2308.10828); [LLM log-parsing review](https://arxiv.org/pdf/2504.04877) (2025); Drain (2017, UNVERIFIED) | 2017–25 | Log-line → template; LILAC = LLM on cache miss, adaptive cache on hit | Templates only | LILAC: LLM synchronous on miss | Entities/state instead of templates; LLM never blocks | Medium as design precedent, low on the claim |
| [Baazizi et al., JSON schema inference](https://openproceedings.org/2017/conf/edbt/paper-62.pdf) (EDBT 2017; [VLDBJ 2019](https://link.springer.com/article/10.1007/s00778-018-0532-7)); [Jxplain](https://odin.cse.buffalo.edu/papers/2021/SIGMOD-JsonSchemas.pdf) (SIGMOD 2021); [Mior, LLMs for JSON Schema Discovery](https://arxiv.org/abs/2407.03286) (2024) | 2017–24 | Structural types from schemaless JSON; Mior adds LLM names/descriptions post hoc | Structural types | Post-hoc | Keys, relationships, state | Low-Medium |
| [Confluent Schema Registry — infer schema for existing topic](https://docs.confluent.io/cloud/current/sr/schemas-manage.html); [Databricks Auto Loader](https://docs.databricks.com/aws/en/ingestion/cloud-object-storage/auto-loader/schema); [Snowflake INFER_SCHEMA](https://docs.snowflake.com/en/sql-reference/functions/infer_schema); [Spark](https://spark.apache.org/docs/latest/sql-data-sources-json.html) | — | Column-type inference; Confluent samples ≤10 messages and "analyzes the message value only. The structure and content of the message key are ignored" | Structure only | No | Everything past structure | Low — and a good HN foil |
| [AutoSchemaKG](https://arxiv.org/abs/2505.23628) (2025); [SCOPE & SCION](https://arxiv.org/abs/2607.21610) (2026); [LLMs4OL](https://arxiv.org/pdf/2307.16648) (ISWC 2023) | 2023–26 | LLM induces entity/event/relation types from *text* with no predefined schema; AutoSchemaKG: 900M+ nodes, "92% semantic alignment with human-crafted schemas"; SCOPE = a benchmark for corpus-to-schema induction | Discovered from text | Offline | Symbolic events, keys, streaming | Medium (schema induction is an established LLM task; SCOPE is a Gate 3 analogue in text) |
| [CHORUS](https://www.vldb.org/pvldb/vol17/p2104-kayali.pdf) (PVLDB 2024); [Korini & Bizer CTA](https://arxiv.org/abs/2306.00745); Sherlock/Sato | 2019–24 | Column types, table classes, join-column prediction with foundation models | Types + joins on tables | Offline | Streams, identity as first-class | Medium |
| [Rossiello & Subramanian, "Discovery Agents for Real-Time Analytics"](https://arxiv.org/abs/2605.27571), CAIS 2026 | May 2026 | Kafka + Flink + LLM agents: generate hypotheses, compile to executable analytics, validate, deploy, over live streams; typed artifact contracts | Not stated (analytics, not schema) | Background agents | Schema/identity discovery; forecasts graded | Medium-Low (adjacent pitch: "LLM agents propose analytics over Kafka in the background") |
| Confluent [Streaming Agents](https://docs.confluent.io/cloud/current/ai/streaming-agents/overview.html) ([launch](https://www.confluent.io/blog/introducing-streaming-agents/), Aug 2025; [Q3'26 update](https://www.confluent.io/blog/2026-q3-confluent-intelligence-ai-update/)); [Confluent NL→Flink SQL via MCP](https://www.confluent.io/blog/querying-kafka-natural-language/) (May 2025) | 2025–26 | LLM + MCP tools inside Flink over **defined tables**; MCP post "fetches structural information from the Schema Registry" first | Engineered | **LLM in the stream** | The opposite split | Low-Medium — proves "LLM in a Kafka stream" is mainstream; s2w's argument is *why not* to do it that way |
| [Microsoft GraphRAG](https://microsoft.github.io/graphrag/index/methods/) (`--discover-entity-types`, incremental append); [iText2KG](https://arxiv.org/abs/2409.03284); [Neo4j LLM KG Builder](https://neo4j.com/labs/genai-ecosystem/llm-graph-builder/); Mem0 | 2024– | LLM KG construction from text, types given or discovered | Text | Batch/per message | Symbolic events, keys, state | Low-Medium |
| [Palantir Foundry Ontology](https://www.palantir.com/docs/foundry/ontology/overview) | — | Object types mapped by engineers; AIP Logic runs LLMs *over* the ontology | Engineered | Consumer | Discovery | Low (vocabulary overlap: "ontology", "object type") |
| Datadog [Log Patterns](https://docs.datadoghq.com/logs/explorer/analytics/patterns/)/[Watchdog](https://docs.datadoghq.com/logs/explorer/watchdog_insights/); Elastic categorization; Splunk field extraction | — | Template clustering, anomaly baselining, AI assistants | Templates | Assistant | — | Low |

**Streaming entity resolution / key discovery** ([Christophides et al. CSUR 2020](https://dl.acm.org/doi/10.1145/3418896); [Gruenheid, Dong, Srivastava, incremental record linkage, PVLDB 2014](http://www.vldb.org/pvldb/vol7/p697-gruenheid.pdf); [Ditto](https://arxiv.org/abs/2004.00584); [Peeters, Steiner, Bizer, EM with LLMs, EDBT 2025](https://www.uni-mannheim.de/media/Einrichtungen/dws/DWS_News/Documents/Peeters-Entity-Matching-using-LLMs-EDBT2025.pdf); [Senzing](https://senzing.com/docs/entity_specification/); [AgenticER vision, VLDB 2026](https://arxiv.org/abs/2607.27435)). **Judgment:** all ER work assumes you know which attributes identify a thing; "schema-agnostic" ER means alignment-free similarity, not key discovery. Gruenheid 2014's "leverage new evidence to fix previous linkage errors" is the direct ancestor of s2w's "repairs" and should be cited as such.

**Knowledge graphs from streams** ([Stream Reasoning survey](https://journals.sagepub.com/doi/10.3233/DS-170006), Data Science 2017 — engineered RDF ontologies; [EventKG](https://arxiv.org/pdf/1804.04526); [TKGC survey](https://arxiv.org/pdf/2201.08236)): mirror image of s2w — sophisticated inference over a stream whose schema is fully given. Low threat; avoid the phrase "stream reasoning", it already means this.

**Name/pitch space:** no hits for "Stream2Worlds", "stream to worlds", or "world model from a Kafka topic" as of 2026-09-27 (search-only absence claim). Nearest lexical neighbour is Alibaba Wan's [Video = World + Event Stream](https://arxiv.org/abs/2607.15038) (Jul 2026, a video model; unrelated).

### Q1 synthesis (judgment)

Three communities each own a piece and none owns the whole. Process mining owns "discover object types, identifiers and relationships from an unlabeled event log" — offline, batch, and (for the best method) dependent on attribute names. Database profiling owns "discover keys and foreign keys, statistics first, LLM adjudicates" — on static tables. Graphiti owns "entities + relationships + temporal validity from a live stream of episodes" — with the LLM on every episode. CEF owns "probabilistic forecasts on a live stream, scored as outcomes arrive" — with the patterns given. s2w's defensible novelty is the composition plus two things nobody in these lists does: **the LLM never touches an event** (it reads snapshots and emits compiled structure), and **the obfuscated-stream evaluation** that separates structure-from-data from semantics-from-names. Say that, cite Rebmann 2022 and Graphiti in the second paragraph of the HN post, and the "isn't this X" thread mostly answers itself.

Corrections to citations in the brief (from subagent D, each checked against the venue page): HoPF is JIIS 2020, not VLDB J; the Stream Reasoning survey is 2017, not 2022; Peeters & Bizer "Entity Matching using LLMs" is EDBT 2025; the JSON schema-inference lineage is Baazizi (EDBT 2017/VLDBJ 2019) then Spoth's Jxplain (SIGMOD 2021); Cai et al. TKGC survey appeared at IJCAI 2023.

---

## Q2. "Possible worlds" as a term

**Facts.**
- Probabilistic databases: "A probabilistic database is a probability distribution over deterministic databases" and "the answer to a query Q on a probabilistic database is a probability distribution over the answers to Q over all possible worlds" — Dalvi, Ré, Suciu, ["Probabilistic Databases: Diamonds in the Dirt"](https://homes.cs.washington.edu/~suciu/file15_cacm-paper.pdf), CACM 2009; foundational query evaluation in [Dalvi & Suciu, VLDB 2004](https://www.vldb.org/conf/2004/RS22P1.PDF). Each world is fixed by a valuation of the uncertain variables ([Wikipedia summary](https://en.wikipedia.org/wiki/Probabilistic_database)). The systems: **MayBMS** — literally ["a possible worlds base management system"](https://www.researchgate.net/publication/253967163_MayBMS_A_possible_worlds_base_management_system) (Koch, Olteanu et al.); **Trio** / ULDBs — "possible instances" ([Benjelloun et al., VLDB 2006](https://www.vldb.org/conf/2006/p953-benjelloun.pdf); [Agrawal et al., Trio demo](https://www.vldb.org/conf/2006/p1151-agrawal.pdf)); **MCDB** — "samples possible worlds from the database and executes the query predicate on each sampled world" ([Jampani et al., SIGMOD 2008](https://www.cs.uml.edu/~ge/pdf/papers_685-2-1/sigmod08.pdf); [TODS 2011](https://dl.acm.org/doi/10.1145/2000824.2000828)). In every case the worlds model uncertainty about the **present** contents of the database, not about the future.
- Modal logic / philosophy: worlds are points of evaluation; necessity = truth in all accessible worlds, possibility = truth in some (Kripke 1959/1963) — [SEP, Modal Logic](https://plato.stanford.edu/entries/logic-modal/); [SEP, Possible Worlds](https://plato.stanford.edu/entries/possible-worlds/) (Lewis's concretism vs abstractionism vs combinatorialism). The metaphysics is irrelevant to s2w but the *semantics* — a set of worlds plus an accessibility relation — is the formal object s2w's forecasts implicitly range over.
- State estimation: particles in a particle filter are "instantiations of the state," i.e. hypotheses about the *current* hidden state ([overview](https://www.emergentmind.com/topics/particle-filtering-for-state-estimation); [forecasting with particle filters](https://www.monash.edu/business/ebs/research/publications/ebs/wp22-2019.pdf)). The literature says "particles"/"hypotheses," not "possible worlds."
- Meteorology: the accepted terms for sampled futures are **ensemble members** and **scenarios** ([ECMWF](https://www.ecmwf.int/en/newsletter/153/meteorology/25-years-ensemble-forecasting-ecmwf); [DWD](https://www.dwd.de/EN/research/weatherforecasting/num_modelling/04_ensemble_methods/ensemble_prediction/ensemble_prediction_node.html); [Mylne 2026, *Weather*](https://rmets.onlinelibrary.wiley.com/doi/10.1002/wea.70015)). RL/planning says **rollouts** or **trajectories**.

**Judgment — collision and benefit.**
- Collision: to a VLDB/SIGMOD reader "possible worlds" means "your *current* state is uncertain" (tuple-level existence uncertainty). s2w means "the *future* is uncertain." Used without definition it invites "so you built a probabilistic database?" and the answer is no.
- Benefit: MCDB's semantics — draw worlds, evaluate a predicate on each, aggregate — is exactly the right formal model for "a forecast is a predicate over sampled futures." Borrow it explicitly and the term becomes a strength: *"a forecast is a query over sampled future worlds (MCDB-style Monte Carlo semantics, extended along the time axis)."*
- Second collision that is actually an asset: s2w *does* have present-state uncertainty — an entity whose identity resolution is ambiguous, a relationship System 2 has proposed but not confirmed. That is the PDB meaning verbatim. If s2w ever represents it, the term does double duty correctly; if not, say which one you mean.

**Suggested wording.** Keep "possible worlds" in the pitch — it is evocative and HN-friendly — and pin it in one sentence the first time: *"s2w draws possible worlds: sampled future states of the entity graph, MCDB-style, rolled forward from the current snapshot. A forecast is a predicate evaluated over those samples, recorded before the outcome and graded after."* In technical prose use **rollout** for the mechanism, **sampled future world** for one sample, and **world ensemble** (never "ensemble" alone — it means model averaging in ML) for the set. Avoid "possible-worlds semantics" as a phrase unless you mean the PDB thing, and avoid "scenario" (reads as hand-authored).

---

## Q3. World models and continuous state

(Subagent C; each row checked against the fetched abstract unless marked.)

| Work | Year | State representation / stream | What s2w adds | Threat |
|---|---|---|---|---|
| Kalman/particle filters, MCL, SLAM — Thrun, Burgard, Fox, *Probabilistic Robotics* (2005, UNVERIFIED) | 2005 | **Engineered** continuous state; sensor stream | Discovers what the state variables are | Low |
| [Friston, free-energy principle](https://www.nature.com/articles/nrn2787) (2010, UNVERIFIED); [Clark, "Whatever next?"](https://www.cambridge.org/core/journals/behavioral-and-brain-sciences/article/whatever-next-predictive-brains-situated-agents-and-the-future-of-cognitive-science/33542C736E17E3D1D44E8D03BE5F4CD9) (2013, UNVERIFIED) | 2010–13 | Theoretical generative model; prediction-error minimisation | s2w's forecast→grade→repair loop is a literal prediction-error loop — cite as ancestor, do not claim | Low |
| [Ha & Schmidhuber](https://arxiv.org/abs/1803.10122) (2018); [DreamerV3](https://www.nature.com/articles/s41586-025-08744-2) (Nature 2025); [MuZero](https://www.nature.com/articles/s41586-020-03051-4) (2020); [Genie 3](https://deepmind.google/blog/genie-3-a-new-frontier-for-world-models/); [Cosmos](https://arxiv.org/abs/2501.03575); [V-JEPA 2](https://ai.meta.com/blog/v-jepa-2-world-model-benchmarks/); [LeCun 2022](https://openreview.net/pdf?id=BZ5a1r-kVsf) (all UNVERIFIED beyond snippets) | 2018–25 | Learned latent; pixels/video; serve an actor | Symbolic, inspectable, entity-typed state; no actor, no reward | Low — only the phrase "world model" overlaps |
| [Wong et al., "From Word Models to World Models"](https://arxiv.org/abs/2306.12672) (2023); [Model Synthesis Architecture](https://arxiv.org/abs/2507.12547) (2025, UNVERIFIED) | 2023–25 | LLM → probabilistic program; domain schemas authored; NL input | Streams, persistence, identity keys, fast execution, grading | Medium |
| [WorldCoder](https://arxiv.org/html/2402.12275) (Tang, Key, Ellis 2024); [Code World Models / GIF-MCTS](https://arxiv.org/abs/2405.15383) (2024) | 2024 | LLM writes Python `T(s,a)→s'`; states as sets of typed objects; **types given by the environment** | No action/reward; types and keys invented; fast path is not the LLM's code alone; graded forecasts | **High** for "LLM writes the symbolic model, code runs it"; low for schema discovery. The citation everyone will demand. |
| [OneLife](https://arxiv.org/abs/2510.12088) (2025) | 2025 | Precondition-effect programmatic laws from exploration; object-oriented state pre-structured | Entity types given | Medium |
| [PatchWorld](https://arxiv.org/abs/2605.30880) (2026) | 2026 | Offline trajectories → "symbolic belief-state programs" repaired by counterexamples; "avoiding LLM calls within the world-model module" | Live streams, identity, grading | **High** — closest to "LLM induces a symbolic belief state from logs and steps out of the loop" |
| [OCM, Object-Centric Environment Modeling](https://arxiv.org/html/2607.02846v1) (2026) | 2026 | After each episode the LLM writes/updates Python **classes for entities and mechanisms** with no skeleton; text environments; LLM reads the model in context | Symbolic *events* not text; System 1 executes; identity/relationships over unbounded streams | **High** for "LLM discovers entity classes from experience" |
| [WorldEvolver](https://arxiv.org/abs/2606.30639), [WorldLLM](https://arxiv.org/abs/2506.06725) (2025–26) | 2025–26 | NL hypotheses/rules revised in prompt context | Persistence as data; fast path | Low-Medium |
| [Text World Models survey](https://arxiv.org/html/2606.09032v1) (2026) | 2026 | Taxonomy; states no cited work uses real event logs/streams or does automatic schema induction | — | Evidence for the gap |
| [Slot Attention](https://arxiv.org/abs/2006.15055) (Locatello 2020, UNVERIFIED) | 2020 | Discovers *objects* as latent slots from pixels | Analogy only | Low |
| [Semantic Bayesian World Models](https://arxiv.org/abs/2609.03834) (position, 2026, UNVERIFIED) | 2026 | KG as evolving belief fabric | An implementation | Low |

**One paragraph.** What s2w shares with the world-model line is the *loop*: maintain a state estimate, predict, compare with what arrives, correct — Friston/Clark's prediction-error minimisation, the Kalman/particle filter's predict-update cycle, Dreamer's latent rollouts. What differs is twofold. First, **engineered or learned-latent vs discovered-symbolic state**: robotics engineers the state vector; Dreamer/MuZero/Genie/Cosmos/V-JEPA learn an opaque latent; the 2024–26 programmatic line (WorldCoder, Code World Models, OneLife, PatchWorld, OCM) writes symbolic state as code but is handed the object types by the environment — OCM and PatchWorld are the closest to inventing them, and both work on text-game episodes. Second, **perceptual vs symbolic streams**: every ML world model above consumes pixels, proprioception or game text; none consumes a firehose of structured business events with an identity problem. The gap the 2026 survey names — no world model built from real event streams, no automatic schema induction — is real. It is also narrower than "world model" suggests: s2w is a *symbolic state estimator with a schema-inducing teacher*, and calling it a world model borrows prestige from a line of work it does not compete with. Use the term, define it, cite WorldCoder and PatchWorld.

---

## Q4. System 1 / System 2 with an LLM teaching a fast model

| Work | Year | What it does | Teacher placement | What s2w adds | Threat |
|---|---|---|---|---|---|
| [Hinton et al., distillation](https://arxiv.org/abs/1503.02531) (2015); [Anil et al., codistillation](https://arxiv.org/abs/1804.03235) (2018) (both UNVERIFIED) | 2015–18 | Soft-target compression; peer distillation during training | Offline / training-time | Structure (rules, schemas), not logits; deployment-time | Low |
| [Snorkel](https://www.vldb.org/pvldb/vol11/p269-ratner.pdf) (2017); [LMs in the loop](https://arxiv.org/abs/2205.02318) (2022); [GPT-3 as labeler](https://arxiv.org/abs/2108.13487) (2021); [Gilardi et al., PNAS](https://www.pnas.org/doi/10.1073/pnas.2305016120) (2023) (UNVERIFIED) | 2017–23 | LLM writes labeling functions / labels; small model trained | Offline batch | Rules/features not just labels; online drift loop | Medium — the pattern is old |
| [Distilling Step-by-Step](https://arxiv.org/abs/2305.02301) (2023, UNVERIFIED); [Yu et al., "Distilling System 2 into System 1"](https://arxiv.org/abs/2407.06023) (2024) | 2023–24 | Rationales as supervision; CoT compiled back into direct generation | Offline | Different System 1 substrate (rules/small models, µs) | Low-Medium (same slogan) |
| Bengio "System 2 deep learning" [NeurIPS 2019](https://neurips.cc/virtual/2019/invited-talk/15488); [Dualformer](https://arxiv.org/abs/2410.09918); [System-1.x](https://arxiv.org/abs/2407.14414) (UNVERIFIED) | 2019–24 | Fast/slow inside one model | In-loop | Two substrates, background channel | Low |
| [FrugalGPT](https://arxiv.org/abs/2305.05176); [RouteLLM](https://arxiv.org/abs/2406.18665); [Hybrid LLM](https://iclr.cc/virtual/2024/poster/19625) (UNVERIFIED) | 2023–24 | Cascade/route to cheap vs expensive model | Big model **in path** when routed to | LLM never in path | Low |
| [RouteNLP](https://arxiv.org/abs/2604.23577) (2026) | 2026 | "Distillation-routing co-optimization loop that clusters escalation failures, applies targeted knowledge distillation to cheaper models, and automatically retrains the router"; 8-week pilot | Teacher serves escalations; distillation in background | Teacher never serves traffic; teaches structure | **Medium-High** |
| [Google, KD for online ranking](https://arxiv.org/abs/2408.14678) (2024) | 2024 | Production KD: "consistent and reliable generation of high quality teacher labels from a continuous data stream" | Big teacher continuously in background on live data | Teacher is an LLM emitting rules/schemas; discovery | **Medium-High** — existence proof of background-teacher-on-live-stream in production |
| [Event-triggered LLM invocation in streams](https://arxiv.org/abs/2607.13048) (2026); [TGL wake-trigger for proactive agents](https://arxiv.org/abs/2605.30152) (2026) | 2026 | When should a fast model wake the LLM (SPRT/CUSUM/Bayesian); temporal-graph model over `(actor, verb, object, t)` events replaces the LLM as trigger, 12–83× faster | LLM after trigger; no feedback to fast model | The teaching step | Medium (TGL is nearly s2w's System 1 on a symbolic event stream) |
| [LILAC](https://arxiv.org/abs/2310.01796) (2024); [CodeAD](https://arxiv.org/abs/2510.22986) (2025); [FlexLog](https://arxiv.org/abs/2406.07467) (2024) | 2024–25 | LILAC: LLM once on novelty, cache thereafter. **CodeAD**: LLM agent "iteratively generates, tests, repairs, and refines" Python rule functions for log anomaly detection, "directly executable on raw logs", <$4 LLM cost/dataset, no LLM at runtime | CodeAD: offline synthesis, online rule execution | Continuous re-teaching on the live stream; entity state not anomaly flags | **High** for claim (2) in the log domain |
| FunSearch (2023), [EoH](https://arxiv.org/abs/2401.02051), [ReEvo](https://proceedings.neurips.cc/paper_files/paper/2024/file/4ced59d480e07d290b6f29fc8798f195-Paper-Conference.pdf) (UNVERIFIED); [CAAFE](https://arxiv.org/abs/2305.03403), [LLM-FE](https://arxiv.org/abs/2503.14434) (UNVERIFIED) | 2023–25 | LLM evolves heuristics/features as code against a fitness signal; deployed code runs without the LLM | Offline search | Fitness = graded forecasts on the live stream | Medium (identical mechanism, different setting) |
| Confluent/Flink [`ML_PREDICT`](https://docs.confluent.io/cloud/current/ai/ai-model-inference.html) (UNVERIFIED); [VectraFlow](https://arxiv.org/abs/2604.03855) (2026) | 2024–26 | LLM as a stream operator; VectraFlow has LLM/embedding/hybrid tiers | **LLM in the stream path** | The inverse | Low-Medium |
| [TypeSafe System One / Jev](https://typesafe.ai/blog/introducing-system-one-models-and-jev) ([docs](https://docs.typesafe.ai/concepts/system-one), [llms.txt](https://docs.typesafe.ai/llms.txt)) | 2026 | "Unstructured state in, typed probabilistic decisions out"; Choice/Score/Noul primitives; probabilities "optimized against outcomes" (RLCD); 70–500 ms end-to-end; docs position Jev as router/guardrail *around* an LLM | **No documented training/teaching path, no mention of an LLM teaching Jev, no streams** | "LLM teaches Jev" is unclaimed by the vendor — and unsupported by any fine-tune API in the docs | Low as prior art; **medium as a dependency risk** (and note: Jev is a hosted ~100 ms API, so the µs tier must be rules/embeddings, not Jev) |

**Synthesis (judgment).** Each half of claim (2) has strong prior art; the conjunction does not. Nobody found runs a teacher that is *never* in the request path, teaches *structure* (types, identity keys, repair rules, forecaster definitions) rather than labels or weights, into a fast model over a *discovered* symbolic state, with graded forecasts as the fitness signal. Attribute CodeAD, RouteNLP, Google's ranking KD, LILAC and FunSearch explicitly or the novelty section reads as ignorance. Two engineering notes fall out: (a) the [event-triggered invocation](https://arxiv.org/abs/2607.13048) paper is a ready-made theory for "when does System 2 look at a snapshot"; (b) confirm with TypeSafe whether Jev can be taught at all before it is load-bearing in the architecture diagram.

---

## Q5. Wikipedia revert prediction

(Subagent A; read from the cited page unless marked UNVERIFIED or *inferred*. Includes two live measurements made 2026-09-27 against the public API — read-only.)

### Q5a. `revertrisk-language-agnostic` / `-multilingual` — the EXACT label

**Paper:** Trokhymovych, Aslam, Chou, Baeza-Yates, Saez-Trumper, "Fair multilingual vandalism detection system for Wikipedia", KDD 2023 — [arXiv 2306.01650](https://arxiv.org/abs/2306.01650) ([HTML](https://arxiv.org/html/2306.01650v1)).

- **Label = Wikimedia Data Lake field `revision_is_identity_reverted`.** The paper's data-record example shows `'revision_is_identity_reverted': 1` as the target; "The goal of our models is to predict whether a given revision will be reverted or not." Data source: "mediawiki history and mediawiki wikitext history datasets." Field docs: `revision_is_identity_reverted` — "whether this revision was reverted by another future revision"; `revision_seconds_to_identity_revert` also exists — [MediaWiki history dumps](https://wikitech.wikimedia.org/wiki/Analytics/Data_Lake/Edits/MediaWiki_history_dumps).
- **Time window: NONE. Revision radius: NONE.** The refinery job groups revisions by `(pageId, sha1)`: "First revision with a given sha1 is the base, others are reverts to the base"; a revision is identity-reverted if any later same-page revision has a sha1 equal to an earlier one — no time or count limit in the code; no-op revisions filtered — [DenormalizedRevisionsBuilder.scala](https://raw.githubusercontent.com/wikimedia/analytics-refinery-source/master/refinery-job/src/main/scala/org/wikimedia/analytics/refinery/job/mediawikihistory/denormalized/DenormalizedRevisionsBuilder.scala) (grep: zero mentions of window/48/radius). **Consequence: the production positive class includes reverts that land days or years later; "reverted within 30 min" is a strict subset.**
- **Negative class:** every non-bot revision not identity-reverted (*inferred*; the paper defines only the binary label plus the filters below).

| | Language-agnostic (LA) | Multilingual (ML) |
|---|---|---|
| Model card | [LA card](https://meta.wikimedia.org/wiki/Machine_learning_models/Production/Language-agnostic_revert_risk) | [ML card](https://meta.wikimedia.org/wiki/Machine_learning_models/Production/Multilingual_revert_risk) |
| Data | all Wikipedias, "observation period from 2022-01-01 to 2023-01-01", bots removed, 70/30 random split; no absolute size on card | 47 languages, 2022-01-01→2022-07-01 train, following week test; 8,586,362 train / 1,079,265 test; "up to 300,000 revisions per language" |
| Filters | bots removed | bots removed; "remove all reverting revisions that are further reverted by the next revision" (edit-war filter); anonymous-only training variant |
| Revert rate in data | not on card | **0.08** train / 0.07 test (all users); **0.28** anonymous-only (paper Table 1) |
| Model | XGBoost on metadata (user age, edit count, groups, page quality, edit-type counts) | mBERT features + mwedittypes + metadata → CatBoost |
| Code | [training notebook](https://gitlab.wikimedia.org/repos/research/knowledge_integrity/-/blob/research_notebooks/RRR/RevisionRevertsRisk_LanguageAgnostic.ipynb); [knowledge_integrity](https://gitlab.wikimedia.org/repos/research/knowledge_integrity); [published models](https://analytics.wikimedia.org/published/wmf-ml-models/revertrisk) | [KI_multilingual_training](https://github.com/trokhymovych/KI_multilingual_training) (`-f1` anon filter, `-f2` war filter) |
| Metrics | AUC **0.865**, 0.538 s/revision ([research page](https://meta.wikimedia.org/wiki/Research:Develop_a_ML-based_service_to_predict_reverts_on_Wikipedia)); LA card's Performance section is empty (checked 2026-09-27) | paper: AUC **0.88**, Pr@R0.75 0.28, F1 0.79 (all users); AUC 0.79 anonymous; ORES same test AUC 0.84 / 0.70. Research page: AUC 0.879, 3.13 s/rev |

- **Live scores are on EventStreams** (sampled 2026-09-27 via `curl`): `mediawiki.page_revert_risk_prediction_change.v1` carries `"model_name":"revertrisk-language-agnostic","model_version":"3","probabilities":{"true":0.526,"false":0.474}`; the multilingual stream carries `revertrisk-multilingual` v4. Schema `/mediawiki/page/prediction_classification_change/1.3.0`, with `rev_id`, `rev_sha1`, `rev_parent_id`, editor `edit_count`/`groups`/`is_temp`/`registration_dt`. **This is a free, production-grade baseline forecaster for Gate 4, published per edit, seconds after the edit** — use it as the strong baseline, and note that it is trained on a *different* (window-free) label.

### Q5b. MediaWiki revert tags

Source: [Manual:Reverts](https://www.mediawiki.org/wiki/Manual:Reverts); [phab T254074](https://phabricator.wikimedia.org/T254074) (GSoC 2020, resolved 2020-09-14).

| Tag | On | Trigger |
|---|---|---|
| `mw-undo` | reverting edit | undo saved without modification |
| `mw-rollback` | reverting edit | rollback link |
| `mw-manual-revert` | reverting edit | saved content SHA1-matches one of the last `$wgManualRevertSearchRadius` revisions |
| `mw-reverted` | **reverted** edit(s) | deferred `RevertedTagUpdate` job, after the reverting edit is auto-approved/patrolled |

- **15-revision window confirmed twice:** [`$wgManualRevertSearchRadius`](https://www.mediawiki.org/wiki/Manual:$wgManualRevertSearchRadius) default 15 (MW 1.36+); [`$wgRevertedTagMaxDepth`](https://www.mediawiki.org/wiki/Manual:$wgRevertedTagMaxDepth) default 15. T254074: "It will be probably 15 edits, as suggested by [a research paper]". **No time window** in the tag mechanics — revision counts only.
- **Tag latency:** job-queue; one live enwiki event showed `mw-reverted` landing **~56 s** after the revert (`rev_timestamp` 14:59:05Z, tag event `meta.dt` 15:00:01Z) — single example, not a distribution. enwiki-specific latency figure: none found (*inferred* seconds-to-minutes).
- enwiki lifetime tag counts ([Special:Tags](https://en.wikipedia.org/wiki/Special:Tags), read 2026-09-27): `mw-reverted` 26,125,893; `mw-undo` 13,327,056; `mw-rollback` 7,068,006; `mw-manual-revert` 5,591,501. Tags exist only since late 2020; 26.1M / ~400M edits since ≈ 6.5% — *inferred*, denominator estimated.

### Q5c. EventStreams — what carries the revert

Stream list parsed from [stream.wikimedia.org/?spec](https://stream.wikimedia.org/?spec) on 2026-09-27: `mediawiki.recentchange`, `mediawiki.revision-create`, `mediawiki.revision-tags-change`, `mediawiki.revision-visibility-change`, `mediawiki.page_change.v1`, `page-create/-delete/-undelete/-move`, `page-links-change`, `page-properties-change`, `page_revert_risk_prediction_change.v1`, `page_revert_risk_multilingual_prediction_change.v1`, `page_revert_risk_wikidata_prediction_change.v1`, `page_outlink_topic_prediction_change.v1`, `page_html_feature_counts_change.v1`.

- **There is no `mediawiki.page-revert` stream.** Revert outcomes arrive via (a) `mediawiki.revision-tags-change` or (b) your own sha1 match over `revision-create`.
- **`mediawiki.recentchange` has NO `tags` field** — [schema 1.0.1](https://schema.wikimedia.org/repositories/primary/jsonschema/mediawiki/recentchange/current.yaml); confirmed on a live event.
- **`mediawiki.revision-tags-change`** — [schema](https://schema.wikimedia.org/repositories/primary/jsonschema/mediawiki/revision/tags-change/current.yaml): `database`, `page_id`, `rev_id`, `rev_timestamp`, `rev_sha1`, `rev_parent_id`, `tags` (after), `prior_state.tags` (before). **Live 60 s sample, 2026-09-27:** 1,228 events, 96 with `mw-reverted`, 128 from enwiki. ⚠️ The short alias `revision-tags-change` returns HTTP 400 "Invalid streams"; use the full name.
- `mediawiki.page_change.v1` ([schema 1.12.0](https://schema.wikimedia.org/repositories/primary/jsonschema/mediawiki/page/change/current.yaml)): no revert or tags fields.
- Ops ([EventStreams](https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams)): SSE over Kafka, no server-side filtering, 15-min connection timeout (reconnect with `Last-Event-ID`), `since=` replay with retention "typically between 7-31 days", hourly canary events (`meta.domain === 'canary'`) to filter.

### Q5d. ORES `damaging` / `goodfaith` / `reverted`

- Definitions ([mediawiki.org/ORES](https://www.mediawiki.org/wiki/ORES)): `reverted` "predicts whether an edit will eventually be reverted"; `damaging` and `goodfaith` are **human-labelled** via Wiki Labels. Deprecation banner present.
- **`reverted` label = `reverted_for_damage`, 48-hour intent:** "edit was reverted within 48 hours, the reverting editor was not the same person" and it is "problematic in that many edits are reverted not because they are damaging but because they are involved in some content dispute" — [Research:ORES](https://meta.wikimedia.org/wiki/Research:ORES:_Facilitating_re-mediation_of_Wikipedia's_socio-technical_problems).
- **Code ([editquality autolabel.py](https://raw.githubusercontent.com/wikimedia/editquality/master/editquality/utilities/autolabel.py)):** `--revert-radius` default 15; `--revert-window` in hours, "If unset, no limit will be used"; `reverted_for_damage` = reverted AND NOT (self-revert OR reverted-back-to OR excluded comment). The [Makefile](https://raw.githubusercontent.com/wikimedia/editquality/master/Makefile) uses `--revert-radius=5` in all 45 autolabel calls and passes **no `--revert-window`** (only `Makefile.manual`'s wikidata target uses 48 h). So "48 h" is documented intent; the shipped labels relied on radius 5 with no window. **enwiki** has no autolabel step — only human labels (`enwiki.human_labeled_revisions.20k_2015.json`, [Wiki Labels campaign](https://labels.wmflabs.org/campaigns/enwiki/4/)).
- Revert-detection library: [`mwreverts.api.check(radius, before, window, …)`](https://pythonhosted.org/mwreverts/api.html) returns `reverting`, `reverted`, `reverted_to`.
- Paper: Halfaker & Geiger, [arXiv 1909.05189](https://arxiv.org/abs/1909.05189) (CSCW 2020). **Deprecation:** ORES → Lift Wing; `ores.wikimedia.org` → `ores-legacy`; goodfaith/damaging/reverted being replaced by Revert Risk — [wikitech ORES](https://wikitech.wikimedia.org/wiki/ORES); [editquality repo](https://github.com/wikimedia/editquality) deprecated late 2023.

### Q5e. ClueBot NG

[User:ClueBot_NG](https://en.wikipedia.org/wiki/User:ClueBot_NG) · [Documentation](https://en.wikipedia.org/wiki/User:ClueBot_NG/Documentation). Bayesian word/2-gram classifiers + edit statistics → ANN; threshold set for a target false-positive rate: **0.1% FP ≈ 40% catch rate** (0.25% FP ≈ 55%); "correctly classifies over 90% of edits" if optimising accuracy; dataset = original set + hand-classified review-interface edits, "some degree of bias"; post-filters (whitelist, edit-count thresholds, 1RR). Median time to revert **5 s** (Halfaker et al. 2013, [PDF](https://stuartgeiger.com/papers/abs-rise-and-decline-wikipedia.pdf)).

### Q5f. Base rates — share reverted, and time-to-revert

Published (enwiki unless noted):

| Figure | Population / period | Definition | Source |
|---|---|---|---|
| 2.9 / 4.2 / 4.9 / 5.8 % | all edits 2005–08 | reverts excl. vandalism and bots | [Chi/PARC 2009](http://asc-parc.blogspot.com/2009/08/part-2-more-details-of-changing-editor.html) |
| ~6.7% | all edits to Jul 2006 | comment-marked reverts (MD5 identity 3.71M vs comment 2.42M) | [Kittur et al. CHI 2007](https://gwern.net/doc/wikipedia/2007-kittur.pdf) |
| ~10% (2010) | all edits | "approximately 10% of all edits as of 2010" | [Halfaker, Kittur, Riedl WikiSym 2011](https://files.grouplens.org/papers/halfaker11bite.personal.pdf) |
| 4–6% | all edits, current | edits that *are* reverts (Wikiscan) | [Wikipedia:Statistics](https://en.wikipedia.org/wiki/Wikipedia:Statistics) |
| 6.7% | global, Jan 2020 | revert rate | [Research:Revert](https://meta.wikimedia.org/wiki/Research:Revert) |
| 8% / 7%; **28% anonymous** | 47 wikis 2022, bots removed | identity revert | [ML card](https://meta.wikimedia.org/wiki/Machine_learning_models/Production/Multilingual_revert_risk); [paper](https://arxiv.org/html/2306.01650v1) |
| 119.7M SHA-1 reverted edits → 74.8M after removing pseudo-reverts | all enwiki to May 2016 | full-page identity | [Kiesel et al. ICWSM 2017](https://downloads.webis.de/publications/papers/kiesel_2017c.pdf) |
| median **12.4 min** (ClueBot NG up) / **21.4 min** (down); "mostly between one minute and 24 hours" | reverted low-quality edits, 2011 | time-to-revert | [Geiger & Halfaker WikiSym 2013](https://stuartgeiger.com/wikisym13-cluebot.pdf) |
| median **7.6 min** (comment reverts) / **9.7 min** (MD5) / **11.3 min** (vandalism) | all edits to Jul 2006 | survival | [Kittur 2007](https://gwern.net/doc/wikipedia/2007-kittur.pdf) |
| median **14 min**, mean 758 min | 668 sampled edits 2004–06 | hand-judged | [Vandalism studies/Study1](https://en.wikipedia.org/wiki/Wikipedia:WikiProject_Vandalism_studies/Study1) |
| "nearly all reverts take place within 48 hours"; "94% of reverts can be detected by matching MD5 checksums" | enwiki | — | [Research:Revert](https://meta.wikimedia.org/wiki/Research:Revert) |
| mass deletions median 2.8 min, obscenities 1.7 min (2003) | enwiki | Viégas, Wattenberg, Dave CHI 2004 — **UNVERIFIED** | [historyflow](https://www.bewitched.com/historyflow.html) |
| 9 of 18 misinformation edits reverted; 3 within ~2.1 min, others 1.2 d–3.5 y | enwiki 2024 election | `revision_is_identity_reverted` | [Formisano et al. 2026](https://www.nature.com/articles/s41599-026-06810-2) |

**No published figure for "fraction of edits reverted within 30 minutes" was found (searches 2026-09-27).**

**Our measurement (enwiki mainspace, `mw-reverted` tag, read-only via `action=query&list=recentchanges`, ns 0, `rctype=edit`, one full UTC hour ≥48 h old so the tag job had landed):**

| Window (UTC) | Population | N | `mw-reverted` | Rate |
|---|---|---|---|---|
| 2026-09-25 13:00–14:00 | registered humans | 3,327 | 112 | **3.37%** |
| | anonymous/temp | 652 | 139 | **21.32%** |
| | bots | 405 | 1 | 0.25% |
| | all humans | 3,979 | 251 | **6.31%** |
| 2026-09-25 14:00–15:00 | all humans | 4,460 | 264 | **5.92%** (anon/temp 148/674 = 22.0%) |

**Time-to-revert, same 14:00–15:00 hour, first 120 of the 264 reverted edits (time order, capped for API rate limits), all resolved:** time from the edit to the first later revision (≤20 revisions) tagged `mw-undo`/`mw-rollback`/`mw-manual-revert` or with SHA1 equal to the edit's parent.

| Reverted within | Share of reverted edits (n=120) | Share of all human edits (× 5.92%) |
|---|---|---|
| 1 min | 25.0% | ~1.5% |
| 5 min | 37.5% | ~2.2% |
| 15 min | 48.3% | ~2.9% |
| **30 min** | **53.3% (64/120)** | **~3.2%** |
| 1 h | 63.3% | ~3.7% |
| 6 h | 79.2% | ~4.7% |
| 24 h | 95.0% | ~5.6% |

Median **19.1 min**, mean 267.5 min — consistent with Geiger & Halfaker's 12.4–21.4 min. **Caveats:** single hour, n=120, earliest-in-hour not random; 95% interval on 53.3% ≈ 44–62% (our estimate) → **30-min base rate ≈ 2.6–3.7% of human mainspace edits**; a later revert-tagged revision may have reverted a different edit (biases short); SHA1 check only against the parent; reverts deeper than 15 revisions never get the tag; edits on since-deleted pages are excluded. For a pre-registered number, rerun over several days with a random sample. Scripts and output are in the session scratchpad only.

### Q5g. Prior work predicting reverts / vandalism in (near) real time

| Work | Label / task | URL |
|---|---|---|
| PAN 2010 vandalism detection (Potthast et al.) | human-annotated PAN-WVC-10; winner AUC 0.92236; corpus 32,452 edits / 2,391 vandalism (UNVERIFIED counts) | [PAN'10](https://pan.webis.de/clef10/pan10-web/wikipedia-vandalism-detection.html) |
| WikiTrust + STiki + NLP (Adler, de Alfaro, Mola-Velasco, Rosso, West 2011) | same labels; RF AUC 0.92236 | [arXiv 1210.5560](https://arxiv.org/abs/1210.5560) |
| STiki (West, Kannan, Lee, EUROSEC 2010) | **rollback-based labels**, metadata-only real-time scoring; tool dead since Mar 2020 | [slides](https://www.andrew-g-west.com/docs/wiki_eurosec_slides.pdf) · [ACM](https://dl.acm.org/doi/10.1145/1752046.1752050) · [WP:STiki](https://en.wikipedia.org/wiki/Wikipedia:STiki) |
| VEWS (Kumar, Spezzano, Subrahmanian, KDD 2015) | **user-level**: 17,027 users blocked for vandalism vs 16,549 benign; 87.8% acc; detects 2.39 edits before ClueBot NG | [arXiv 1507.01272](https://arxiv.org/abs/1507.01272) |
| WSDM Cup 2017 Wikidata vandalism (Heindorf et al.) | label = rollback-reverted; 82.68M revisions, 0.24% vandalism; **online/near-real-time**; ROC-AUC 0.947, PR-AUC 0.458 | [arXiv 1712.05956](https://arxiv.org/abs/1712.05956) |
| ORES `reverted` (Halfaker & Geiger 2020) | reverted-for-damage, 48 h intent, radius 5–15 | [arXiv 1909.05189](https://arxiv.org/abs/1909.05189) |
| Revert Risk LA/ML (Trokhymovych et al. 2023) | identity revert, **no window**; served live on EventStreams | [arXiv 2306.01650](https://arxiv.org/abs/2306.01650) |
| Graph-Linguistic Fusion for Wikidata vandalism (2025) | LM classifier; label not in abstract | [arXiv 2505.18136](https://arxiv.org/abs/2505.18136) |
| Formisano et al. 2026 | uses ML revert-risk + identity-revert outcomes on 2024-election edits | [HSSC](https://www.nature.com/articles/s41599-026-06810-2) |

**No paper found (2026-09-27) that frames the task as "reverted within N minutes", and none that uses an LLM to predict Wikipedia reverts.** Closest real-time framings: WSDM Cup 2017 (online learning, Wikidata) and the live revert-risk streams.

### Q5 implications for Gate 4 (judgment)

1. **Three incompatible production definitions of "revert":** identity/SHA1 with no window (Data Lake, revertrisk); `mw-reverted` tag with ≤15-revision depth and no window (MediaWiki); reverted-for-damage within 48 h, radius 5–15 (ORES). A 30-minute window is stricter than all three. Pre-register which you inherit and say why (the tag is the cheapest live ground truth; SHA1-vs-parent is the most reproducible).
2. **Cheapest live ground truth:** join `revision-create` (or `recentchange`) to `mediawiki.revision-tags-change` on `(database, rev_id)`, check `"mw-reverted" ∈ tags`, and compare `rev_timestamp` of the *reverting* edit (found via `mw-undo`/`mw-rollback`/`mw-manual-revert` or SHA1) to the original. Tag latency ≈ a minute (one observation) — this is Grzenda's *verification latency*, distinct from the 30-min horizon (§Q6). Reverts deeper than 15 revisions are invisible to the tag path; sha1-match over `revision-create` catches them at the cost of holding page-history state.
3. **Base rate for the climatology baseline:** ≈ 3% of human mainspace edits reverted within 30 min (2.6–3.7%), ≈ 6% ever; ~21–22% for anonymous/temp accounts vs ~3.4% for registered. Brier of climatology at p=0.03 is ~0.029; a model must beat that, and the Wikimedia revert-risk stream is the honest strong baseline — trained on a different label, so report both.
4. **Novelty check for Gate 4 itself:** the *forecast* is not new (revert-risk already publishes per-edit probabilities live); the new parts are the horizon-specific label, the pre-registration, and grading under censoring. Say so.

---

## Q6. Forecast evaluation on streams

(Subagent B; every row from a fetched page unless marked.)

### Scoring rules and calibration
| Item | What it gives | URL |
|---|---|---|
| Brier 1950 | Squared-error probability score | [MWR](https://journals.ametsoc.org/view/journals/mwre/78/1/1520-0493_1950_078_0001_vofeit_2_0_co_2.xml) |
| Gneiting & Raftery 2007 | Proper / strictly proper scoring rules; log, Brier, CRPS | [JASA PDF](https://sites.stat.washington.edu/raftery/Research/PDF/Gneiting2007jasa.pdf) |
| Murphy 1973 | Brier = uncertainty + reliability − resolution | [J. Appl. Meteor.](https://journals.ametsoc.org/view/journals/apme/12/4/1520-0450_1973_012_0595_anvpot_2_0_co_2.xml) |
| Bröcker & Smith 2007 | Reliability diagrams need consistency bars | [WAF](https://journals.ametsoc.org/view/journals/wefo/22/3/waf993_1.xml) |
| Naeini 2015; Guo 2017; **Nixon 2019**; **Gruber & Buettner 2022** | ECE origin; popularisation; ECE's binning flaws; proper calibration errors | [AAAI](https://ojs.aaai.org/index.php/AAAI/article/view/9602) · [ICML](https://arxiv.org/abs/1706.04599) · [CVPRW](https://arxiv.org/abs/1904.01685) · [NeurIPS](https://arxiv.org/abs/2203.07835) |
| Mason 2004; Wilks 2019 | Brier skill score vs climatology; skill scores are harsh | [MWR](https://journals.ametsoc.org/view/journals/mwre/132/7/1520-0493_2004_132_1891_oucaar_2.0.co_2.xml) · [Wilks 4e](https://shop.elsevier.com/books/catalog/isbn/9780128158234) |

### Censoring and late labels
| Item | What it gives | URL |
|---|---|---|
| **Graf et al. 1999** | Time-dependent Brier BS(t) with IPCW; integrated Brier score | [PubMed](https://pubmed.ncbi.nlm.nih.gov/10474158/) |
| Gerds & Schumacher 2006 | Consistency of IPCW Brier | [Biom J](https://onlinelibrary.wiley.com/doi/abs/10.1002/bimj.200610301) |
| **Kvamme & Borgan, "Brier Score under Administrative Censoring"** (JMLR vol. 24; year inferred, UNVERIFIED) | When censoring is "the run stopped at clock time X", KM-IPCW is invalid; administrative Brier score needs no censoring-distribution estimate | [arXiv](https://arxiv.org/abs/1912.08581) · [JMLR](https://jmlr.org/papers/volume24/19-1030/19-1030.pdf) |
| Harrell 1996; Uno 2011 | C-index; IPCW-corrected C | [Stat Med](https://onlinelibrary.wiley.com/doi/10.1002/(SICI)1097-0258(19960229)15:4%3C361::AID-SIM168%3E3.0.CO;2-4) · [Stat Med](https://onlinelibrary.wiley.com/doi/abs/10.1002/sim.4154) |
| **Sonabend, Bender, Vollmer 2022, "C-hacking"** | Pre-select measures; pair discrimination with calibration and a proper score | [Bioinformatics](https://academic.oup.com/bioinformatics/article/38/17/4178/6640155) |
| van Houwelingen 2007 | Landmarking / dynamic prediction | [Scand J Stat](https://onlinelibrary.wiley.com/doi/abs/10.1111/j.1467-9469.2006.00529.x) |
| scikit-survival | `brier_score`, `integrated_brier_score`, `concordance_index_ipcw`; evaluation times must lie inside the follow-up range | [docs](https://scikit-survival.readthedocs.io/en/stable/user_guide/evaluating-survival-models.html) |

Mapping to "event within horizon H": a forecast at t₀ asks P(T ≤ t₀+H). Observed (min(T,C), δ): (a) event by t₀+H → 1; (b) followed past t₀+H, no event → 0; (c) follow-up ends before t₀+H with no event → **censored, not 0**. Graf handles (c) by IPCW; Kvamme–Borgan is the case when the cut-off is the run ending — which is the usual case for a live stream.

### Delayed feedback and prequential evaluation
| Item | What it gives | URL |
|---|---|---|
| Dawid 1984 | Prequential principle | [JRSS-A](https://rss.onlinelibrary.wiley.com/doi/abs/10.2307/2981683) |
| **Gama, Sebastião, Rodrigues 2013** | Prequential error with forgetting; convergence of holdout/prequential/windowed/faded estimators | [Machine Learning](https://link.springer.com/article/10.1007/s10994-012-5320-9) |
| **Grzenda, Gomes, Bifet 2020** | Delayed-labelling evaluation; *verification latency* is a delay distinct from the horizon; **continuous re-evaluation** of refined predictions | [DMKD](https://dl.acm.org/doi/10.1007/s10618-019-00654-y) |
| Gomes et al. 2022 | Survey of delayed/partially labelled streams | [CSUR](https://dl.acm.org/doi/10.1145/3523055) |
| River `progressive_val_score` | Reference implementation with a `delay` argument | [docs](https://riverml.xyz/latest/api/evaluate/progressive-val-score/) |
| Chapelle 2014; Ktena 2019; Yasui 2020; Yang 2021 | Delayed conversions: model the delay distribution; never treat not-yet-labelled as negative ("feedback shift"); importance weighting | [KDD](https://dl.acm.org/doi/10.1145/2623330.2623634) · [RecSys](https://arxiv.org/abs/1907.06558) · [WWW](https://arxiv.org/abs/2002.02068) · [AAAI](https://ojs.aaai.org/index.php/AAAI/article/view/16587) |

### Pre-registration and live-forecast practice
[NeurIPS pre-registration workshops 2020/2021](https://preregister.science) (papers reviewed without results; [PMLR 148](https://proceedings.mlr.press/v148/), [181](https://proceedings.mlr.press/v181/)); [Hofman et al., "Pre-registration for Predictive Modeling"](https://arxiv.org/abs/2311.18807); [OSF prereg](https://www.cos.io/initiatives/prereg) (timestamped, frozen; "Transparent Changes" doc); [Good Judgment Open scoring](https://www.gjopen.com/faq) (daily Brier averaged over open days, relative to median); Metaculus Baseline/Peer log scores ([FAQ](https://www.metaculus.com/help/scores-faq/) — **UNVERIFIED**, page blocked); [Mellers et al. 2014](https://journals.sagepub.com/doi/10.1177/0956797614524255); M4/M5/[M6 live competition](https://arxiv.org/abs/2310.13357); [Tashman 2000 rolling-origin](https://www.sciencedirect.com/science/article/abs/pii/S0169207000000650); [Nixtla CV](https://nixtlaverse.nixtla.io/statsforecast/docs/tutorials/crossvalidation.html).

### Existing forecast ledgers
[Metaculus](https://www.metaculus.com/help/scores-faq/) · [GJ Open](https://www.gjopen.com/faq) · [Manifold](https://docs.manifold.markets/faq) (creator-resolved) · [Fatebook](https://fatebook.io/) (open source; imports the retired [PredictionBook](https://predictionbook.com/)) · [Kalshi settlement](https://docs.kalshi.com/getting_started/market_settlement) · [Autocast](https://arxiv.org/abs/2206.15474) (NeurIPS D&B 2022) · [ForecastBench](https://arxiv.org/abs/2409.19839) (dynamic, ~1,000 unresolved questions, live leaderboard; humans beat top LLMs) · [Halawi et al. 2024](https://arxiv.org/abs/2402.18563) · [Prophet Arena](https://arxiv.org/abs/2510.17638) (ICLR 2026; live markets, multi-horizon). Also Complex Event Forecasting (§Q1b) — the DEBS community's own version of graded stream forecasts.

### What to adopt (judgment)
1. **Primary score Brier (or log), with Murphy decomposition and BSS vs running climatology.** ECE only as a secondary diagnostic with adaptive bins; prefer Gruber–Buettner's proper calibration error for one number.
2. **Treat "event within H" as right-censored survival data.** Grade only after (a) t₀+H has elapsed *and* (b) the label channel's verification latency has elapsed — for Gate 4 the `mw-reverted` tag comes from a deferred job, so these are different clocks. A forecast whose window was cut by the run ending is **censored, not negative**; record the reason. Use Kvamme–Borgan's administrative Brier as the default; Graf IPCW when censoring is genuinely random.
3. **Never coerce unobserved-within-horizon to 0 unless the stream is provably complete over the window** (Chapelle/Yasui/Yang feedback shift). Store a `stream_complete_through` watermark beside each grade; EventStreams' 7–31-day replay makes completeness checkable.
4. **Pre-declare the measure set** (Sonabend) so no metric shopping happens after the fact; add a `verify:` line per registered claim, matching the fleet's convention.
5. **Pre-registration record** (freeze on OSF or a signed tag *before the first forecast*): hypothesis; horizon and clock (event time vs ingestion time); label definition with the exact revert signal (`mw-reverted` vs SHA1 identity, 48h/radius-15 conventions); censoring rule; baselines; primary score; N or wall-clock stopping rule; stream start/cutoff; registration timestamp; code commit hash; change log.
6. **Baselines**: running base rate (mandatory), persistence, and a 3–5 feature logistic model (Chapelle's reference learner) — the honest "did the world model add anything" comparison. Note Rebmann 2022 / HoPF-style heuristics are the right *structure-recovery* baselines for Gate 3, separate from these *forecast* baselines.
7. **If forecasts are revised before grading**, use GJ Open's time-averaged daily Brier or Grzenda's continuous re-evaluation, not last-forecast-only.

---

## Q7. Venues (as of 2026-09-27)

| Venue | Fit | Next deadline | Pages | URL |
|---|---|---|---|---|
| **ACM DEBS 2027**, Galway, Jun 29–Jul 1 2027 | Best fit; CFP lists "complex event forecasting", "online relational learning" | Research abstract **Feb 16 2027**, paper **Feb 23 2027**; industry/GC/demo TBA | 12 pp research, 6 pp short/vision (2026 rules) | [2027](https://2027.debs.org/) · [2026 CFP](https://2026.debs.org/call-for-research-papers/) |
| DEBS Grand Challenge | 2026 ran a **Call for Challenges** (6-pp proposal with dataset + metrics + baseline); a Wikipedia-revert-forecast challenge is a plausible proposal | 2027 TBA | 6 pp | [GC](https://debs.org/grand-challenges/) · [call](https://2026.debs.org/call-for-grand-challenges/) |
| **PVLDB Vol. 20 → VLDB 2027**, Athens, Aug 2027 | Systems paper; rolling | Monthly, 1st of month, through **Mar 1 2027**; one revision | PVLDB standard (UNVERIFIED) | [dates](https://www.vldb.org/2027/important-dates.html) |
| VLDB 2027 demo | Live ledger demo | Not yet published (UNVERIFIED) | — | [vldb.org/2027](https://vldb.org/2027/) |
| **SIGMOD 2027**, Huntington Beach, Jun 2027 | Research; demo suits a live-graded ledger | Research R4 abstract **Oct 10**, paper **Oct 17 2026**; **demo Jan 11 2027**; industrial Nov 24 2026 | — | [dates](https://2027.sigmod.org/calls_papers_important_dates.shtml) · [demos](https://2027.sigmod.org/calls_sigmod_demos.shtml) |
| **ICDE 2027**, Copenhagen, May 2027 | "data streams, CEP", "AI/ML for data systems" | Round 2 **Nov 11 2026** | 12 pp | [CFP](https://icde2027.github.io/cf-research-papers.html) |
| CIDR 2027 | 6-pp ideas papers | Aug 4 2026 passed; CIDR 2028 ~Aug 2027 (UNVERIFIED) | 6 pp | [CFP](https://www.cidrdb.org/cidr2027/cfp.html) |
| EDBT 2027, Lille, Apr 2027 | Research or 6-pp vision | Round 3 **Oct 7 2026** | 12/6 | [dates](https://edbticdt2027.github.io) |
| **ICPM 2027**, Rende, Feb 8–12 2027 | Adjacent (online process mining; OCEL export makes s2w directly relevant) | Research passed (Sep 18 2026); **demos Nov 2 2026**; workshop papers **Nov 18 2026** | — | [dates](https://icpmconference.org/2027/important-dates/) |
| SMA4PM @ ICPM 2027 (Streaming Management & Analytics for PM) | Exactly "streaming + predictive + drift" | Nov 18 2026 (from ICPM page) | — | [site](https://sma4pm.github.io/) |
| **TheWebConf (WWW) 2027**, Dublin, May 2027 | Where Wikipedia work lands | Research abstract **Oct 18**, paper **Oct 25 2026**; short **Nov 9/16 2026** | 8 pp + refs (12 max); short 4 pp | [research](https://www2027.thewebconf.org/research-track-papers/) · [dates](https://www2027.thewebconf.org/important-dates/) |
| NeurIPS Evaluations & Datasets track (renamed from D&B) | A forecast-grading protocol is in scope | 2026 passed (May); 2027 ~May (UNVERIFIED) | — | [CFP](https://neurips.cc/Conferences/2026/CallForEvaluationsDatasets) |
| NeurIPS 2026 workshop **FMTS: Foundation Models for Temporal Systems — From Forecasting to World Modeling** | Nearly the s2w thesis | Sep 15 2026 passed; watch for 2027 | 4 pp | [site](https://fmts-workshop.github.io/) |
| ICLR 2027 workshops | List published Nov 29 2026; paper deadlines ~Feb 2027 (UNVERIFIED) | — | — | [call](https://iclr.cc/Conferences/2027/CallForWorkshops) |
| KDD 2027 ADS track | Requires a *deployed* application with post-launch metrics | Cycle 1 passed; cycle 2 ~Feb 2027 (UNVERIFIED) | 8 pp + refs | [CFP](https://kdd2027.kdd.org/applied-data-science-ads-track-call-for-papers/) |
| WSDM 2027; CIKM 2027 | Web mining; CIKM resource track (4 pp) suits "the ledger as a resource" | WSDM passed; CIKM ~May 2027 (UNVERIFIED) | — | [WSDM](https://wsdm-conference.org/2027/cffp.html) · [CIKM resource](https://cikm2026.diag.uniroma1.it/resource-papers/) |
| CSCW | Wikipedia sociotechnical work; **rolling** via PACM HCI, no fixed deadline | Rolling | — | [rolling](https://cscw.acm.org/rolling.html) |
| Wiki Workshop | 2-pp non-archival extended abstracts; virtual; free feedback pass | 13th ed. passed (Jan 2026); 14th ~Jan 2027 (UNVERIFIED) | 2 pp | [meta](https://meta.wikimedia.org/wiki/Wiki_Workshop) |
| Wikimedia Research Showcase | Monthly talk series — **paused as of April 2026** per the page | — | — | [page](https://www.mediawiki.org/wiki/Wikimedia_Research/Showcase) |

**Nearest actionable deadlines:** EDBT R3 Oct 7 · SIGMOD R4 Oct 10/17 · **WWW Oct 18/25** (short Nov 9/16) · ICPM demos Nov 2 · ICDE R2 Nov 11 · SMA4PM Nov 18 · SIGMOD industrial Nov 24 · SIGMOD demo Jan 11 2027 · **DEBS Feb 16/23 2027** · PVLDB monthly to Mar 1 2027.

**Venue judgment.** DEBS 2027 research (Feb 2027) is the primary target — the community that owns CEP, CEF and stream benchmarks, and the one whose reviewers will already know iCEP and Wayeb. File a **DEBS Grand Challenge proposal** built from the Gate 4 stream in parallel; it costs 6 pages and puts the revert-forecast benchmark in front of the whole field. WWW 2027 only if Gate 4 results exist in four weeks; otherwise its Nov 16 short-paper slot. Wiki Workshop 2027 as a free 2-page feedback pass. NeurIPS 2027 E&D for the evaluation-protocol contribution once the ledger has months of graded history. Practical path regardless: arXiv (cs.DB, cross-list cs.LG) + blog + HN, with the pre-registration record public *before* results and the graded ledger linked, ForecastBench-style.

---

## Appendix — how to phrase the claim (judgment)

Weak (contestable in one comment): *"s2w discovers a world model from any event stream with no schema — nobody has done this."*

Defensible: *"Given only an event stream, s2w proposes entity types, identity keys and relationships (as object-centric process mining does offline — Rebmann 2022 — and as 2026 database work does for keys — Tursio, LLM-FK), compiles them into rules a microsecond path applies with the LLM never on the event path (unlike Graphiti, LILAC or Flink `ML_PREDICT`), maintains the result as a replayable projection (as Rama and Kafka Streams do for hand-written projections), and records forecasts about entities as immutable records graded against the stream under proper scoring with censoring (as CEF and ForecastBench do for pattern- and question-level forecasts). We test whether the LLM recovers structure better than heuristics and better than an LLM shown raw events, including on obfuscated streams where attribute names carry no information."*

Every clause in the second version has a citation in this document.

---

## Design implications (dispositions pending)

1. Name Zep/Graphiti ourselves in the README and launch, with the real differences. → pending
2. "Possible worlds": keep in the pitch, define it in one sentence (MCDB-style), say "rollout" or
   "sampled future world" in technical prose. → pending
3. Export OCEL 2.0 and adopt its vocabulary (object type, E2O, O2O) in `s2w-model`. → pending
4. Rebmann et al. BPM 2022 as a gate-3 comparison, or as the cited closest academic prior art. → pending
5. Wikimedia's revert-risk label has no time window, so B2's raw scores answer a different
   question; recalibration stays mandatory. → adopted: contract A5/A10 (v2, 2026-09-27)
6. Scoring: Murphy decomposition of the Brier score; treat "within H" as right-censored; never
   coerce unobserved outcomes to 0; pre-register with a signed tag before the first forecast.
   → pending (partly adopted: A4 never coerces unobserved to negative)
7. Venue: ACM DEBS 2027 (deadline around 2027-02-16) plus a Grand Challenge proposal. → pending
   (stream2worlds#1)
