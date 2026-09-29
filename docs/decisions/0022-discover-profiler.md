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

1. **Decode and flatten.** A root string field that parses as a JSON object in ≥99% of the
   ≥20 events carrying it becomes a decode step. Objects flatten by key; arrays are skipped
   (a v0 key path cannot address them). Strings, `i64`s and bools are keyable; floats and
   nulls are not. Payloads that are not JSON objects are counted and skipped.
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
   it passes.
5. **Aliases.** Two entity paths equal in ≥99% of the ≥20 events carrying both are one class.
6. **1:1 merge.** Classes that determine each other (both directions functional, same 95% / 5
   group rule, ≥20 shared events) are two encodings of one entity (research 0002 §4) and merge.
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

**Ids and labels.** A rule id or attribute name is the path's segments joined by `.`, with `\`
and `.` escaped by `\`, so distinct paths never share an id. A type label is the sorted,
distinct `parent/leaf` tails of the class's paths joined by `+` (with `\`, `/` and `+`
escaped); when two classes would share a label, both use their full paths. `rule_id` and
`type_labels` are public so the replay check can re-derive them.

**Determinism.** `BTreeMap` only, no floats in any decision or output, no tie broken by a name.
Output rules are sorted by id; the replay canonicalizes the same way.

## The obfuscation-invariance contract

Renaming every key and hashing every string leaf (inside decoded strings too; integers and bools
pass through, as in check 11's obfuscator) must change the proposed mapping only by the same
renaming: same types, same rules, same relationships and kinds, ids, labels and attribute names
re-derived from the renamed paths. A unit test checks this on a synthetic stream whose renaming
reverses key sort order; a profiler that drops attributes by name fails it (mutation-checked).
Check 12 in `cargo xtask check` (#163 PR 3b) will assert it on the recorded Wikipedia fixture,
read through a neutral-named symlink so the vocabulary scan stays clean.

## Measured on the recorded fixture

`crates/s2w-sources/testdata/wikipedia-page-change.raw.sse` (1,615 events, recorded
2026-09-27), parsed to the stored `{"data":…,"id":…}` envelope: decode `data`; event-type field
`data.page_change_kind`; **12 types and 143 relationship rules** (74 `n:1`, 69 `n:m`). The first
run, before plan review's fixes to the dependency denominator, the grey band, the 1:1 merge and
relationship endpoints, gave 17 types and 318 relationship rules. The types include the page
(`page_id`, aliased across the event key), editor and performer ids (aliased), the wiki, the
Wikidata item, and revision and content hashes (repeated when one revision appears in several
events). Composite keys would split per-wiki ids; they are out of scope below.

**Claim volume.** 143 relationship rules is a lot of claims per window. PR 4 measures what
applying the mapping costs and decides whether to prune (for example, relationships between
aliases of one pair of types) before auto-apply.

## Out of scope

Composite keys, carry-over of identity across events, inclusion dependencies, embeddings, a
learned scorer and the evaluation corpus (#56's H-min and H-full). Wiring into `serve` and
auto-apply (#163 PR 4) and the mapping state surfaces [0017](0017-view-and-agents-first-class.md) requires (#163
PR 5).
