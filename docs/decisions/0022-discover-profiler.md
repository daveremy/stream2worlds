# 0022: `s2w-discover` — an H-lite profiler that proposes a stream mapping or abstains

Date: 2026-09-28 · Status: accepted · Gate 3 · Issue #163 (PR 3a of 5) · Amends [0001](0001-workspace-layers.md) (layers) · Builds on [0018](0018-no-compiled-domain-code.md), [0021](0021-stream-mapping-v0.md), [research 0002](../../research/0002-structure-without-llm.md)

## Decision

[0021](0021-stream-mapping-v0.md) made the stream mapping data. This record adds the first
producer of that data that needs no person and no LLM: a heuristic profiler, research 0002's
H-lite slice (§6 stages 1 to 4, plus per-event value-equality aliases and co-occurrence
relationships). It lives in a new pure crate, `s2w-discover`, and exposes one function:

```rust
pub fn discover(payloads: &[&[u8]], cfg: &Config) -> (Profile, Discovery)
```

`payloads` are the log's stored bytes, in stream order. `Discovery` is either a `StreamMapping`
that passed `validate()` or `Abstain(reason)`. `Profile` reports every path's count, distinct
values and role, including each abstention, plus the decode steps and the event-type field.

The profiler reads statistics, never meaning. Every decision rests on value equality, presence
and stream order, never on a key's name or a value's text, so it is bound by the same rule as
every engine since [0018](0018-no-compiled-domain-code.md). Type labels and attribute names are
built from the key names the stream carries, as data, until System 2 names types (Dave's
ruling on #163: labels come from field paths).

## The stages and their thresholds

Percentages are whole percent on integer ratios, rounded down. All are `Config` fields.

1. **Decode and flatten.** A root string field that parses as a JSON object in every one of
   the ≥20 events carrying it becomes a decode step (the executor abstains on a whole event
   whose decode fails, so a partial decode would silently drop events). Objects flatten by key; arrays are skipped
   (a v0 key path cannot address them). Strings, `i64`s and bools are keyable; floats and
   nulls are not, and a path that is ever null or a float is never a key or an attribute. Payloads that are not JSON objects are counted and skipped.
2. **Per-path statistics.** Count, distinct values, and recurrence: the share of repeats whose
   previous occurrence was not the immediately preceding carrier.
3. **Event-type field** (reported only): the always-present path with 2 to 32 values whose
   values best explain which optional paths an event carries (present in ≤2% or ≥98% of each
   value's events). A tie reports none.
4. **Roles, with abstain bands.** Fewer than 20 carriers: `Sparse`. Non-keyable or mixed kinds:
   `Other`. One value: `Constant`; two: `Flag`. Distinct/count ≥98%: `EventId` (research 0002
   §3: unique per event names the event, not an entity). Below 90% with recurrence <10%:
   `Sequence`. Everything else takes the **functional-dependency test**: over every one of the
   candidate's repeated values (all repeat groups are the denominator; a dependent missing or
   carried once in a group counts against it), the share of groups where another path is
   constant. Only informative dependents count (distinct values across constant groups ≥ half
   the constant groups). Best share ≥95%: `Entity`; 80–95%: `GreyDependency` (abstain); lower:
   `NoDependents`. Fewer than 5 repeat groups: `FewGroups`. A path at 90–98% uniqueness gets the
   test too, since uniqueness falls as the window grows; it abstains as `GreyUniqueness` unless
   it passes. *2026-09-29 (s2w#208): a path that passes at or above `type_uniqueness_pct` (90,
   equal to the grey band's floor) is `NearUnique` instead of `Entity`: it keys no type, so a
   grey-band path never does. See the amendment below.* *2026-09-29 (s2w#250): a path that
   passes below that line, has at most `category_max` values, and whose repeated values each decide
   which optional paths their events carry is `Category` instead of `Entity`: it keys no type. See
   the second amendment below.*
5. **Aliases.** Two entity paths equal in ≥99% of the ≥20 events carrying both are one class.
6. **1:1 merge.** Classes that determine each other, transitively (both directions functional, same 95% / 5
   group rule over the events carrying both, ≥20 of them) are two encodings of one entity (research 0002 §4) and merge.
   The key is chosen without names: most alias members, then most events, then most distinct
   values, then integer over string over bool. A tie keeps them apart. The losers' paths become
   attributes. A one-way dependency does not merge or demote.
7. **Assemble.** Each class is one type; each member path is one entity rule. Attributes are
   non-key paths passing the dependency test under that key. A relationship joins two types
   that co-occur in ≥20 events: `n:1` from the many side when exactly one direction is
   functional, otherwise `n:m` from the side with fewer distinct values, and none when the counts
   tie or both directions are functional. Only each class's most-carried members are named.
   No entity type: abstain. Fewer than 1,000 events: abstain (at 200 events, measured page ids
   sit at 97.5% uniqueness and read as event ids; at 1,615 they sit at 89%).

**Ids and labels.** A rule id or attribute name is the path's segments joined by `.`, decode
prefix included (the path must address the decoded value), with `\`
and `.` escaped by `\`, so distinct paths never share an id. A type label is the sorted,
distinct `parent/leaf` tails of the class's paths joined by `+` (with `\`, `/` and `+`
escaped); when two classes would share a label, both use their full paths. `rule_id` and
`type_labels` are public so the replay check can re-derive them.

Two thresholds are fixed rather than in `Config`: a dependent is informative when its distinct
values across constant groups are at least half the constant groups, and the event-type test
uses the 2% / 98% presence band.

**Determinism.** `BTreeMap` only, no floats in any decision or output, no tie broken by a name.
Output rules are sorted by id, so their order follows the (renamed) names; the invariance
contract below holds up to rule order, and the replay canonicalizes it.

## The obfuscation-invariance contract

Renaming every key and hashing every string leaf (inside decoded strings too; integers and bools
pass through, as in check 11's obfuscator) must change the proposed mapping only by the same
renaming: same types, same rules, same relationships and kinds, ids, labels and attribute names
re-derived from the renamed paths. A unit test checks this on a synthetic stream whose renaming
reverses key sort order; a profiler that drops attributes by name fails it (mutation-checked).
Check 12 in `cargo xtask check` (#163 PR 3b) asserts it on the recorded Wikipedia fixture,
read through the neutral-named link `crates/s2w-discover/testdata/recorded.raw.sse` so the
vocabulary scan stays clean, with check 11's key and value maps. It adds about 3 s to an
unoptimized `cargo xtask check` (two profiler passes over 1,615 events).

## Measured on the recorded fixture

`crates/s2w-sources/testdata/wikipedia-page-change.raw.sse` (1,615 events, recorded
2026-09-27), parsed to the stored `{"data":…,"id":…}` envelope: decode `data`; event-type field
`data.page_change_kind`; **12 types and 143 relationship rules** (78 `n:1`, 65 `n:m`) at
`PROFILER_VERSION` 1 (the amendment below gives the version 2 figures). The first
run, before plan review's fixes to the dependency denominator, the grey band, the 1:1 merge and
relationship endpoints, gave 17 types and 318 relationship rules. The types include the page
(`page_id`, aliased across the event key), editor and performer ids (aliased), the wiki, the
Wikidata item, and revision and content hashes (repeated when one revision appears in several
events). Composite keys would split per-wiki ids; they are out of scope below. Two measured
limits: the Wikidata item id and its concept URI determine each other but tie on every rank
field, so they stay two types with no relationship between them (a tie is never broken by
name); and `Sequence` does not fire on the fixture, because datacenter partitions interleave
timestamps, so `dt` fields fall to `NoDependents` through the dependency test instead.

**Claim volume.** 143 relationship rules is a lot of claims per window. PR 4 measures what
applying the mapping costs and decides whether to prune (for example, relationships between
aliases of one pair of types) before auto-apply. *2026-09-29: auto-apply shipped first (#197 PR 4a); the
measurement is #197 PR 4b.* *2026-09-29: the measurement found the world too large; the prune is the amendment below.*

## Amendment 2026-09-29: a near-unique key names no type (s2w#208, `PROFILER_VERSION` 2)

The learned mapping OOMed the demo box at 1 GiB inside 131k events (#197). Measured with #197
PR 4b's `discover_volume` test (10k-event window, fold to 10^5 events with fresh strings per
fixture cycle, the upper bound decision 0025 reads against ~350 MiB):

| lever | relationship rules | claims/event | entities | relationships | resident |
|---|---|---|---|---|---|
| none (`PROFILER_VERSION` 1) | 142 | 115.9 | 287,582 | 2,763,239 | 1,009.9 MiB |
| drop every `n:m` relationship | 65 | 65.6 | 287,582 | 1,124,257 | 685.3 MiB |
| **near-unique key names no type** | 94 | 75.1 | 97,633 | 916,641 | **252.0 MiB** |
| both | 39 | 38.3 | 97,633 | 333,931 | 175.8 MiB |

Three paths in the 10k window passed the entity test from inside the grey band (96–97% unique).
Each was a type of its own; nearly every event minted a new entity of each, carrying 15 to 18
attributes and about ten edges, so they made most of the world's growth. The rule: a path that
passes the dependency test with distinct/count at or above `Config::type_uniqueness_pct` (90) is
`Role::NearUnique`. It is reported in `Profile`, keys no type, and stays eligible as another
type's attribute. The statistic is the role stage's own integer ratio, so renaming keys and
hashing strings leaves it unchanged (check 12). This extends research 0002 §3's argument for
`EventId` into the grey band: a key that is new in nine events of ten names the event, not a
thing that recurs. The surviving entity paths in the 10k window top out at 87.5% unique.

Kept: `n:m` relationships, since the prune alone meets the gate. Not done: collapsing a type
pair's relationship rules to one endpoint pair. Alias members of one type share a natural key,
so those rules repeat claims but not world edges; they cost log volume, not memory.

On the 1,615-event fixture the mapping is now 5 types, 12 entity rules and 46 relationship rules
(32 `n:1`), down from 12 types and 143: in a short window more keys look near-unique. The window
serve profiles is 10,000 events.

*2026-09-29, measured (#197 PR 4b, `crates/s2w-app/tests/discover_volume.rs`, release build, hub).*
On the recorded fixture (11,667 events) with the production `Config`, the first 10,000 events
give 20 entity rules and 142 relationship rules, and the mapping applies at **115.9 claims per
event** (0 abstained). Folding 10^5 events: replaying the fixture as-is (later cycles re-observe
the same entities, a lower bound) gives 56,840 entities, 375,760 relationships and **110 MiB**
resident; making every cycle's strings new (an upper bound) gives 287,582 entities, 2,763,239
relationships and **1,010 MiB**. The ~350 MiB deploy line (#197 ruling) lies between the
bounds, so the measurement does not clear it. Window stability: the identities at 1k, 5k and
10k events each differ from the whole fixture's (23/215, 22/173 and 20/142 against 22/168
rules). Profiling 10,000 events takes 4.4 s. Both findings go to a follow-up issue: a
name-free prune and a window rule.

## Amendment 2026-09-29: a key whose values decide an event's shape names no type (s2w#250, `PROFILER_VERSION` 3)

Research [0009](../../research/0009-h-min-plain-wikipedia.md) found H-lite (`PROFILER_VERSION` 2)
keying `log_action`, the action name of a log event (18 values in the 10^4-event `dev` window),
as an entity type. It passes the dependency test honestly: `log_type` is constant under each of
its 15 repeat groups and takes 10 values across them, so it is an informative dependent. The
dependency is a taxonomy (action -> action family), not an entity's attribute.

What sets it apart on `dev`, measured with a throwaway probe (not committed) over the first 10^4
and the whole 2x10^5 events: among the events carrying the key, count the optional paths
(carried by at least 20 of them and by more than 2% and fewer than 98% of them), then count how
many of those every repeat group carries in at most 2% or at least 98% of its events.

| path | 10^4 events: explained / optional | 2x10^5 events | role at `PROFILER_VERSION` 3, 10^4 |
|---|---|---|---|
| `log_action` | 8 / 8 | 12 / 12 | `Category` (was `Entity`) |
| `log_type` | 8 / 8 | 11 / 12 | `NoDependents` (unchanged; never passed the dependency test) |
| `type` | 10 / 13 | 10 / 13 | `FewGroups` (unchanged) |
| `notify_url` | 1 / 6 | 1 / 6 | unchanged |
| `comment`, `parsedcomment` | 0 / 13 | 4 / 13 | unchanged |
| `meta.uri` | 0 / 13 | 1 / 13 | unchanged |
| `title`, `title_url`, `server_name`, `server_url`, `meta.domain`, `wiki`, `user`, `namespace` | 0 / 13 | 0 / 13 | unchanged |
| `log_params.img_timestamp` | 0 / 0 | 0 / 0 | unchanged (no optional path) |

The full v3 role table for the window is committed as
[`h-lite-v3.dev-10000.profile.md`](../../research/h-measure/results/h-lite-v3.dev-10000.profile.md),
printed by `cargo xtask h-measure profile --corpus dev --window 10000`; the explained/optional
counts above are the probe's, not that verb's.

**The rule.** In stage 4, a path that passes the dependency test below `type_uniqueness_pct` is
`Role::Category` instead of `Entity` when it has at most `Config::category_max` (32, stage 3's
bound) distinct values, at least one path is optional among the events carrying it (the
definition above), and every repeat group of the path carries every such optional path in at
most 2% or at least 98% of its events. `Category` is reported in `Profile`, keys no type, and
stays eligible as another type's attribute, like `NearUnique`. Only repeat groups count, since a
value seen once trivially explains every path. `pct` rounds down, so a group of 49 events with
one stray reads 2% and counts as pure, as in stage 3.

The statistic is stage 3's own presence test, applied to a candidate key over its own carriers:
presence and value equality only, integer percentages, no name and no string's text, so check 12
holds unchanged. Research 0002 §3's argument extends once more: a value that says what kind of
event this is classifies the event; it does not name a thing that recurs.

**Accepted false demotion.** A key with at most 32 values in the window, each of which fixes
which optional fields its events carry (for example up to 32 devices, each emitting one fixed
payload shape), reads as a category and keys no type; it stays an attribute. Without names it is
structurally the same as `log_action` -> `log_type`. The bound exists so that a many-valued key
of that kind (hundreds of devices, one shape each) keeps its type. A plain `distinct <= 32` rule
was rejected: a small fleet of sensors whose payloads do not depend on the sensor keeps its type.

**Not caught.** A closed vocabulary that does not change the event's shape (a status field with
no optional paths). None is an `Entity` on `dev`, so no rule for it is justified.

**Window dependence.** In the whole 2x10^5-event `dev` corpus `log_action` has 36 values, more
than 32, so the rule does not fire there and `log_action` stays an entity at that window.
`log_action_comment` (free text) is also an entity there; it is not in #250's list and is
recorded as a finding only. The window `serve` profiles and the frozen H-lite window is 10^4.

On the 1,615-event fixture the mapping is unchanged: 5 types, 12 entity rules, 46 relationship
rules (32 `n:1`), no `Category` path (re-measured 2026-09-29 on this change; the page-change
stream carries no log events).

Research 0009 implication 1 named H-min proper "`PROFILER_VERSION` 3"; that number is taken
here, so #244's H-min change takes `PROFILER_VERSION` 4.

## Out of scope

Composite keys, carry-over of identity across events, inclusion dependencies, embeddings, a
learned scorer and the evaluation corpus (#56's H-min and H-full). *2026-09-29: the evaluation corpus
exists and H-lite at `PROFILER_VERSION` 2 is measured on it (research
[0009](../../research/0009-h-min-plain-wikipedia.md)): on plain `recentchange` it proposes no
`user` or revision type, keys a small action-name field (`log_action`) as an entity, and keys
pages and wikis at alias paths. Inclusion dependencies are #244.* *2026-09-29 (s2w#250): `log_action` is a `Category` at
`PROFILER_VERSION` 3 (amendment above). A `user` type needs a new entity criterion (#250 PR 2);
a revision recurs only across two paths, an inclusion dependency (#244); choosing among alias
encodings of one entity needs a format that joins different values (#245).* Wiring into `serve` and
auto-apply (#163 PR 4; *2026-09-29: done, [decision 0025](0025-learned-mapping-auto-apply.md)*) and the mapping state surfaces [0017](0017-view-and-agents-first-class.md) requires (#163
PR 5).
