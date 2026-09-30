# 0027: Stream mapping links (format version 2)

Date: 2026-09-29 · Status: accepted (PR 1 of #245: the format) · Gate 3 · Issue #245 · Amends [0021](0021-stream-mapping-v0.md) (version 2), [0023](0023-routes-from-stored-mappings.md) (identity hashes the mapping's own version), [0005](0005-pure-fold.md) (a second producer of merges) · Builds on [0018](0018-no-compiled-domain-code.md)

## Context

A version-1 mapping (0021) keys an entity rule on the values at its key paths, and two rules
with one `type_label` join only when their values are equal. So no version-1 mapping can say
that **different** values name one entity: one thing written as a short code, a host name and
a URL stays three entities. research 0009 (#56) measured the cost: it caps recall for every
mapping producer (the heuristic profiler, System 2 and an operator alike), and a type whose
mentions sit at alias paths scores low recall even under the oracle mapping.

The fold can already join two keys: `WorldEvent::EntitiesMerged{survivor, absorbed}` and
`World::merge` (0005), with "first merge wins" and cycle-free union. What is missing is a way
for a mapping to state which rules co-name.

## Decision

**A version-2 mapping may carry `links`: pairs of entity rule ids,
`[{"survivor": "<rule id>", "absorbed": "<rule id>"}]`.** A link says "these two rules are two
encodings of one type". When both rules match in one payload and give different keys, the
executor claims `EntitiesMerged{survivor_key, absorbed_key}` and the existing fold does the
union. The mapping names two paths, never two values: which values join is learned from
co-occurrence in the stream. It adds no value tables and no domain data, and it is invariant
under obfuscation (0022), since only rule ids and co-occurrence matter.

### Link semantics

1. `survivor` and `absorbed` are ids of entity rules that share one `type_label`. A link
   across labels would merge types, and nothing needs that.
2. Per type the links form a star. `validate()` rejects:
   - a link on a version-1 mapping (`LinksNeedVersion`),
   - a link that names no entity rule (`UnknownLinkRule`),
   - a link from a rule to itself (`SelfLink`),
   - a link across type labels (`LinkAcrossLabels`),
   - a rule absorbed by more than one link, a repeated link included (`AbsorbedTwice`),
   - a rule that is both a survivor and absorbed (`SurvivorAbsorbed`).
3. Claim order per payload (the executor, #245 PR 2): entities in rule order, then merges in
   link order, then relationships in rule order. Merges come before relationships so this
   payload's edges bind to the survivor's entity, since the fold never rewrites an edge. A link
   whose two keys are equal claims nothing.
4. **First link wins, per absorbed key.** This is the fold's existing rule (0005), and it is the
   precision guard: an absorbed value shared by several survivors joins the first survivor it
   co-occurs with and never the rest. It never chains two survivors together, because a
   survivor is never absorbed. So the survivor must be the more specific encoding: the rule
   with more distinct values in the window, which determines the other. A producer derives the
   direction from that distinct-value count; a tie gives no link.
5. Deterministic under replay: the result depends only on stream order and values.
6. A version-2 mapping with no links is valid. Its identity differs from the same rules at
   version 1, which is correct: the identity names the version.

### Accepted costs

- **A merge is not retroactive** (0005). An edge or attribute observed on an absorbed key before
  its first co-occurring payload stays on the absorbed id.
- **An absorbed key and a survivor key share one label**, so an absorbed value textually equal
  to some survivor's value is that same entity. That is the natural-key rule (0021, rule 3)
  applied as written, and harmless when the two encodings never collide in text.
- **Every link claims one merge per matching payload, forever.** The fold no-ops the repeats,
  but the verdict log (0012) stores them. The producer that first emits links (#245 PR 4)
  measures the growth and the memory of one extra entity per absorbed key before auto-apply
  (0025) serves a version-2 mapping.

### Format and versions

- `StreamMapping` gains `links: Vec<LinkRule>`, last in declaration order, with
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]`. `LinkRule` is `deny_unknown_fields`.
- `MAPPING_VERSION` stays **1**: the base version, a mapping without links. The new
  `MAPPING_VERSION_LINKS = 2` is the version that adds links and the newest this build reads.
  `validate()` accepts both. *The #245 plan said "`MAPPING_VERSION = 2`"; keeping the base
  constant at 1 means every writer that states no link (the profiler, h-measure, the fixtures)
  still writes a version-1 mapping with the bytes and identity it wrote before this record, and
  no writer changed to keep it that way. A writer opts into version 2 by setting it.*
- An old build rejects a version-2 mapping, because `version` is checked and
  `deny_unknown_fields` applies. That is acceptable with one deployment.
- `KEY_FORMAT` is unchanged: links change which entities merge, never the key text.

### Identity (amends 0023)

`StreamMapping::identity()` hashes the mapping's **own** `version`, not a constant. For a
version-1 mapping that is the value it always hashed, and a version-1 mapping serializes byte
for byte as before (`links` is omitted when empty), so **every stored version-1 mapping keeps
its identity**: no verdict or snapshot is re-keyed and no world is rebuilt. The pinned fixture
identity (`mapping-6815cb3fc24b0848`) is unchanged and is the proof, with a model test that pins
a version-1 mapping's bytes and identity as computed before this record. A version-2 mapping is
a new identity, pinned by a known-answer test.

### Shape: a top-level `links` vector, not `absorbed_by` on the rule

The alternative was `absorbed_by: Option<rule id>` on `EntityRule`, which makes "absorbed at
most once" and "no repeated link" true by construction. The vector wins on three counts:

- **A link is a peer of a relationship**: a pair of rule ids. The profiler obfuscation replay
  (check 12) already remaps and sorts relationship `from`/`to`, and a link takes the same two
  lines of remap and one sort; `absorbed_by` would add a remap inside every entity rule.
- **`EntityRule` keeps its schema.** Every consumer of a rule (the profiler's per-rule output,
  h-measure's key rules) is unchanged, and so is each rule's JSON.
- **Link order is explicit data**, which is the merge claim order (semantics 3), instead of
  following from entity rule order.

The cost is that the two star rules construction would have given are validation rules
instead; `validate()` checks both and a test pins each rejection.

### Until the executor runs links

`MappingEngine::new` refuses a mapping with links (`MappingEngineError::LinksNotExecuted`)
until #245 PR 2 makes it claim merges, and `routes::decode_envelope` excludes a stored linked
mapping from routing with that reason (other callers still decode it), so `serve` reports it at start-up and still routes every other
source. A link is never silently dropped. A version-2 mapping without links runs.

## Plan (#245)

| PR | Content |
|---|---|
| 1 (this) | The format, this record, the 0005, 0021 and 0023 amendments; the h-measure preamble names the ceiling "the oracle-v0 mapping". |
| 2 | `MappingEngine` claims merges (semantics 3 and 4); check 11 gains a merge arm and a version-2 fixture. |
| 3 | h-measure scores through the fold, so a link can score; an "oracle with links" ceiling row. |
| 4 | The profiler emits links from its 1:1 merges (`PROFILER_VERSION` 6, a 0022 amendment), with pre-registered predictions and the memory gate. |

## Surfaces (0017)

The timeline and delta already render merges. The mapping state view and the MCP `sources`
tool show no mapping contents at all yet; #163 (PR 5, per-source mapping state) adds both, links
included.

## Consequences

- `s2w-model`: `LinkRule`, `MAPPING_VERSION_LINKS`, six `MappingError` variants, identity over
  the mapping's own version.
- `s2w-system1`: `MappingEngineError::LinksNotExecuted`.
- `s2w-app`: route resolution excludes a linked mapping until PR 2.
- Every `StreamMapping` literal gains `links: Vec::new()`; no behaviour change.
- No new dependencies; the allowlist is unchanged.

verify: `cargo test -p s2w-model mapping && cargo test -p s2w-system1 mapping` passes.
