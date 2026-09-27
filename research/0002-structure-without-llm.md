# 0002: Discovering structure without an LLM

- **Question:** how far can data profiling, key and dependency discovery, and semantic type
  detection go at recovering entities, identity rules, relationships and state fields from a
  schemaless JSON event stream, with no LLM, when field names and identifier values may be
  obfuscated? What should the heuristics arm **H** be?
- **Date:** 2026-09-27 · **Researcher:** sagan (with four research subagents) · **Status:** done
- **Feeds:** gate 3 (contract Part B). The arm H must be strong, or H+S2 wins against a straw man
  and the gate proves nothing.

Conventions as in the index: every claim cites the page used; **UNVERIFIED** means seen only in a
search snippet or abstract, not a fetched page. Facts and my judgments are kept apart; judgments
are labelled **Judgment**.

---

## Verdict

1. **Everything H needs exists as published method; nothing exists as a Rust library.** Key
   discovery, inclusion dependencies (INDs), foreign-key ranking, case-id detection and
   lifecycle discovery are each well studied (§2–§5). No Rust crate implements UCC, FD or IND
   discovery as of 2026-09-27 (§7). The sketch layer (HyperLogLog, heavy hitters, Bloom,
   quantiles, HyperMinHash) exists in Rust and is maintained. H is an integration job, not a
   research job.
2. **Obfuscation removes names and value text, and nothing else that H needs.** A keyed hash is
   a bijection inside a domain, so counts, equality, repetition, set containment, co-occurrence
   and per-entity sequences survive exactly. A constant time shift keeps order and gaps. Every
   published method that fails under obfuscation fails because it reads names or value text
   (Sherlock, Sato, Doduo, TURL, the name features in FK ranking). The value-statistics methods
   are unaffected; the one measured case shows it (HoPF Table 7, §4).
3. **Naming is free under the gate-3 metric.** Types are aligned by Hungarian assignment and the
   obfuscated stream is "scored up to renaming" ([contract B3](../docs/evaluation-contract.md)).
   So knowing that a cluster is "page" rather than "user", which is what an LLM is best at, earns
   nothing on the obfuscated stream. System 2 can only win there on **structure**: composite
   keys, alias fields, hashed free text that looks like an identifier, low-support event types,
   and censored references (§6).
4. **Judgment: H may score above 0.90 on Wikipedia identity.** Wikipedia's identifiers are clean,
   repeated and co-referenced; the signatures in §3 separate them well. If so, the contract's
   0.10 margin is unreachable and gate 3 is judged on the safety floors, the 0.60 floor and B3
   alone (contract B4, last paragraph). Build H first and measure it on the development window
   before anyone builds System 2, so this is known early. This is unmeasured.
