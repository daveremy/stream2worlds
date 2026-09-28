# 0008: A domain-specific dashboard from a domain-free core

- **Question:** how can a generic engine that works over any event stream show each domain in its
  own terms, and can it show people things they did not know to ask? Dave, 2026-09-28, after
  watching the web view demo: *"how can we have a generic, abstracted technology that works over
  any technology have a powerful dashboard specific to the domain? i think the answer must rely
  on llms (system 2) to customize the dashboard for the domain. what are the major concerns of
  this domain, what type of live model should we show, what iconography, what colors even? a
  dynamic dashboard based on the data and the domain that can shift as we learn the domain.
  streaming implies incremental learning for the lifetime of the stream. there is a baseline of
  knowledge, repeated things, and there are surprises (attention heads?). can we help a user
  discover things they don't even know they need to know?"* Same day, 06:02: *"we need to see X
  events or have system 2 tell us when enough events have been received to have an opinion on an
  effective dashboard. system 2 is where opinions live."* Ruling the same day: visualization is
  first class and co-developed with the core, and so is agent usage (decision 0017).
- **Date:** 2026-09-28 · **Researcher:** sagan (with six research subagents) · **Status:** done
- **Feeds:** decision 0017 (view and agents first class); s2w#114 (domain-free view), s2w#116
  (System 2 view spec), s2w#117 (baseline and surprise feed); the gate-3 epic (s2w#13), where
  System 2 first authors a view spec, including on the obfuscated copy; the gate-4 epic (s2w#14),
  where accepted surprise questions become graded forecasts; the MCP tool set (decision 0009,
  `attention()`, `world.describe`).

Conventions as in the index: every claim cites the page used; **UNVERIFIED** means seen only in
a search snippet, an abstract or a third-party write-up, not a fetched primary page. Facts and my
judgments are kept apart; judgments are labelled **Judgment**. Karpathy's three hypotheses from
s2w#116 and s2w#117 are tested in order: the dashboard is data (§1–§3), shape before domain
(§4), a lifetime baseline plus surprise (§5–§7). §8 is the readiness question added after
Dave's 06:02 message. §9 is the agent angle.

---

## Verdict

1. **"The dashboard is data" is the industry consensus as of 2026, not a bet.** Every
   generative-UI protocol that shipped in the last twelve months — Google's A2UI (2025-12-15),
   Vercel's json-render (January 2026), MCP Apps (stable 2026-01-26) and OpenAI's Apps SDK — has
   the model emit a declarative description validated against a closed catalog, and the host
   renders it. A2UI states it as policy: *"a declarative data format, not executable code"*, and
   the client *"can only request to render components from that catalog"*
   ([Google](https://developers.googleblog.com/introducing-a2ui-an-open-project-for-agent-driven-interfaces/)).
   The specific shape they converged on — a flat map of id'd nodes, children by reference, data
   bindings as paths rather than values — is the shape the view spec should take (§2).
2. **The failure to design against is valid-but-wrong, not invalid.** On VisEval's 2,524
   natural-language-to-chart tasks, GPT-4 produced an unrenderable chart 3.3% of the time and a
   renderable chart that answered the wrong question 21.4% of the time
   ([Chen et al. 2024](https://arxiv.org/html/2407.00981)). A JSON schema catches the 3%. The
   21% needs a second, semantic layer: Draco-style hard constraints over the spec ("a state-flow
   panel requires a field System 1 has classified as a state") plus the repair loop LIDA and
   Grafana Assistant both landed on — feed the validator's error back to the model and try once
   more (§3). **Judgment:** the constraint layer is where hypothesis 1 becomes real; the schema
   alone is the easy 3%.
3. **"Shape before domain" has strong precedent for keys, timestamps and chart choice, and none
   for the two shapes the demo needs most.** Keys and foreign keys have name-free detectors
   (Rostin et al. 2009, Zhang et al. 2010, HoPF); CompassQL and Grafana ship explicit type-to-mark
   rules; Sherlock, Sato and DODUO are values-only by construction. But no shipped open-source
   tool infers a latitude/longitude pair from *values* — kepler.gl, datasette-cluster-map and Lux
   all match column *names* — and no paper picks out "which field is the status field"
   automatically (§4). Those two detectors are ours to write, and they are the ones that make the
   obfuscated ADS-B split screen work.
4. **Surprise as −log p is the right feed score; Bayesian surprise is the right retirement
   rule.** Itti and Baldi's surprise is the KL divergence between the observer's prior and
   posterior — how much an observation *moves beliefs*, not how improbable it was — which is why
   TV static carries 20× the Shannon information of programme content and draws no attention
   ([Itti & Baldi 2009](https://pmc.ncbi.nlm.nih.gov/articles/PMC2782645/)). Their implementation
   is a decayed Poisson–Gamma rate model, exactly s2w#117's "rate break" case. Compute both
   quantities from the same conjugate counters: −log p ranks the feed, and a surprise stops
   appearing when repeats no longer move the posterior (§5). Dave's "attention heads" has a
   literal counterpart in the literature: **precision weighting** (Friston), a scalar per
   baseline that scales surprise by how confident the baseline is — a first occurrence in a
   field with three prior observations is not a surprise.
5. **The insight literature measured the failure hypothesis 3 is built to avoid.** In a
   controlled study, over 60% of insights users reported from visual exploration were false, and
   confirming them with a test on the *same* data still left 11% false, double what α = 0.05
   promises; confirming on held-out data removed the inflation
   ([Zgraggen et al., CHI 2018](https://cs.brown.edu/research/ptc/assets/publications/zgraggeninvestigating.pdf)).
   An LLM narrating a surprise feed is a garden of forking paths at machine speed
   ([Gelman & Loken 2013](https://sites.stat.columbia.edu/gelman/research/unpublished/p_hacking.pdf)).
   Turning a proposed insight into a forecast about *future* events is Zgraggen's held-out
   confirmation with the future as the validation set. Three mechanisms cover every measured
   failure mode (§7): freeze the question before the grading data exist, grade out of sample,
   deduplicate against the live question set. No system in §6 does all three.
6. **"Enough events" is a stopping problem, and the fixed-N answer is the known-wrong one.**
   Production tools encode "enough" as fixed samples — BigQuery's 500 rows, DuckDB's 20,480
   objects — and their bug trackers hold the consequences (a field absent from the sample fails
   or is silently coerced later). Anytime-valid inference (confidence sequences, e-processes)
   exists precisely for a statistic that is peeked at continuously
   ([Howard et al. 2021](https://projecteuclid.org/journals/annals-of-statistics/volume-49/issue-2/Time-uniform-nonparametric-nonasymptotic-confidence-sequences/10.1214/20-AOS1991.full);
   [Ramdas et al. 2023](https://projecteuclid.org/journals/statistical-science/volume-38/issue-4/Game-Theoretic-Statistics-and-Safe-Anytime-Valid-Inference/10.1214/23-STS894.full)).
   Good–Turing's unseen mass N1/N and Chao1 give domain-free "how much schema is still unseen"
   estimates; racing gives "the label field is settled" with a named tie as the "not yet"
   reason (§8). System 2 should emit a typed `{ready, waiting_for}` rather than free-text
   hedging, because reasoning-tuned models abstain about 24% less than their base models
   ([AbstentionBench 2025](https://arxiv.org/abs/2506.09038)).
7. **The agent angle is settled on one point and open on another.** Every protocol keeps "what
   the model sees" as structured content and the rendered UI as human-only; Grafana's MCP server
   exposes a dashboard as a document read by section (summary, JSONPath property, panel queries)
   with the rendered image as an opt-in exception (§9). Chart pixels are a poor channel for an
   agent that must quote a number: frontier models read values off unlabelled charts with about
   6–7% error ([CHI 2026](https://arxiv.org/abs/2606.29808)). What nobody ships is one view
   definition served to both the renderer and the agent, so "the agent explains the dashboard the
   person is looking at" holds by construction. `[no-record: view spec as an event in the domain
   log, served to renderer and agent alike — searched web, arXiv, GitHub 2026-09-28]`. That is
   the novel part of decision 0017, and worth naming as such.

---

## 1. LLM-generated visualization: what works and what breaks

### 1a. Systems

| System | Date | What the LLM writes | How it is checked | Reported numbers |
|---|---|---|---|---|
| [LIDA](https://arxiv.org/abs/2303.02927) (Microsoft) | 2023-03 | Fills a constrained region of a code scaffold per grammar (Altair, Matplotlib, …), fill-in-the-middle | Execute and filter non-compiling code; GPT-4 self-evaluation on six dimensions produces natural-language repair instructions, re-applied | Visualization error rate 3.5% over 2,200+ charts, baseline over 10% ([README](https://github.com/microsoft/lida)) |
| [Data Formulator](https://arxiv.org/abs/2309.10094) (Microsoft) | 2023-09 | Data *transformation* code only; the user binds concepts to encoding shelves | The user chooses the encoding; the LLM never does | 10-participant study |
| [Data Formulator 2](https://arxiv.org/abs/2408.16119) | 2024-08 | Same, plus "data threads": a navigable history of (transformation, chart) steps | Branch from any prior step | 8-participant study; 0.7 release 2026-05-28 adds agents that "write and run code in an isolated environment" ([MSR blog](https://www.microsoft.com/en-us/research/blog/data-formulator-0-7-ai-powered-data-analytics-for-enterprise-data/)) |
| [Grafana Assistant](https://grafana.com/blog/llm-grafana-assistant/) | 2025-05 | Dashboard JSON, edits, queries | *"Errors are automatically fed back into the conversation, giving the LLM a chance to correct mistakes"* | None published |
| [NL4DV](https://arxiv.org/abs/2008.10723) | 2020 | (pre-LLM) A JSON analytic specification plus ranked Vega-Lite specs | Semantic parsing, rule-based | — |
| ChartGPT ([arXiv 2311.01920](https://arxiv.org/abs/2311.01920)) | 2023-11 | Vega-Lite via six sequential sub-tasks following grammar-of-graphics stages | Fine-tuned FLAN-T5 | **UNVERIFIED** beyond the abstract |

Two design choices recur. Data Formulator keeps the LLM out of the *encoding* decision entirely;
LIDA lets it in, but through a scaffold with executable checks and a repair pass. Both ship.

### 1b. How often the chart is wrong

[VisEval](https://arxiv.org/html/2407.00981) (Chen et al., TVCG 2024) is the cleanest
measurement. 2,524 queries over 146 databases, three checkers: *validity* (does it render),
*legality* (right data mapping, chart type and sort), *readability* (GPT-4V-judged overlap and
contrast).

| Model | Invalid | Valid but wrong | Pass |
|---|---:|---:|---:|
| GPT-4 | 3.29% | 21.44% | 75.27% |
| GPT-3.5 | 8.79% | 29.42% | 61.79% |
| Gemini Pro | 14.35% | 34.06% | 51.59% |
| CodeLlama-7B | 42.95% | 28.88% | 28.17% |

The dominant error classes are wrong aggregation from misreading the table, wrong visual
mapping, and wrong ordering. Charts needing three channels (stacked or grouped bars) underperform
two-channel variants for every model. A 2026 study translated Draco's rules into natural language
and asked small models to spot violations: near-perfect prompt adherence, F1 up to 0.82 on common
violations, **under 0.15 on subtler perceptual rules**
([arXiv 2602.20137](https://arxiv.org/abs/2602.20137)). Misleading-chart detection is similar:
models detect manipulated visual encodings 45% of the time ([Misleading ChartQA](https://arxiv.org/abs/2503.18172),
**UNVERIFIED**), and can be prompted into *producing* misleading charts that fool both models and
people ([ChartAttack](https://arxiv.org/abs/2601.12983)).

**Judgment.** For s2w the LLM's job is smaller than VisEval's — it picks a form and panels over a
world whose types System 1 already classified, and never writes a query that computes a number —
so the 21% is an upper bound, not a forecast. But the error *kind* transfers: the model will
choose a form the data cannot support (a state flow over a field with 4,000 distinct values). That
is a constraint check, not a schema check.

### 1c. Semantic colors and icons

The question "what iconography, what colors even?" has a fifteen-year literature.
[Lin et al., EuroVis 2013](https://idl.cs.washington.edu/files/2013-SemanticColor-EuroVis.pdf)
choose *semantically resonant* colors for categories from image search histograms and show they
speed chart reading versus a standard palette. Setlur and Stone (TVCG 2016) define a term's
*colorability* from n-gram co-occurrence with color words (**UNVERIFIED** numbers).
[Setlur and Mackinlay, CHI 2014](https://dl.acm.org/doi/10.1145/2556288.2557408) generate icon
encodings from a category label through NLP and web imagery. The LLM-era follow-ups are
cautionary: concept-to-color output from GPT-4o-mini, CLIP and RoBERTa depends on *"model design,
training data and context"* ([Word2Color, TVCG 2026](https://www3.cs.stonybrook.edu/~mueller/research/pages/Word2Color/)),
and persona effects on LLM color choice are model-dependent
([arXiv 2607.02455](https://arxiv.org/abs/2607.02455)).

**Judgment.** Karpathy's "fixed icon set, color roles" is the right constraint. Let the model pick
a *role* from an enum (`primary`, `warning`, `state:terminal`, …) and an icon *name* from a shipped
set; never a hex value. The design page's grammar — solid is actual, dashed ochre is possible,
dash patterns back up color — is a role system already; the spec should reference those roles.

---

## 2. The shape of a view spec: what the declarative-UI protocols agree on

| Protocol | Date | Representation | Validation | Safety boundary |
|---|---|---|---|---|
| [A2UI](https://a2ui.org/introduction/what-is-a2ui/) (Google) | 2025-12-15, v0.9.1 in 2026 | Flat list of components with id references; data bindings as paths (`{"path": "/booking/date"}`); messages `createSurface`, `updateComponents`, `updateDataModel` | Client-held catalog; unknown types are not rendered | "Declarative data format, not executable code" |
| [json-render](https://github.com/vercel-labs/json-render) (Vercel Labs) | 2026-01 | `{root, elements: {id: {type, props, children: [ids]}}}` streamed as JSON Patch | `defineCatalog` with Zod prop schemas | Model can only use catalog entries |
| [MCP Apps](https://blog.modelcontextprotocol.io/posts/2026-01-26-mcp-apps/) (SEP-1865) | stable 2026-01-26; official extension 2026-07-28 | Server predeclares a `ui://` HTML resource; tools point at it via `_meta.ui.resourceUri` | Host reviews predeclared templates | Sandboxed iframe, CSP from declared domains, JSON-RPC over `postMessage` |
| [OpenAI Apps SDK](https://developers.openai.com/apps-sdk/reference) | 2025-10 | HTML widget; `structuredContent` (model reads) vs `_meta` (widget only) vs `widgetState` (persisted) | — | Iframe; the three-channel split |
| [AG-UI](https://docs.ag-ui.com/introduction) | 2025 | Event stream; `STATE_SNAPSHOT` plus `STATE_DELTA` as RFC 6902 JSON Patch | App-owned | "Event-sourced diffs" |
| [Vega-Lite](https://vega.github.io/vega-lite/) | 2017 | JSON grammar with a published JSON Schema | Schema | The substrate for nvBench, Data2Vis, NL4DV, VizLinter, CompassQL |
| [Mosaic](https://idl.uw.edu/papers/mosaic) (Heer & Moritz, TVCG 2024) | 2024 | vgplot spec (YAML/JSON); clients publish *data needs* as declarative queries | A coordinator runs and optimizes the queries against DuckDB | Spec here, query execution there |
| Grafana [schema v2](https://grafana.com/blog/dynamic-dashboards-grafana-12/) | 2025-05 | Layout separated from panel "elements" because in v1 *"even the smallest UI tweak can trigger massive JSON changes"* | Foundation SDK typed builders; still experimental | Numbered versions with author, message and JSON diff |
| [Perses](https://perses.dev/) (CNCF) | 2024 | Dashboard model validated by CUE schemas | Plugins add panel kinds with static validation | **UNVERIFIED** details |

Thesys's June 2026 survey sorts generative UI into three tiers — *static* (model selects prebuilt
components), *declarative* (model composes a spec from a catalog; "most production work"),
*open-ended* (model writes HTML, sandboxed iframe mandatory) — and measures the open-ended tier at
roughly twice the output tokens of declarative
([State of Generative UI](https://www.openui.com/blog/state-of-generative-ui-report); the
vendor's own benchmark, treat the ratios as **UNVERIFIED**).

**Judgment.** Four things transfer directly to the view spec.

1. **Flat map of id'd nodes, children by reference.** It streams, diffs and patches well, which
   is what storing versions as events needs. A2UI and json-render both chose it for LLM
   generation specifically.
2. **Bindings, not values.** A panel carries a *query* (a path into the world API, with
   parameters), never data. This is hypothesis 1's "presentation only", and it is how A2UI's data
   model works.
3. **Layout separate from panels** (Grafana's v2 lesson), so System 2 revising one panel does not
   rewrite the whole spec, and a spec diff is readable.
4. **Mosaic's split** is the closest architectural precedent: the spec declares data needs; a
   coordinator (here `s2w-app::query`) executes them. The renderer never talks to the fold.

The one thing none of them has is a spec that lives *in the same log as the domain events*.
Grafana keeps dashboard versions in its own store; AG-UI event-sources UI state but not alongside
the data it displays. `[no-record: as in Verdict 7]`.

---

## 3. Constraints the LLM cannot violate

### 3a. Draco: hard constraints, soft constraints, and linting from one rule base

[Draco 2](https://idl.cs.washington.edu/files/2023-Draco2-VIS.pdf) (Yang, Moritz et al., VIS 2023)
represents a chart as answer-set-programming facts — `attribute((mark,type),m,bar).` — and states
design knowledge as **hard constraints** (a `violation` makes a spec inadmissible: a nominal field
on a continuous scale) and **soft constraints** (weighted preferences the solver minimizes).
Weights are learned from pairwise human-preference data with a RankSVM-style model. The same rule
base does completion (recommend from a partial spec) and linting (score a full spec).
[VizLinter](https://arxiv.org/abs/2108.10299) reuses it as a linter plus a fixer;
[Dziban](https://idl.uw.edu/papers/dziban) adds similarity to an *anchor* chart so a
recommendation stays close to what the user already has. [CompassQL](https://github.com/vega/compassql)
takes the other route: a Vega-Lite-shaped query with wildcards, enumerated and pruned by
expressiveness rules, then ranked. Its source has the concrete defaults: an integer field is
nominal if `distinct < 40 && distinct/count < 0.05`; nominal becomes a **key** if
`distinct/count > 0.8 && count > 50`; Q×Q → point, Q×N → bar, Q×T → line
(`schema.ts`, `mark.ts`, `typechannel.ts`).

**Judgment.** The pattern across the literature is: a neural component proposes, a symbolic layer
decides admissibility, soft preferences rank the survivors. For s2w the symbolic layer is small
and domain-free, because System 1's shape classification (§4) supplies the facts:

- `form:map` requires a field pair classified `geo:latlon`;
- `form:state-flow` requires a field classified `state` (low cardinality, per-entity transitions);
- `form:timeline` requires a `time` field and a primary entity;
- `form:graph` requires at least one relationship kind with degree spread;
- every panel query must be a registered query over the world API with parameters drawn from the
  world's actual types and fields — never free text.

Hard constraints reject; the rejection text goes back to the model once (LIDA's and Grafana's
repair loop); a second rejection falls back to the System 1-derived v0 spec (§10). Do not repair
silently on the server: a repaired spec is a spec nobody wrote.

### 3b. Schema-constrained emission is now cheap

OpenAI's strict structured outputs score 100% schema adherence on their complex-schema test where
the prior model scored under 40% ([OpenAI 2024-08](https://openai.com/index/introducing-structured-outputs-in-the-api/)).
[JSONSchemaBench](https://arxiv.org/abs/2501.10868) (10,000 real schemas) finds coverage varies
by engine (Guidance 0.86–0.98; Outlines 0.03 on the hardest GitHub set) and that constrained
decoding *improves* downstream accuracy by up to 4 points while speeding generation about 50%.
**Judgment:** keep the view-spec schema inside the subset the strict modes support (bounded enums
for icons and roles, no `patternProperties`), validate server-side anyway, reject rather than
repair.

### 3c. Metamorphic checks

McNutt, Kindlmann and Correll's *visualization mirages* (CHI 2020,
[doi 10.1145/3313831.3376420](https://doi.org/10.1145/3313831.3376420), **UNVERIFIED** beyond the
abstract) propose perturbing the data or spec and flagging charts whose reading changes. s2w has
the perturbation built in: the obfuscated copy. **Judgment:** the gate-3 test for a view spec is
that the plain and obfuscated streams yield the same *form* and panel set (labels may differ);
a spec that changes form under renaming was keyed on names, which hypothesis 2 forbids.

---

## 4. Shape before domain: what can be detected from values alone

### 4a. Semantic type detection is values-only by construction

| System | Date | Input | Headline number |
|---|---|---|---|
| [Sherlock](https://arxiv.org/abs/1905.10688) (KDD 2019) | 2019 | 1,588 features from column *values*: character distributions, word embeddings, paragraph vectors, global statistics; headers used only to build labels | 78 types, support-weighted F1 0.89 |
| [Sato](https://www.vldb.org/pvldb/vol13/p1835-zhang.pdf) (VLDB 2020) | 2020 | Adds table context: a 400-topic LDA over cell values ("without any headers") and a CRF over columns | F1 0.925 weighted, 0.735 macro |
| [DODUO](https://arxiv.org/abs/2104.01785) (SIGMOD 2022) | 2022 | BERT over the serialized table; "only considers the table content (i.e., cell values)" | 94.3 micro / 84.6 macro on VizNet |
| [ArcheType](https://arxiv.org/abs/2310.18208) (VLDB 2024) | 2024 | Zero-shot LLM; column name "not required"; adding names and statistics *degrades* zero-shot | Zero-shot F1 66.0 (SOTAB), 87.3 (D4) |
| [Babamahmoudi et al., TaDA 2025](https://www.vldb.org/2025/Workshops/VLDB-Workshops-2025/TaDA/TaDA25_2.pdf) | 2025 | Single-column, "without any additional context from the table" | GPT-4.1-mini with retrieval 0.81 micro / 0.52 macro; DODUO 0.76 / 0.47 |

Two caveats carry weight. DODUO's accuracy on VizNet drops from 98/91 (micro/macro) where test
columns fully overlap training values to 67/42 below 10% overlap
([WWW 2025 companion](https://dl.acm.org/doi/10.1145/3701716.3715530)) — the benchmarks
partly measure memorization. And prompted LLMs "hallucinate types on 2.7–47.6% of columns" on new
data lakes ([LakeHopper, 2026](https://arxiv.org/abs/2602.08793)). On hashed ids these methods all
degrade to "identifier", which is the right answer.

**Judgment.** Research 0002 already places Sherlock-class features in H. For the *view*, what
matters is a coarser classification than 78 types: identifier, categorical, state, numeric
measure, time, geo, free text. That classification is what §3a's constraints consume.

### 4b. Geo: every tool matches names

kepler.gl's point-pair detection is `findPointFieldPairs` matching suffix regexes for
`lat`/`lng`/`lon`/`long`/`latitude`/`longitude`; it applies no range check on values
(`src/table/src/kepler-table.ts`, [kepler.gl](https://github.com/keplergl/kepler.gl)). H3 cells
*are* detected from values (ten sampled strings pass `h3IsValid`). datasette-cluster-map and Lux
(`like_geo()` is true only for columns named `state` or `country`) are name-based too. Great
Expectations, Deequ and visions have no lat/lon inference. `[no-record: an open-source detector
that infers a lat/lon pair from values — searched GitHub and the profiling literature 2026-09-28]`

**Judgment.** The value rule is folklore, not published: two numeric fields, one within ±90 and
one within ±180, similar decimal precision, high per-event co-presence, non-degenerate joint
spread (not all points on one line), and successive values for the same entity that move at
plausible speeds. Publishable as a small contribution; essential for the ADS-B demo on the
obfuscated copy.

### 4c. Time

Rust [`dateparser`](https://docs.rs/dateparser) disambiguates epochs by digit count: 10 digits
are seconds, 13 milliseconds, 19 nanoseconds, anything else fails. **Judgment:** digit count is
brittle for streams mixing units; test monotonicity and inter-arrival spread against wall clock
in addition to magnitude.

### 4d. State fields: nobody picks the column

Process mining takes the activity column as given: PM4Py's `discover_transition_system` builds a
state graph from a named activity attribute; Synoptic ([FSE 2011](https://github.com/ModelInference/synoptic))
infers a finite-state model from logs that already say which field is the event type.
`[no-record: a paper that selects which field is the status field — searched arXiv, ACM, BPM
proceedings 2026-09-28]`

**Judgment.** The detector is straightforward: for each low-cardinality field, group by entity
key, build the per-entity transition matrix; a state field has few distinct values, many
entities with more than one value over time, a sparse transition matrix, and high agreement on
first and last values across entities. The design page's cart example (`paid → shipped`) is this
shape. It is also the field the surprise feed watches for never-seen transitions (§5c).

### 4e. Keys and hubs survive hashing

[Rostin et al., WebDB 2009](https://hpi.de/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2009_rostin_a.pdf)
list ten foreign-key features; only two (ColumnName, TypicalNameSuffix) are name-based, and
Coverage and OutOfRange are among the most discriminative. Zhang et al. (VLDB 2010) add
*randomness*: foreign-key values should look like a random sample of the key domain. HoPF's
primary-key score averages cardinality, value length, position and name suffix
([Jiang & Naumann 2020](https://hpi.de/oldsite/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2020_jiang_holistic.pdf)).
**Judgment:** for the view, "hub-and-spoke" is a key whose values recur across many events and
are referenced by other keys. The world API already serves `hub` nodes with `in_degree` and
`by_kind` (decision 0006), so the graph form's precondition is measured, not inferred.

### 4f. Shape to default form: the rules that ship

Grafana suggests a time series when `hasFieldType(time) && hasFieldType(number) && rowCountTotal
>= 2`, a bar chart when `rowCountMax < 100`, a stat when one string field or ≤50 frames
([panel suggestions](https://grafana.com/developers/plugin-tools/how-to-guides/panel-plugins/add-suggestions-support)).
CompassQL's marks and channel scores are in §3a. Lux never plots id columns. kepler.gl creates a
point layer per lat/lon pair and an arc layer when it finds two pairs. Tableau's Show Me
(Mackinlay, Hanrahan, Stolte, TVCG 2007) states per-form minimums (a symbol map needs one geo
dimension; lines need a date and a measure) — the third-party enumeration is **UNVERIFIED**.

**Judgment.** This is the v0 view spec (§10): a table of `(shape facts) → (form, panels)` rules
that System 1 can apply with no LLM. It is also the baseline System 2 must beat at gate 3 — the
same "not a straw man" discipline research 0002 applied to H.

### 4g. Domain naming is unbenchmarked

Sato's LDA topic vector is the only classical values-only "what is this table about" signal, and
it feeds type prediction, not a label. [AutoDDG](https://arxiv.org/abs/2502.01050) (2025) has an
LLM write dataset descriptions from names, values and statistics. SemTab, GitTables and TURL score
entity and type matching, not domain naming. `[no-record: a benchmark scoring "name the domain
from values alone" — searched 2026-09-28]`. **Judgment:** gate 3's obfuscated stream *is* the
benchmark; publish the split.

---

## 5. Surprise: definitions, sketches and detectors that fit the budget

### 5a. Two surprises and one attention

**Shannon surprise** is −log p(x) under the current model; it scores the datum. **Bayesian
surprise** ([Itti & Baldi 2009](https://pmc.ncbi.nlm.nih.gov/articles/PMC2782645/); theory in
[Neural Networks 2010](https://pmc.ncbi.nlm.nih.gov/articles/PMC2860069/)) is
KL(P(M|D) ‖ P(M)): how far an observation moves the observer's beliefs, in "wows" (a two-fold
posterior change). Their contrast is the *white snow paradox*: static carries about 20× the
Shannon information of programme content and draws no attention, because a well-learned noise
model is not moved by more noise. Measured: 72% of human saccades landed on above-average-surprise
locations across 46,489 video frames. Their temporal surprise is the KL between Gamma posteriors
over a Poisson rate, updated with a forgetting factor ζ = 0.7 — a decayed conjugate rate model.

Pimentel et al.'s review ([Signal Processing 2014](https://www.robots.ox.ac.uk/~davidc/pubs/NDreview2014.pdf))
defines novelty detection as one-class modelling of "normal"; Carreño et al. (AI Review 2020,
**UNVERIFIED**) separate *outlier* (far from the sample), *anomaly* (deviates from a learned
normal), *novelty* (a new class that, once confirmed, joins normal). s2w#117's "a repeated
surprise becomes baseline" is the novelty definition.

Friston's free-energy account ([Nat. Rev. Neurosci. 2010](https://www.nature.com/articles/nrn2787))
casts attention as **precision weighting**: gain on prediction errors from channels currently
believed reliable. **Judgment:** this is the literal content of Dave's "attention heads?". It is
a scalar per baseline — sample count and variance — that multiplies −log p, so that a first
occurrence in a field with three prior observations does not rank. Not a learned attention
matrix; the metaphor is fair and the mechanism is simple.

**Judgment on the split.** Use −log p as the feed score (cheap, additive across fields, what
s2w#117 says) and Bayesian surprise as the retirement rule: when repeats stop moving the
posterior, the item stops appearing. Both come from the same Dirichlet (categorical) and Gamma
(rate) counters, and the KL between successive conjugate posteriors is closed-form and O(1).

### 5b. Bounded-memory sketches, with Rust

| Sketch | Source | Guarantee, memory | Rust (crates.io, checked 2026-09-28) |
|---|---|---|---|
| Streaming histogram | [Ben-Haim & Tom-Tov, JMLR 2010](https://www.jmlr.org/papers/v11/ben-haim10a.html) | B bins, merge closest; no formal bound | None; ~80 lines |
| t-digest | [Dunning & Ertl 2019](https://github.com/tdunning/t-digest) | Tail rank error in ppm; ~140 ns/add; mergeable | `tdigest` 1.0.1; in `datasketches` |
| DDSketch | [Masson et al., PVLDB 2019](https://arxiv.org/abs/1908.10693) | *Relative* error α on every quantile; α = 2% → 275 buckets ≈ 2 KB for 1 ms–1 min | `sketches-ddsketch` 0.4.1 (90M downloads) |
| KLL | [Karnin, Lang, Liberty 2016](https://arxiv.org/abs/1603.05346) | Optimal O((1/ε) log log(1/δ)) space | `kll-rs` 0.1.4; not yet in `datasketches` 0.5.0 |
| Count-Min | Cormode & Muthukrishnan 2005 | Overestimate ≤ εN w.p. 1−δ | `datasketches` |
| HyperLogLog | Flajolet et al. 2007 | 0.81% error at 12 KB | `hyperloglogplus`; `datasketches` |
| Exponential histogram | [Datar et al., SODA 2002](http://www-cs-students.stanford.edu/~datar/papers/sicomp_streams.pdf) | Sliding-window counts in O((1/ε) log² N) bits; basis of ADWIN2 | None; ~150 lines |

**Judgment.** For −log p you need a *density*, not a quantile: use DDSketch bucket counts as a
histogram (count/N), which keeps −log p finite and relative-error bounded. None of these decay
natively; the standard trick is rotating sketches or a periodic multiply of float counts.

### 5c. Never-seen values, transitions and types

A Bloom filter miss is a guaranteed first occurrence (`fastbloom` 0.17, 23.8M downloads); a
Count-Min count gives −log p̂ for rare-but-seen values. The p for a never-seen value is
Good–Turing's unseen mass P0 = N1/N, the fraction of observations that are singletons
([Gale & Sampson 1995](https://www.tandfonline.com/doi/abs/10.1080/09296179508590051)). It
behaves correctly: for an id field N1/N stays near 1 and a new value is unsurprising; for an enum
N1 → 0 and a new value is a large surprise (cap with a floor). Pitman–Yor
([Teh 2006](https://www.stats.ox.ac.uk/~teh/research/compling/acl2006.pdf)) gives
P(new) = (θ + d·K)/(n + θ), whose type growth is Heaps' law; a field whose distinct-count curve
departs from its fitted (θ, d) is itself a surprise ("this field started minting new values").
A never-seen state transition is the same machinery on the hashed (from, to) pair with a
Dirichlet row per source state.

### 5d. Rates, change points, cohorts, graphs

- **Rates.** A fading-factor Poisson–Gamma per (event type, hour-of-week) is 168 × 16 bytes per
  type; negative binomial when overdispersed. [Poisson-FOCuS](https://arxiv.org/abs/2208.01494)
  (2022) is a Poisson CUSUM optimal over all window lengths at about half the cost of a grid.
  EWMA control charts are the cheapest detector and a forgetting baseline at once.
- **Change points.** [ADWIN](https://www.cs.upc.edu/~gavalda/papers/adwin06.pdf) (Bifet & Gavaldà
  2007) drops the older half of a window when two sub-windows differ by a Hoeffding bound, so
  the user never chooses a window length; O(log W) memory in ADWIN2. Bayesian online change-point
  detection ([Adams & MacKay 2007](https://arxiv.org/abs/0710.3742)) runs a run-length posterior
  with conjugate predictives; Rust `changepoint` 0.15 implements `Bocpd`. Hinder et al.'s 2024
  unsupervised-drift survey ([Frontiers](https://www.frontiersin.org/journals/artificial-intelligence/articles/10.3389/frai.2024.1330257/full))
  warns that loss-based drift detection is the wrong tool when the target is monitoring for
  anomalous behaviour.
- **Cohorts.** Peer-group analysis (Bolton & Hand 2001; Weston et al. 2008) summarises an
  entity's k nearest peers per period and flags the entity whose distance jumps; the streaming
  version is a decayed mean and variance per cohort.
- **Graphs.** [MIDAS](https://arxiv.org/abs/1911.04464) (AAAI 2020) keeps two Count-Min sketches
  per edge (current tick, all-time) and a chi-squared score in constant time and memory; MIDAS-R
  decays and adds per-node sketches (the "new hub" case); **MIDAS-F stops updating the baseline
  while the score is above threshold** — a published answer to "should a surprise be learned?".
  No Rust crate; C++ reference exists.
- **Whole-detector families.** Half-Space Trees (IJCAI 2011), Robust Random Cut Forest
  (ICML 2016; Rust `krcf` 0.4) and HTM anomaly likelihood (Ahmad et al. 2017) are the general
  detectors; NAB's scoreboard puts HTM around 70 and RCF at 51.7 on its profiles (partly
  **UNVERIFIED**). The reusable idea from HTM is independent of HTM: normalize any detector's raw
  score by its own recent distribution before alerting. Python's River has all of these; no Rust
  equivalent exists.

**Budget check (judgment, arithmetic on cited numbers).** At 1,000 events/s × ~10 fields, a
DDSketch update is a log and an increment, t-digest ~140 ns, Bloom and Count-Min a few hashes,
MIDAS constant per edge: well under 100 µs per event single-threaded. Memory per event type: 2 KB
DDSketch plus 2.7 KB of Gammas plus a shared ~100 KB Count-Min; 1,000 types is 100–200 MB worst
case, tens of MB realistically. Inside decision 0004's envelope. ADWIN and BOCPD run only on a few
hundred derived rate series. The pieces with no crate — Good–Turing/Pitman–Yor novelty, ADWIN,
MIDAS — are each small.

### 5e. What products learned about surprise feeds

Datadog Watchdog requires ≥ 0.5 requests/s per endpoint and two weeks of history (six is
"optimal"), drops hit-rate anomalies with no latency or error impact, and suppresses seasonal
patterns ([docs](https://docs.datadoghq.com/watchdog/alerts/)). Dynatrace Davis alerts on "3
violating samples out of any 5 minutes", aggregates rather than high-cardinality dimensions, and
treats one or two alerts a month as healthy ([docs](https://docs.dynatrace.com/docs/dynatrace-intelligence/use-cases/avoid-overalerting)).
Uber's Argos was motivated by "hundreds of daily false alarms" from static thresholds (2015).
**Judgment:** four controls recur — minimum support, consequence filter, persistence (k of n), a
non-alerting tier. Show everything ranked; notify on nothing until it persists and crosses a
precision-weighted threshold.

---

## 6. Automated insight: what exists and why users distrust it

| System | Date | Insight definition | Ranking | What went wrong |
|---|---|---|---|---|
| [QuickInsights](https://www.microsoft.com/en-us/research/uploads/prod/2019/05/QuickInsights-camera-ready-compliant.pdf) (SIGMOD 2019; Power BI) | 2019 | `{subspace, breakdown, measure}` plus a pattern type | `f(impact) · g(significance)` | The paper devotes §3.2 to filtering *trivial* insights (functional dependencies produce "pre-determined relationships") and near-duplicates; Power BI caps output at 32 cards ([docs](https://learn.microsoft.com/en-us/power-bi/create-reports/service-insights)) |
| MetaInsight (SIGMOD 2021, **UNVERIFIED** detail) | 2021 | A *commonness* plus its *exceptions* | Conciseness, impact | Built because QuickInsights output was "disjointed" |
| [DataShot](https://www.microsoft.com/en-us/research/wp-content/uploads/2019/08/VIS2019_DataShot.pdf) (VIS 2019) | 2019 | `{context, breakdown, focus}` | significance × impact | — |
| [Voder](https://faculty.cc.gatech.edu/~john.stasko/papers/infovis18-voder.pdf) (InfoVis 2018) | 2018 | Data facts as widgets linked to charts | — | Contribution is linking each fact to its evidence chart |
| [Foresight](https://arxiv.org/abs/1707.03877) (VLDB 2017) | 2017 | "A strong manifestation of a statistical property" | Per-type top-k | **UNVERIFIED** |
| InsightPilot (EMNLP 2023) | 2023 | LLM issues typed analysis actions over the engines above | — | **UNVERIFIED** |
| [Snowy](https://arxiv.org/abs/2110.04323) (UIST 2021) | 2021 | Recommends *utterances*, not answers | Interestingness × language pragmatics | Two barriers named: not knowing what to consider, and discoverability |

Two definitional papers matter. [Law, Endert and Stasko (VIS 2020)](https://arxiv.org/abs/2008.13057)
interviewed 23 professional Tableau and Power BI users: insights are *actionable, unexpected,
trustworthy, interconnecting*; automation that "abbreviates the process of manual analysis" is
"difficult to trust"; tools assume "a highly-correlated scatterplot" is insightful and "this
assumption may not always hold true"; Quick Insights presents "general-purpose insights without
considering users' domains and work context". [Battle and Ottley (TVCG 2023)](https://arxiv.org/html/2206.04767v3)
formalize an insight as a *link between domain knowledge (outside the data) and analytic
knowledge (in the data)*.

Commercially, Tableau Pulse scores facts by impact on the metric and re-ranks from thumbs up and
down ([docs](https://help.tableau.com/current/online/en-us/pulse_insights_platform_insight_types.htm));
GA4's insights need two weeks of hourly or 90 days of daily history and cap custom insights at 50
([docs](https://support.google.com/analytics/answer/9517187)); ThoughtSpot SpotIQ ranks by usage
and feedback. `[no-record: an independent evaluation of Pulse, SpotIQ, Watchdog or Quick Insights
— searched 2026-09-28]`.

**Judgment.** Every ranking function in this table is impact × significance. Law et al. say users
mean *actionable and unexpected to me*. Battle and Ottley say the surprise feed by itself yields
analytic knowledge, not insight; the domain link is what System 2 adds when it phrases a surprise
as a question, and the person's acceptance is what makes it an insight. That is hypothesis 3
stated in the literature's terms.

---

## 7. False discovery, and why a graded question fixes it

- [Zgraggen, Zhao, Zeleznik, Kraska, CHI 2018](https://cs.brown.edu/research/ptc/assets/publications/zgraggeninvestigating.pdf):
  28 participants, synthetic data with known ground truth; "over 60% of user reported insights
  were wrong"; confirming on the same data left 11% false at α = 0.05; confirming on a held-out
  dataset removed the inflation. Their sentence: without a validation dataset or accounting for
  all comparisons, "we have no guarantees on the bounds of the expected number of false
  discoveries."
- [Zhao et al., SIGMOD 2017](https://arxiv.org/abs/1612.01040): every visualization the user
  looks at is an implicit hypothesis test; 100 correlations at α = 0.05 with 10 real and power 0.8
  yield about 13 "discoveries", 5 of them false; α-investing spends a budget per look.
- [Gelman and Loken 2013](https://sites.stat.columbia.edu/gelman/research/unpublished/p_hacking.pdf):
  the problem exists without fishing, whenever the choice of test is data-contingent; only a
  pre-chosen test, or one drawn from a pre-registered set, has a valid p-value.
- LLMs as hypothesis generators: AI-generated research ideas rated more novel than experts'
  (49 writers, 79 blind reviewers), but after deduplication at cosine 0.8 only about 5% of a
  large generated pool was non-duplicate, and LLM self-evaluation failed
  ([Si, Yang, Hashimoto, ICLR 2025](https://arxiv.org/abs/2409.04109)). On
  [InsightBench](https://arxiv.org/abs/2407.06423) an agent recovers 0.60 of expert insights with
  a specific goal and 0.40 with the generic goal "find interesting trends". On
  [InsightEval](https://arxiv.org/html/2511.22884) (2025) agents score F1 0.24–0.33 with precision
  above recall — "limited breadth". Chart summaries from GPT-4, Claude 3 and GPT-4o contained 199
  hallucinated sentences in 1,083 (18%) ([ChartInsighter](https://arxiv.org/abs/2501.09349),
  **UNVERIFIED**). [HypoGeniC](https://arxiv.org/abs/2404.04326) (2024) is the closest published
  analogue to hypothesis 3: propose hypotheses, then update them bandit-style with predictive
  accuracy as the reward.
- Forecasting as grounding. [Halawi et al. (NeurIPS 2024)](https://arxiv.org/abs/2402.18563): a
  retrieval-augmented GPT-4 system reached Brier 0.179 against the crowd's 0.149 on 914
  post-cutoff questions, and matched the crowd (0.238 vs 0.240) on the selective subset. An LLM
  pipeline generated 1,499 forecasting questions of which about 96% were verifiable and
  unambiguous, with 95% accurate automatic resolution ([arXiv 2601.22444](https://arxiv.org/abs/2601.22444),
  2026); its motivation was that auto-generation from recurring data "produces highly correlated
  or relatively trivial questions". Snowflake's Cortex Analyst warns its LLM-suggested questions
  "may not always be answerable" ([docs](https://docs.snowflake.com/en/user-guide/snowflake-cortex/cortex-analyst/suggested-questions-feature)).

**Judgment.** An LLM reading a surprise feed and choosing which surprise to narrate is at step
three or four of Gelman's ladder. Freezing the narrated pattern as a forecast about future events
is Zgraggen's held-out confirmation with the future as the validation set: the hypothesis exists
before the grading data do, so the forking paths collapse to one. The graded record then estimates
the *generator's* false-discovery rate, which no static insight system has. Three rules follow,
and each maps to a measured failure above:

1. **Freeze the question text before grading data exist** (Zgraggen, Gelman). The LLM must not
   rephrase a question after seeing early results; version it, grade the accepted version.
2. **Grade out of sample** — the forecast ledger (gate 4, contract A4's censoring rules).
3. **Deduplicate against the live question set** (Si et al.'s 5%, InsightEval's breadth, the
   2026 paper's "correlated or trivial"), and **tell System 2 what the user cares about**
   (InsightBench's 0.60 vs 0.40 — goal specificity is the largest lever measured).

What makes a proposed question gradable, distilled from Metaculus's guidelines (**UNVERIFIED**;
the page returned 403), Good Judgment Project practice and the 2026 paper: a named metric with a
fixed computation over the world; a population frozen at proposal time; a resolution window; a
threshold or base rate stated before the window; a resolution source (the ledger); an explicit
"annulled" outcome if the source disappears. A question that fails a machine check on any of
these is not shown to the person (Snowflake's caveat, made a rule).

---

## 8. Readiness: when has the stream said enough for an opinion?

Added for Dave's 06:02 message. The proposed split (karpathy): System 1 publishes domain-free
readiness signals (a decaying rate of new fields and shapes, distinct-count convergence, label
choice stability); System 2 decides when to author the view spec, or says "not yet" and names
what it is waiting for, triggered by stability rather than every N events; until then the view
shows a "learning" state.

### 8a. Schema-inference convergence: the tools guess a number and the bug trackers pay

BigQuery's schema auto-detect scans up to the first 500 rows of one file, and fields absent from
those rows fail later loads ([docs](https://docs.cloud.google.com/bigquery/docs/schema-detect)).
DuckDB's `read_json` samples 20,480 objects; issue #25786 (opened 2026-09-16) records that a
value past the sample that does not fit the inferred type raises for strings but *silently
coerces* booleans and numerics ([duckdb#25786](https://github.com/duckdb/duckdb/issues/25786)).
PyArrow infers on the first block only and freezes ([docs](https://arrow.apache.org/docs/python/json.html)).
Snowflake's `INFER_SCHEMA` warns the type "will depend on the number of rows parsed"
([docs](https://docs.snowflake.com/en/sql-reference/functions/infer_schema)). The academic line —
[Baazizi et al., VLDBJ 2019](https://link.springer.com/article/10.1007/s00778-018-0532-7);
[Klettke et al., BTW 2015](https://btw-2015.informatik.uni-hamburg.de/res/proceedings/Hauptband/Wiss/Klettke-Schema_Extraction_and_Stru.pdf),
who treat a rarely-present property as a structural *outlier* rather than schema — publishes no
records-to-convergence curve. `[no-record: a measured "records until the inferred JSON schema
stops changing" curve — searched 2026-09-28]`

The domain-free estimators exist elsewhere. Treat field paths (and, per field, value shapes) as
species: **Good–Turing** N1/N is the probability the next event carries a never-seen path
(§5c); **Chao1** = S_obs + f1²/(2·f2) is a lower bound on total richness from singleton and
doubleton counts (Chao 1984). Heaps' law says vocabulary never plateaus, it decelerates as a
power law — so "no new fields" is never literally true, and the criterion must be a rate.

### 8b. Distinct counts: the classification converges before the count does

[Charikar et al., PODS 2000](https://dl.acm.org/doi/10.1145/335168.335230) prove that any
estimator examining r of n rows has ratio error at least √((n−r)/(2r) · ln(1/δ)) on some input
(constant **UNVERIFIED** from the primary PDF); with a 1% sample the guaranteed error is about
7×. [Haas et al., VLDB 1995](https://www.vldb.org/conf/1995/P311.PDF) found no estimator good
everywhere. **Judgment:** the view does not need the cardinality, it needs the *class* —
identifier, categorical, numeric — and the singleton ratio f1/n separates those long before the
count settles: near 1 for ids, toward 0 for enums. A field that never converges is an identifier,
which is a readiness *fact*, not a failure.

### 8c. Anytime-valid inference: the fix for peeking

Wald's SPRT (1945) made sample size data-dependent. Confidence sequences are valid uniformly over
all n, so peeking at every event costs nothing
([Howard, Ramdas, McAuliffe, Sekhon, Ann. Stat. 2021](https://projecteuclid.org/journals/annals-of-statistics/volume-49/issue-2/Time-uniform-nonparametric-nonasymptotic-confidence-sequences/10.1214/20-AOS1991.full));
e-values and e-processes preserve error control under optional continuation
([Grünwald, de Heide, Koolen, JRSS-B 2024](https://academic.oup.com/jrsssb/article/86/5/1091/7706165);
[Ramdas et al., Stat. Sci. 2023](https://projecteuclid.org/journals/statistical-science/volume-38/issue-4/Game-Theoretic-Statistics-and-Safe-Anytime-Valid-Inference/10.1214/23-STS894.full)).
Optimizely deployed always-valid p-values because users "endogenously choose sample sizes by
continuously monitoring" ([Johari et al., Operations Research 2022](https://arxiv.org/abs/1512.04922)).

**Judgment.** "Stable after 500 events" is exactly the optional-stopping error these papers exist
to fix, and s2w's readiness signals are read continuously by definition. The primitive for "no new
field is appearing" is a running Bernoulli e-process for "this event introduced a new path";
the learning state ends when the time-uniform upper bound on that probability falls below a
tolerance (say 0.5%) *and* Chao1 slack is under one field. No fixed N; a coverage guarantee
regardless of when System 2 looks.

### 8d. Label and argmax stability

"The label field has not changed for M events" is a fixed window with no guarantee. Two
principled replacements: **stability selection** ([Meinshausen & Bühlmann 2010](https://rss.onlinelibrary.wiley.com/doi/full/10.1111/j.1467-9868.2010.00740.x))
— recompute the label-field score on B subsamples and require the argmax to win with frequency
≥ 0.8; or **racing** ([Maron & Moore, NIPS 1993](https://proceedings.neurips.cc/paper/1993/hash/02a32ad2669e6fe298e607fe7cc0e1a0-Abstract.html);
[Even-Dar et al., JMLR 2006](https://jmlr.org/papers/v7/evendar06a.html)) — keep a confidence
interval on each candidate's score and declare the leader settled when the runner-up's upper bound
is below the leader's lower bound. Racing yields the "not yet" reason for free: "`title` and
`name` are still tied."

### 8e. Stopping rules from active learning

[Bloodgood and Vijay-Shanker, CoNLL 2009](https://aclanthology.org/W09-1107/) stop when Cohen's
κ between successive models' predictions on a fixed *unlabelled* stop set exceeds 0.99 for three
consecutive rounds (thresholds **UNVERIFIED** against the primary text); the follow-up bounds the
F-measure change by 4(1−T)/T. Their second finding: stopping must be *user-adjustable*; no single
threshold suits every user. **Judgment:** translate directly. Hold a fixed stop set of buffered
events; after each batch re-run System 1's view-relevant classifications (field classes, label
choice, dimension/measure roles); κ ≥ 0.99 for three batches is "predictions have stabilized",
with a published bound. Expose the threshold.

### 8f. How products say "not yet", and how LLMs fail to

Watchdog needs 24 hours of logs and two weeks of metrics; Datadog's anomaly monitor needs three
seasons of history and shows no bounds during the first ones
([docs](https://docs.datadoghq.com/monitors/types/anomaly/)); Dynatrace's default reference
period is 7 days, with error-rate alerting after a service has run 20% of a week
([docs](https://docs.dynatrace.com/docs/platform/davis-ai/anomaly-detection/concepts/automated-multidimensional-baselining));
Grafana ML detects weekly seasonality only with two weeks of data; NAB's probationary period is
15% of the file, capped at 750 records ([util.py](https://github.com/numenta/NAB/blob/master/nab/util.py)).
**Judgment:** every mature product defines "enough" as coverage of the expected *period*, one to
three seasons, not an event count. On an unknown stream the period is unknown but detectable
(autocorrelation of arrival times), and "not yet" can be phrased as "have not seen one full cycle
of the dominant period."

On the LLM side, [AbstentionBench (NeurIPS 2025)](https://arxiv.org/abs/2506.09038) finds
abstention "unsolved" across 20 frontier models, with reasoning-tuned models abstaining about 24%
less than their base models. [CLAM](https://arxiv.org/abs/2212.07769) decomposes into
detect-ambiguity → ask → answer. **Judgment:** a reasoning-heavy System 2 will tend to author
early. Enforce abstention structurally: System 2 returns a typed
`{ready: bool, waiting_for: [{signal, current, target}]}` where every `waiting_for` item names a
System 1 readiness signal and its threshold. The renderer shows that list as the learning state —
progress against named criteria, NAB and Datadog style — not a spinner.

### 8g. Re-opening on drift

Two triggers, two responses. **Soft:** ADWIN or BOCPD on the readiness statistics themselves
(new-path rate, label-field score) re-arms the learning state. **Hard:** a categorical schema
event — a new path whose support crosses the outlier threshold, a path whose presence rate falls
below it, a value-shape change — goes straight to System 2 with the diff. The observability
products treat schema change as categorical: Bigeye reports additions, removals and fundamental
type changes but not precision changes ([docs](https://docs.bigeye.com/docs/schema-change-detection));
Debezium emits schema-change events with binlog positions; Confluent's Schema Registry names the
compatibility classes ([docs](https://docs.confluent.io/platform/current/schema-registry/fundamentals/schema-evolution.html)).
**Judgment:** borrow the vocabulary to size the response. An added optional field is
backward-compatible: append a panel, keep the spec version. A removed or retyped field the spec
depends on is breaking: re-author. Both land in the log as view-spec events, so the scrubber shows
when the dashboard changed and why.

---

## 9. The agent sees what the person sees

**What the model sees, by protocol.** MCP Apps: `content` is "text representation for model
context"; `structuredContent` is "structured data optimized for UI rendering (not added to model
context)"; a tool's `visibility` can hide UI-only tools from the agent
([spec 2026-01-26](https://github.com/modelcontextprotocol/ext-apps/blob/main/specification/2026-01-26/apps.mdx)).
OpenAI Apps SDK: "only `structuredContent` and `content` appear in the conversation transcript";
`_meta` reaches the widget "without exposing the data to the model"; `widgetState` persists UI
state between renders ([reference](https://developers.openai.com/apps-sdk/reference)). In every
case the model never perceives the rendered UI.

**How observability servers expose a dashboard.** [mcp-grafana](https://github.com/grafana/mcp-grafana)
offers a dashboard four ways: full JSON (`get_dashboard_by_uid`, with a `version` parameter for a
historical save), a summary (title, panel count, panel types, variables), a JSONPath property
extract "to reduce context window consumption", and `get_dashboard_panel_queries`. A rendered PNG
exists (`get_panel_image`) and needs a renderer service; the guidance is to avoid the full JSON
unless needed and to prefer patches for edits. Datadog's MCP server (preview, 200+ tools) exposes
dashboards as definitions with widget validation and carries attention-shaped tools
(`search_datadog_monitors`, `apm_search_watchdog_stories`) ([docs](https://docs.datadoghq.com/mcp_server/tools/)).
Grafana Assistant is itself reachable as a remote MCP server and produces scheduled digests
("all alerts that fired yesterday") ([blog 2026-04-21](https://grafana.com/blog/grafana-assistant-everywhere/)).

**Pixels are a poor channel for numbers.** On label-free charts, GPT-4.1, Gemini 2.5 Flash and
GLM-4.5V extract values with 6–7.5% adaptive MAPE despite near-perfect format success; the authors
conclude the model "cannot serve as a dependable extractor without human oversight"
([CHI 2026](https://arxiv.org/abs/2606.29808)). Models understand a chart's declarative structure
(93–97% on layer attribution) far better than its rendered geometry
([arXiv 2609.08657](https://arxiv.org/abs/2609.08657)). A deterministic Vega-Lite spec score
correlates 0.65 with human judgment against 0.71 for a vision-LLM judge, "a cost-effective
alternative when specifications are available" ([VegaChat 2026](https://arxiv.org/html/2601.15385v1)).
The caveat is real: for some table tasks images beat text
([arXiv 2402.12424](https://arxiv.org/html/2402.12424v5)). Playwright's MCP defaults to the
accessibility tree over screenshots ("text only", "deterministic") and recommends screenshots only
for "canvas apps, charts, image-heavy layouts" ([docs](https://playwright.dev/mcp/snapshots)).

**Judgment.** Three things follow.

1. **Serve the spec and the panel results, not pixels.** The agent reads the same view-spec event
   the renderer draws and runs the same panel queries. "The agent explains the dashboard" then
   holds by construction, not by a screenshot round-trip. Keep an image path only for "does this
   look wrong" questions.
2. **Tier the reads like Grafana.** `world.describe` already is the summary tier; add
   `view.spec(version?)` (full), `view.panel(id)` (one panel's query and current result) and
   `view.changes(since)` (spec diffs). The full spec is the exception, the panel the norm.
3. **The novelty is the shared definition.** An accessibility tree is a semantic view model
   derived *after* a human UI is built, lossily. s2w inverts it: spec first, renderer and agent
   downstream. No protocol or product ships that (`[no-record]`, Verdict 7). Name it in the design
   page and the launch: the agent and the person are looking at the same event.

`[no-record: a published measurement of how agents behave with a curated "what changed" digest
versus raw tool access — searched 2026-09-28]`. If s2w measures it (agent task success with and
without `attention()`), that is publishable.

---

## 10. Recommended first slice: co-developing the view with the core

Sequenced against decision 0017's table. Each step ships its three surfaces.

**Before gate 3 — view spec v0, derived, not authored.**

1. **Schema.** A flat map of id'd nodes (§2): `{version, derived_by: system1|system2, form,
   primary: [entity_type], panels: {id: {kind, query, params, label, icon, role}}, layout,
   readiness}`. Forms: `graph | map | timeline | state-flow | table`. Icons and roles are enums
   shipped with the renderer. Every `query` is a registered query over `s2w-app::query` with
   typed parameters; no free text reaches the fold. JSON Schema inside the strict-mode subset.
2. **Shape facts from System 1** (§4): per-field class (`id | categorical | state | measure |
   time | geo:latlon | text`), per-type hub degree (already served), relationship kinds, event
   rates. Two new detectors: value-based lat/lon pairs (§4b) and state fields (§4d). Stored as
   world-profile facts, so the spec's constraints can reference them.
3. **Rules → v0 spec.** A `(shape facts) → (form, panels)` table in the CompassQL/Grafana style
   (§4f): geo pair → map with a graph panel; state field → state-flow with rates per transition;
   otherwise graph with type legend, active-now and hubs (s2w#114's content becomes the default
   panels). This is the heuristics arm System 2 must beat at gate 3.
4. **Constraints** (§3a): each form's preconditions as hard checks; a spec that fails is
   rejected with a reason string. Runs on every spec, derived or authored.
5. **The spec is an event.** Appended to the log with its version and `derived_by`; the fold
   exposes "the current spec at offset N"; the scrubber shows the dashboard as understood then
   (decision 0017 §2). Revocation is an event too.
6. **Readiness** (§8): System 1 publishes `new_path_rate` (e-process upper bound), `chao1_slack`,
   per-field class settledness (f1/n separation), `label_race` (leader, runner-up, gap). The view
   shows a learning state listing these against thresholds until the v0 spec's preconditions are
   met. Thresholds are configuration, not constants.
7. **Agent surface.** `view.spec`, `view.panel`, `view.changes`, and readiness in
   `world.describe`. Same JSON the renderer consumes (decision 0009's identical-response rule).
8. **Obfuscation check.** The v0 spec on the plain and obfuscated Wikipedia streams must agree on
   form and panel set. Fitness function.

**Gate 3 — System 2 authors and revises.**

9. System 2 receives the shape facts, the readiness signals, a sample, and the v0 spec, and
   returns either `{ready: false, waiting_for: [...]}` (§8f) or a spec that passes the same
   constraints. One repair round on rejection; then fall back to v0. Its spec is a new event
   with `derived_by: system2` and the model version. The obfuscated copy must yield the same form.
10. **Measure** at gate 3, beside the identity metrics: form agreement plain vs obfuscated; the
    fraction of authored specs rejected by constraints (VisEval's 21% is the number to beat);
    and a small human preference test, v0 versus authored, on the three gate-3 streams.

**Gate 3 → 4 — baseline and surprise.**

11. Fading-factor Dirichlet and Gamma counters per (type, field), (type, transition), (type,
    hour-of-week); Good–Turing novelty per field; DDSketch densities for measures; MIDAS-style
    per-edge sketches for new hubs (§5). Surprise = −log p × precision weight; retirement when
    the posterior stops moving; MIDAS-F's rule of not learning while surprised. A surprise panel in
    the spec and `attention()` over MCP, ranked, with the four noise controls of §5e.

**Gate 4 — surprises become graded questions.**

12. System 2 reads `attention()` and the user's stated goals and proposes questions that pass a
    machine resolvability check (§7): named metric, frozen population, window, threshold,
    resolution source, annulment. Accepted questions are frozen text, registered forecasts in
    the ledger, deduplicated against live questions. The generator's own false-discovery rate is
    reported from the ledger.

**Judgment on scope.** Steps 1–8 are a sprint-sized slice and need no LLM. They give the demo
labels, forms and a learning state on day one, and they give gate 3 a baseline that is not a straw
man. Step 9 is the first place "System 2 is where opinions live" is tested against a measured
alternative.

---

## Open questions

1. **Who owns the readiness thresholds?** §8e says they must be user-adjustable; the contract
   freezes gate-3 measurements. Are the defaults part of the frozen arm, or configuration outside
   it? (Dave decides; affects B1.)
2. **Is "same form on the obfuscated copy" a gate-3 pass condition or a reported measurement?**
   Adding it to B1 changes a frozen contract; reporting it does not.
3. **How much of the world's *values* does System 2 see when authoring?** Names are hashed on the
   obfuscated copy, but values are not (research 0002 §1). If System 2 reads sample values, the
   split-screen claim narrows to "not memorizing field names." The demo line should say which.
4. **Layout: does the spec carry it, or does the renderer decide from form and panel count?**
   Grafana's v2 lesson says separate them; A2UI leaves layout to the client catalog.
5. **What does the person state as goals, and where does it live?** InsightBench's 0.60 vs 0.40
   makes this the largest lever on question quality, and nothing in the current design captures
   it. A world-profile field the inbox can edit is the smallest answer.
6. **One spec per world, or per audience?** An agent may want denser panels than a person.
   Decision 0009's identical-response rule says one; the token budget (Grafana's summary tier)
   says the *read* is tiered, not the spec.
7. **Precision weighting needs a prior on how many observations make a baseline trustworthy.**
   Fixed? Learned from how often early surprises retired? (Affects s2w#117.)

---

## Sources not reachable in full text

Metaculus question-writing guidelines (403); Charikar et al. PODS 2000 PDF (403; constant taken
from secondary sources); MetaInsight (MSR PDF had an empty text layer); Setlur & Stone TVCG 2016
(PDF unparseable); Bloodgood & Vijay-Shanker thresholds (abstract only); Draco 2 constraint count
(not in the paper text; check the repo's `.lp` files before quoting a number).

---

## Design implications

Each is a candidate; karpathy assigns the disposition.

1. **View spec v0 schema and renderer, derived by System 1 rules before gate 3** (§10 steps 1–5;
   the flat id'd-node shape, bindings not values, layout separated). → candidate: adopt as a
   decision record (0018, view spec) and a sized issue under s2w#116; the renderer stays generic
   per decision 0017.
2. **Two new System 1 shape detectors, value-based lat/lon pairs and state fields** (§4b, §4d),
   both unpublished. → candidate: adopt under s2w#114/#116; also paper candidates for s2w#1.
3. **Hard constraints per form, rejection with reason, one repair round, then v0 fallback; no
   silent server-side repair** (§3a). → candidate: adopt in the view-spec decision record.
4. **Icons and colors as enums; the LLM never emits a hex value or a free icon name** (§1c). →
   candidate: adopt; the design page's solid/dashed/ochre grammar becomes the role set.
5. **Readiness as anytime-valid signals, System 2 returns typed `{ready, waiting_for}`, the view
   shows a learning state against named criteria; thresholds are configuration** (§8). →
   candidate: adopt under s2w#116; open question 1 to Dave before the gate-3 freeze.
6. **Re-open on drift: ADWIN/BOCPD on readiness statistics (soft) and categorical schema events
   (hard), classified backward-compatible vs breaking** (§8g). → candidate: adopt in the same
   record; the schema-event detector is a System 1 item.
7. **Form agreement plain vs obfuscated as a gate-3 fitness function** (§3c, §10 step 8). →
   candidate: adopt as a fitness function now; whether it enters B1 is open question 2 (a contract
   amendment, Dave decides).
8. **Surprise: −log p × precision weight from fading conjugate counters, Bayesian-surprise
   retirement, MIDAS-F "do not learn while surprised", the four product noise controls** (§5). →
   candidate: adopt in a decision record for s2w#117; the Rust gaps (Good–Turing/Pitman–Yor,
   ADWIN, MIDAS) are small implementation issues.
9. **Question hygiene: frozen text, out-of-sample grading, deduplication, machine resolvability
   check, user goals as an input** (§7). → candidate: adopt in the gate-4 epic s2w#14; the
   generator's false-discovery rate is a reported gate-4 number.
10. **Agent surface: `view.spec`, `view.panel`, `view.changes`; spec and results, not pixels;
    reads tiered like Grafana** (§9). → candidate: adopt under s2w#115 with the v0 spec, so the
    agent surface ships with the view (decision 0017's three-surface rule).
11. **Name the novelty**: one view definition, stored as an event in the domain log, served to
    renderer and agent alike; no protocol or product does this as of 2026-09-28 (`[no-record]`).
    → candidate: adopt in the design page's "LLMs as clients" section and the launch framing;
    paper candidate for s2w#1.
12. **Research 0002's semantic-type work covers §4a; this note adds nothing there.** → candidate:
    rejected as a new item (already in H).
13. **A measured comparison of agent task success with and without `attention()`** (§9). →
    candidate: deferred to a post-slice issue, not before gate 4's ledger exists.