5. **Three signals appear unpublished, as far as these searches reached.** The distinct-count
   growth curve as a role signal, per-value inter-arrival burstiness as identity evidence, and
   the *carry-over pair* (field X in an entity's event t equals field Y in its event t−1). Each is
   cheap and survives obfuscation. They are candidate contributions for the paper, not claims
   yet (§8).

---

## 1. What obfuscation leaves intact

The obfuscated stream (contract B2): field names become `f1…fN`; identifier values become a keyed
hash of (domain, value); identifiers inside URLs and titles are extracted and hashed the same way;
other names and free text are hashed; timestamps shift by one secret constant; other numbers and
the event structure are unchanged.

| Signal | Survives? | Why |
|---|---|---|
| Field names, name embeddings | No | renamed |
| Value text: character classes, word embeddings, regex parses of ids | No | hashed to fixed-length hex |
| Distinct counts, uniqueness ratio, frequency shape, constancy | Yes | bijection inside a domain |
| Set containment between fields (INDs), co-occurrence in one event | Yes | same value, same hash, within a domain |
| Per-entity event sequences, repetition, burstiness | Yes | grouping by a hashed key gives the same groups |
| Order and gaps of timestamps | Yes | constant shift |
| Numeric order of hashed ids (e.g. `revision.new > revision.old`) | **No** | hashing destroys order |
| Unhashed numbers (lengths, namespace, Kafka offsets) | Yes | unchanged by rule |
| JSON structure: nesting, presence per event, arrays | Yes | unchanged by rule |

The published feature families sort the same way. Sherlock's 1,588 features split into character
distributions (960), word embeddings (200), paragraph vectors (400) and 27 global statistics; its
own ablation gives F1 0.78, 0.79, 0.73 and **0.25** for each group alone
([arXiv 1905.10688](https://ar5iv.labs.arxiv.org/html/1905.10688)). Only the statistics group
survives hashing, and it is the weakest alone because Sherlock's 78 labels are semantic types
(city, sales), not roles. Roles are a much easier target: seven classes, most of them defined by
statistics in the first place.

No published paper measures a column-type model on hashed values (searched arXiv and the web with
hashed, anonymized, pseudonymized, tokenized and masked, 2026-09-27). The only measured
obfuscation effect is HoPF's removal of column **names** (§4).

---

## 2. Profiling algorithms: what runs one pass on a stream

Survey reference: Abedjan, Golab, Naumann, "Profiling relational data: a survey", VLDB J 2015
([PDF](https://hpi.de/oldsite/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2015_abedjan_profiling.pdf)).
It splits profiling into single-column (cardinalities, distributions, patterns), multi-column, and
dependencies (keys, INDs, FDs). Its Table 1 already lists the role signals H needs: uniqueness
(distinct/rows), constancy (frequency of the top value/rows), first digit for Benford, and a
"data class" (code, indicator, text, date/time, quantity, identifier).

| Task | Algorithm | One pass, bounded memory? | Source |
|---|---|---|---|
| Per-path count, nulls, types, min/max | counters | yes, exact | survey §3 |
| Distinct count, uniqueness | HyperLogLog | yes, about 1–2% error | [JSONoid](https://arxiv.org/pdf/2307.03113) |
| Quantiles | KLL, t-digest, DDSketch | yes, approximate | [datasketches](https://crates.io/crates/datasketches) |
| Single-field key | HLL(X) ≈ count | yes, but cannot certify zero duplicates | JSONoid §6.2 |
| Multi-field keys (UCCs) | HCA, DUCC, HyUCC, HPIValid | no; position-list indexes over the data | [DUCC](http://www.vldb.org/pvldb/vol7/p301-heise.pdf), [HyUCC](https://dl.gi.de/bitstreams/c752bd2b-ab55-4a3b-a14a-7a4c4ef5b913/download), [HPIValid](http://www.vldb.org/pvldb/vol13/p2270-birnick.pdf) |
| Refuting a key or FD | reservoir sample, pairs that agree | yes, exact refutation only | HyUCC and HyFD sampling phase |
| Exact FDs | TANE, DFD, FDep, HyFD, FDHits | no | [7-algorithm evaluation](http://www.vldb.org/pvldb/vol8/p1082-papenbrock.pdf), [HyFD](https://hpi.de/oldsite/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2016_papenbrock_a.pdf) |
| Testing a named FD's confidence | reservoir plus Count-Min | yes, additive error | [Cormode et al. SIGMOD 2009](https://people.cs.umass.edu/~mcgregor/papers/09-sigmod.pdf) |
| Exact INDs | SPIDER, BINDER, SINDY | no; sort or disk buckets | [SPIDER](https://hpi.de/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2007_bauckmann_efficiently.pdf), [BINDER](http://www.vldb.org/pvldb/vol8/p774-papenbrock.pdf) |
| Approximate INDs | Faida (HLL union test), MANY (Bloom signatures) | sketch phase yes; verification needs values | [Faida](https://dl.gi.de/bitstreams/0e772c5e-6967-4cae-9edb-23f75bf62237/download), [MANY](https://hpi.de/oldsite/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2017_tschirschnitz_detecting.pdf) |
| JSON structure plus keys and FKs | JSONoid (monoid per path: HLL, Bloom, histograms) | yes; FK suggestion is O(paths²) over Bloom filters | [arXiv 2307.03113](https://arxiv.org/pdf/2307.03113) |
| Nested INDs and FDs over JSONPath | Mior 2021, dynamic unrolling | no; data in memory, unary INDs only | [arXiv 2111.10398](https://arxiv.org/pdf/2111.10398) |

Scale facts that decide H's design:

- **Batch exact discovery is affordable at gate-3 sizes.** HyFD found all FDs of 1,024,000 rows ×
  19 columns in 97 s single-threaded; its authors warn that above 50 columns and 10 million rows
  it may run for days ([HyFD](https://hpi.de/oldsite/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2016_papenbrock_a.pdf)).
  HPIValid keeps UCC discovery on 7.5 M rows under 13 GB ([PVLDB 2020](http://www.vldb.org/pvldb/vol13/p2270-birnick.pdf)).
- **Candidate explosion, not row count, is the real limit.** INDs among web tables grew from
  122 k at 1 k tables to 259 M at 50 k tables, mostly "scrap" (empty columns, integer sequences)
  ([MANY Table 3](https://hpi.de/oldsite/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2017_tschirschnitz_detecting.pdf)).
  H should restrict candidates to fields already classified as identifier-like, and to arity 1
  and 2.
- **HLL inclusion–exclusion is a poor containment estimator exactly where foreign keys live.**
  Its error scales with the union, so a small set inside a large one is noisy and the estimate
  can go negative ([Ertl, arXiv 1702.01284](https://arxiv.org/abs/1702.01284);
  [HyperMinHash, arXiv 1710.08436](https://arxiv.org/abs/1710.08436)). Faida avoids this by
  testing the equality |s(Y)| = |s(X) ∪ s(Y)| rather than estimating a ratio.

**Judgment: use exact 64-bit hash sets, not sketches, for gate 3.** A development window of
10^5–10^6 events has at most a few million distinct identifier values per field, which is tens of
megabytes. Exact sets remove sketch error from the comparison. Keep HyperMinHash or Bloom
filters as the fallback past a per-field cap, for the product's unbounded case.

---

## 3. Role signatures that survive obfuscation

Synthesized from the sources in the last column. Thresholds are starting points to tune on the
development window, not published values unless cited.

| Role | Signature | Sources |
|---|---|---|
| **Event id** | Uniqueness 1.0 (or 1.0 in combination with another field); distinct count grows with slope 1; never the referenced side of an IND | "grouping ratio" 0 for event ids ([Andaloussi et al. 2018](https://www.alexandria.unisg.ch/bitstreams/3f624d5a-6933-4f2c-b2fa-6b498d5f984b/download)) |
| **Entity id** | Nominal; distinct count keeps growing but below the event count; values repeat; no dominant value; bursty repeats; referenced by other fields, or the key other fields vary under | Nezhad et al.'s three tests: nominal, many distinct values, repeated ([UNSW TR 0709](https://cgi.cse.unsw.edu.au/~reports/papers/0709.pdf)); `fr.unique`, `fm.unique` ([Toyoda et al.](https://arxiv.org/abs/2301.12829)); Greg Young's event store ([CQRS Documents](https://cqrs.wordpress.com/wp-content/uploads/2010/11/cqrs_documents.pdf)) |
| **Reference** | Value set contained in another identifier field's set; coverage of that set at least about 10% | Rostin: about 60% of true FKs cover all key values, none below 10% ([WebDB 2009](https://hpi.de/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2009_rostin_a.pdf)) |
| **Category** | Distinct count saturates early; high constancy; skewed frequencies | ptype-cat uses only distinct count U, U/N and clean variants, 0.93 overall ([arXiv 2111.11956](https://ar5iv.labs.arxiv.org/html/2111.11956)) |
| **Flag** | Exactly 2 distinct values | AutoGluon rule ([docs](https://auto.gluon.ai/0.4.1/tutorials/tabular_prediction/tabular-feature-engineering.html)) |
| **Timestamp** | Near-unique with ties; non-decreasing along the stream; magnitude of epoch seconds or milliseconds; first digit fixed | tie observation (Andaloussi); magnitude and unit rules ([launchpad #74](https://github.com/Basekick-Labs/launchpad/issues/74)) |
| **Measure** | Many values, not monotone, not unique, parses as a number, Benford-like first digits | survey Table 1 (first digit) |
| **State** | Varies within an entity; small domain or a carry-over pair; sparse, directional transitions | constancy test: a case attribute is "constant in 90% of the cases" ([López-Pintado et al.](https://arxiv.org/html/2408.13666)); state-change events ([SA-OCPM](https://www.alessandroberti.it/new_papers/2025_Dina_SAOCPM.pdf)) |
| **Free text** | Words per value above 1, high type-token ratio, high length variance | AutoGluon; Sherlock `avg_word_cells` |

Two cases need cross-field evidence because single-field statistics cannot separate them:

- **Hashed free text looks like an identifier.** Once `comment` is hashed, it is fixed length and
  nearly unique. It differs from an identifier only across fields: it participates in no IND and
  groups no carry-over chain. This separation is my judgment; I found no source that tests it.
- **Hashed identifiers lose order.** The plain-stream rule "`revision.new` > `revision.old`" does
  not survive. The equality chain does: `revision.old` of an edit equals `revision.new` of that
  page's previous edit (§5).

---

## 4. Keys, relationships and cardinality

**Foreign-key ranking features that use no names.** Rostin et al. train a classifier on ten
features of each IND A ⊆ B ([PDF](https://hpi.de/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2009_rostin_a.pdf)).
Seven survive obfuscation: distinct dependent values, coverage, how often A is itself referenced,
multi-dependent, multi-referenced, out-of-range share (only on unhashed numbers), and table-size
ratio. Column-name similarity, value-length difference and the `id` suffix do not. J48 reached
F-measure above 80%, often close to 100%, with all ten.

**The one measurement of removing names.** HoPF's Table 7 reports F1 with and without column
names ([PDF](https://hpi.de/oldsite/fileadmin/user_upload/fachgebiete/naumann/publications/PDFs/2020_jiang_holistic.pdf)):
TPC-H 0.88 → 0.88; TPC-E 0.80 → 0.72; AdventureWorks 0.46 → 0.39; MusicBrainz 0.69 → 0.36. The
Randomness method, which never reads names, is unchanged in every row. The lesson for H: names
help most where several fields share one value domain (MusicBrainz), and value statistics alone
are enough where domains are distinct. Keyed hashing by domain makes domains more distinct, not
less.

**What does not transfer.** Zhang et al.'s Randomness test and HoPF's distribution score compare
histograms of the two sides ([PVLDB 2010](https://dl.acm.org/doi/10.14778/1920841.1920944)).
Hashes are uniform, so every pair looks random. These tests become uninformative under
obfuscation, and H should not use them there.

**Cardinality of a relationship.** For two identifier fields A and B seen in the same events,
count distinct B per value of A and distinct A per value of B. At most one both ways is 1:1, one
way is N:1, otherwise N:M. This is the rule in Rebmann et al.'s public code
(`discover_case_object`: one-to-n, n-to-one, N-to-M; [repo](https://github.com/a-rebmann/object-information-extraction))
and matches de Murillas's root, regular and converging nodes
([KAIS 2020](https://link.springer.com/article/10.1007/s10115-019-01430-6)). A 1:1 pair with equal
distinct counts is an **alias**: two encodings of the same entity. The contract requires every
mention in any field to be typed, so H must merge aliases into one type.

**Composite keys.** A field can identify an entity only together with another. In
`recentchange`, `id` (rcid) repeats across wikis, so the event key is (`wiki`, `id`); the
subagent saw iowiki and Wikidata rcids side by side in a 25-second live sample, and inferred the
collision from the rule (MediaWiki rcids are per-wiki; [Manual:Recentchanges table](https://www.mediawiki.org/wiki/Manual:Recentchanges_table)).
Debezium solves the same problem by adding a table identifier to the key when several tables
share a topic, because a primary key "is unique within only that table"
([topic routing](https://debezium.io/documentation/reference/stable/transformations/topic-routing.html)).
Popova, Fahland and Dumas define an entity as a set of event tables that share a primary key and
find keys by FD discovery ([arXiv 1303.2554](https://arxiv.org/abs/1303.2554)).

**Judgment: the false-merge floor makes composite keys the most important rule in H.** A merge
of two pages that share a title across wikis is a false merge, and the contract caps H+S2's
false-merge rate at 0.05 per replicate. H should refine a candidate key K to (K, P) whenever K's
groups contain more than one value of a field P that is otherwise constant within K's
carry-over chains. Whether this case exists on the obfuscated stream depends on an open question
about the hash (§9).

---

## 5. Entities, lifecycles and state from event logs

Process mining has solved the offline version of this problem on flat logs; the stream version
reuses the same statistics.

| Work | What it computes | Obfuscation-proof part |
|---|---|---|
| OCEL 2.0 ([arXiv 2403.01975](https://arxiv.org/abs/2403.01975)) | Target model: events, objects with types, event-object and object-object relations with qualifiers; object attributes stored as one row per change | Target shape for H's mapping |
| Nezhad et al. ([UNSW TR](https://cgi.cse.unsw.edu.au/~reports/papers/0709.pdf); [VLDB J 2011](https://link.springer.com/article/10.1007/s00778-010-0203-9)) | Candidate identifiers by distinct ratio d/n between two thresholds; key-based rules (same value) and reference-based rules (chain to a previous message); composite rules in a lattice | All of it |
| Toyoda et al. ([arXiv 2301.12829](https://arxiv.org/abs/2301.12829)) | Nine column features, classifiers for case id, timestamp, activity | Only 2 of 9 features: unique ratio and mean count per value |
| Rebmann et al., BPM 2022 ([code](https://github.com/a-rebmann/object-information-extraction)) | BERT tags attribute names, then uniqueness across cases ≥ 0.9 and cardinality rules | Uniqueness and cardinality only; the name tagging does not survive |
| Popova, Fahland, Dumas ([arXiv 1303.2554](https://arxiv.org/abs/1303.2554)) | Event-type tables, keys by FD discovery, entities as tables sharing keys, lifecycle per entity | All of it |
| Esser and Fahland ([arXiv 2005.14552](https://arxiv.org/abs/2005.14552)) | Event knowledge graph: correlate events to entities, derive relations, directly-follows per entity | All of it; identifiers were chosen by hand |
| SA-OCPM ([PDF](https://www.alessandroberti.it/new_papers/2025_Dina_SAOCPM.pdf)) | State-change events where an object's attribute value at t differs from t−ε | All of it |
| Greg Young, Kafka, Debezium | The aggregate id is present on every event, repeats, and orders a version that increases by one ([CQRS Documents](https://cqrs.wordpress.com/wp-content/uploads/2010/11/cqrs_documents.pdf); [Kafka intro](https://kafka.apache.org/intro); [Debezium](https://debezium.io/documentation/reference/stable/connectors/postgresql.html)) | All of it |

**The carry-over pair.** Group events by a candidate entity key. If field X in the entity's event
at t equals field Y in its event at t−1 in most consecutive pairs, then (Y, X) is a before/after
pair of one state variable, and the key is confirmed as the entity that owns it. In
`recentchange` this finds both `length.old`/`length.new` (unhashed numbers) and
`revision.old`/`revision.new` (hashed, order lost, equality kept). It needs one last-value map
per candidate key. I found no paper that uses this as a discovery rule (§8); it restates the
Greg Young version check and the SA-OCPM state-change construction.

**Wikipedia `recentchange`, the roles H should recover.** From the schema
([current.yaml](https://schema.wikimedia.org/repositories/primary/jsonschema/mediawiki/recentchange/current.yaml)),
the common fragment ([current.yaml](https://schema.wikimedia.org/repositories/primary/jsonschema/fragment/common/current.yaml)),
and a 25-second live sample of 1,068 events taken by a subagent on 2026-09-27:

| Field | Role |
|---|---|
| `meta.id` | event id, unique |
| `id` (rcid) | event id per wiki; key is (`wiki`, `id`) |
| `type` | category: edit, new, log, categorize, external |
| `wiki`, `server_name`, `server_url`, `server_script_path`, `meta.domain` | one wiki entity, several aliases |
| `title` (+ `namespace`, `wiki`) | page entity |
| `title_url`, `notify_url` | page and revision identifiers inside URLs; in live events, **not in the schema** |
| `user` | user entity |
| `revision.old` → `revision.new` | revision entity and a carry-over pair; `new` events carry only `revision.new` |
| `length.old` → `length.new` | numeric state, carry-over pair |
| `timestamp`, `meta.dt` | event time and receipt time |
| `bot`, `minor`, `patrolled` | flags; `patrolled` only where the wiki supports patrolling |
| `comment`, `parsedcomment` | free text; an alias pair |
| `log_id`, `log_type`, `log_action`, `log_params`, `log_action_comment` | only when `type=log`; `log_params` is array, object or string |

Two schema facts worth a line in the answer key: the YAML parenthetical labels for
`revision.old` and `revision.new` are swapped relative to MediaWiki's column definitions, though
the live data is correct (`revision.new > revision.old`); and live events carried
`$schema: /mediawiki/recentchange/1.0.0` while the registry's current file is 1.0.1. Canary events
(`meta.domain == "canary"`) must be filtered
([EventStreams](https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams)).

**Conditional presence reveals the event-type field.** The `log_*` paths appear only when
`type=log`. A category field whose value predicts which paths are present is the event-type
discriminator, and H should profile each event type separately after finding it. This follows
from the schema; it is my synthesis, not a published rule.

---

## 6. The design for H

Seven stages. Stages 1 to 5 are one pass over the development window; 6 and 7 are small
computations over the profile.

1. **Flatten.** One column per JSONPath; array elements streamed as a set of values, not a cross
   product (Mior's dynamic unrolling, 37–210× faster than flattening,
   [arXiv 2111.10398](https://arxiv.org/pdf/2111.10398)). Record which paths each event carries.
2. **Profile each path.** Count, presence rate, JSON type mix, exact distinct set up to a cap,
   top values (Filtered Space-Saving), string length statistics, numeric quantiles, share of
   events non-decreasing against the previous event, distinct count at 10^3, 10^4 and 10^5
   events (the growth curve), first-digit histogram, and burstiness of repeats for sampled values.
3. **Find the event-type field** by conditional presence, then profile each event type.
4. **Classify roles** with the signatures in §3, scored, with an abstain band.
5. **Relate identifiers.** For identifier-like pairs only: containment both ways, co-occurrence
   cardinality, aliases (1:1 with equal distinct counts), composite-key refinement, and
   carry-over pairs per candidate key.
6. **Assemble types.** An entity type is an alias class of identifier paths, joined across event
   types by containment. Relationships come from co-occurrence cardinality and containment
   direction. State fields are those that vary within an entity (fail the 90% constancy test) or
   form a carry-over pair.
7. **Emit the mapping** in the contract's schema, with abstentions where scores fall in the grey
   band.

**Where local embeddings fit.** The contract defines H as "System 1 rules and local embeddings".
Embeddings help only where names are readable: the plain stream and the private stream. There,
split `snake_case` and camelCase before embedding; pre-trained models otherwise tokenize
identifiers poorly (practice reported in [arXiv 2507.14376](https://arxiv.org/pdf/2507.14376) and
[arXiv 2305.17378](https://arxiv.org/pdf/2305.17378), both **UNVERIFIED** direct quotes). On the
obfuscated stream, names carry no text, so the name embedder should abstain rather than guess.
**Judgment:** this means the obfuscated stream tests statistics-only H, and the private stream
tests statistics plus names. Say so in the gate-3 report.

**Where H is likely to lose points** (judgment; each is where System 2 could honestly win):

| Weak spot | Why | Mitigation in H |
|---|---|---|
| References censored by the window | `revision.old` usually points to an edit before the window opened, so global containment is low | Use the carry-over chain instead of containment; measure containment only for values first seen after warm-up |
| Composite keys | Page titles and rcids may collide across wikis | The refinement rule in §4 |
| Hashed free text | Looks like an identifier | Require an IND or a carry-over chain before calling a field an entity id |
| Rare event types | `log` subtypes have few events, so statistics are thin | Abstain below a minimum support |
| Aliases with different hashes | `wiki` and `server_name` hash differently | 1:1 alias detection |
| `meta.offset` | An unhashed monotone integer can pass as a timestamp | Step-size check: offsets increase by small integers |

---

## 7. Rust building blocks

All versions from crates.io on 2026-09-27.

| Need | Crate | Notes |
|---|---|---|
| Distinct counts | [`cardinality-estimator`](https://crates.io/crates/cardinality-estimator) 1.0.3 (Cloudflare) | HLL++ with a small-set representation (8 bytes for 2 elements), merge, serde feature |
| One vendor for several sketches | [`datasketches`](https://crates.io/crates/datasketches) 0.5.0 (Apache) | HLL, CPC, Theta, Bloom, Count-Min, frequent items, REQ, t-digest; binary format tested against Java and C++ |
| Top values | [`topk`](https://crates.io/crates/topk) 0.7.0 | Filtered Space-Saving, merge, serde |
| Containment past the cap | [`hyperminhash`](https://crates.io/crates/hyperminhash) 0.2.5 | intersection and cardinality from one LogLog-size sketch; small project (8.7k downloads) |
| Membership | [`fastbloom`](https://crates.io/crates/fastbloom) 0.17.0 | serde feature |
| Quantiles | [`sketches-ddsketch`](https://crates.io/crates/sketches-ddsketch) 0.4.1 | relative error, merge |
| Name embeddings | [`model2vec-rs`](https://crates.io/crates/model2vec-rs) 0.3.0 | static embeddings, no ONNX; about 8,000 strings/s single-threaded per its README; potion-base-8M is about 8 MB |
| Value embeddings, if ever | [`fastembed`](https://crates.io/crates/fastembed) 7.1.0 | depends on `ort` 2.0.0-rc.13, still a release candidate |
| JSON stream | `serde_json` `StreamDeserializer`; `sonic-rs` or `simd-json` if parsing dominates | |
| Name matching after splitting | [`textdistance`](https://crates.io/crates/textdistance) 1.1.1 | token Overlap and Jaccard |
| UCC, FD or IND discovery | **none** | as of 2026-09-27 |

`[no-record: Rust UCC/FD/IND discovery crate, as of 2026-09-27; searched=GitHub repository search (9 queries, language:rust), crates.io search (5 queries), web search]`.
Reference implementations to read: Metanome (Java,
[algorithms](https://github.com/HPI-Information-Systems/metanome-algorithms)), Desbordante (C++,
[repo](https://github.com/Desbordante/desbordante-core)), and JSONoid (Scala,
[repo](https://github.com/dataunitylab/jsonoid-discovery)), the closest existing design to H.
`genson-rs` is a dormant but readable Rust schema-inference reference
([repo](https://github.com/junyu-w/genson-rs)).

---

## 8. Gaps in the literature

Each is a negative finding as of 2026-09-27, from the subagents' searches of arXiv, ACM,
Springer, GitHub and the web. Treat each as "not found", not "does not exist".

- **Column-type detection under hashed values:** no measurement found. Sherlock's robustness
  section covers missing values and format noise only ([sherlock.media.mit.edu](http://sherlock.media.mit.edu/)).
- **Distinct-count growth curve as a role signal:** no paper found. The parts exist separately:
  Heaps' law, the Zipf–Heaps relation ([arXiv 1412.4577](https://arxiv.org/abs/1412.4577)), and
  skew-dependent distinct-value estimation ([Haas et al., VLDB 1995](https://www.vldb.org/conf/1995/P311.PDF)).
  A patent excerpt says a field with linearly growing distinct strings is an ID (**UNVERIFIED**,
  [USPTO 12105712](https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/12105712)).
- **Inter-arrival burstiness as identity evidence:** no paper found. Correlation Miner uses time
  gaps between activity pairs, not per-value repeats ([Pourmirza et al.](https://pure.tue.nl/ws/files/61131933/Pourmirza2017.pdf)).
- **Carry-over pairs as a discovery rule:** no paper found.
- **One-pass approximate FD by distinct counts** (|X| = |X ∪ A| as TANE's partition test with
  sketches): no paper found.

---

## 9. Open questions for the contract

These change what H can observe. They are questions, not edits; the contract's owner decides.

1. **Is a page title hashed as (page, title) or as (page, wiki, title)?** If the former, titles
   collide across wikis and composite-key discovery is part of the test. If the latter, the
   collision is removed by the obfuscation itself. The answer key should say which.
2. **Are `revision.old` and `revision.new` one identifier domain?** The carry-over chain survives
   only if they are.
3. **Is `id` (rcid) an identifier domain, or "other numbers"?** If it stays numeric, it keeps its
   order and its per-wiki collisions.
4. **Are aliases (`wiki`, `server_name`, `server_url`, `meta.domain`) one entity in the answer
   key?** H will merge them; the key should agree or mark them ambiguous.

---

## Sources not reachable in full text

Marked **UNVERIFIED** where used: Swan's speedups, DynFD internals, Baazizi et al.'s JSON schema
inference mechanism, the Data Profiling book's chapter bodies, CORDS, Rebmann et al.'s paper
body (the code was read instead), De Fazio et al. 2023, the 2026 "simple heuristic for the case
ID attribute" paper, Zhang et al. 2010's full text, and SortingHat's names-off ablation.

## Design implications

1. H is an integration of published methods; §6's seven-stage design, exact hash sets for
   containment, and abstaining name embeddings on the obfuscated stream.
   → decided: [decision 0007](../docs/decisions/0007-gate3-h-arm.md) (2026-09-27); H-min build
   and measurement: stream2worlds#56
2. Build H first and measure it on the development window before building System 2.
   → adopted: gate-3 epic stream2worlds#13 sequencing (2026-09-27)
3. H may exceed 0.90 identity F1 on Wikipedia, making B4's 0.10 margin unreachable.
   → deferred: stream2worlds#4 (measure first; if true, Dave re-decides the margin before the freeze)
4. The four contract questions in §9. → deferred: stream2worlds#17
5. Three signals that appear unpublished (distinct-count growth, burstiness, carry-over pairs).
   → deferred: stream2worlds#1 (paper candidates, not claims)

