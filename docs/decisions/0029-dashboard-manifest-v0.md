# 0029: Dashboard manifest v0

Date: 2026-09-29 · Status: accepted · Gate 3 · Issue #300 (#288 PR 1) · Builds on [0019](0019-system2-proposal-records.md), [0020](0020-proposal-surfaces-and-agent-decider.md), [0021](0021-stream-mapping-v0.md), [0023 routes](0023-routes-from-stored-mappings.md)

## Decision

**A world's dashboard manifest is a `dashboard-manifest` proposal in the proposal store,
resolved per world by 0023's rule.** It says what the world is, the form a native of its domain
recognises it by (the quintessential projection), who looks at it (roles), and how each entity
type and event type reads. The record lives in `proposals.sqlite3` (0019): no new table, no
schema bump. A `world_presentation` row was rejected: that table is latest-row-wins with no
decisions, so a manifest there could not be graded by actor or revoked by identity.

This PR ships the record, its validation, resolution and the read surfaces. Nothing proposes a
manifest yet; the proposers land in #301 and the viewer in #302. The plan is the `## Plan
(s2w#288)` comment on #288 and the two comments after it that amend it; where they differ, the
later comment wins.

### The envelope

One definition. Every key is required (`null` where absent) and unknown keys are refused at
every level:

```json
{ "format": 1,
  "world": "<world>",
  "input_hash": "<16 hex>",
  "attempt": 1,
  "manifest": { …DashboardManifest… } | null,
  "provenance": { "prompt_hash": "<16 hex>" | null, "input_tokens": 0 | null,
                  "output_tokens": 0 | null, "latency_ms": 0 | null,
                  "raw": "<reply text, ≤ 1 MiB>" | null, "error": "<reason>" | null } }
```

- `format` is 1. `world` is non-empty. `input_hash` and a non-null `prompt_hash` are 16
  lowercase hex digits. `attempt` is 1 to 3. `raw` is at most 1 MiB (bytes).
- **`manifest` is null exactly when `provenance.error` is set**: an invalid reply, non-JSON
  output, a timeout or an exec failure. Either both or neither is a refusal.
- **`identity()` hashes the `manifest` (and the format number) only.** Latency, tokens and `raw` never move it. A
  null-manifest row has no identity: resolution excludes it and every read reports it.
- `input_hash` = `fnv1a64_hex` over length-prefixed (the canonical JSON of the proposer's
  input, the prompt hash or `""`). The input includes the `built_on` mapping identities, so a
  new mapping changes it. Proposal id = `fnv1a64_hex` over length-prefixed (actor model, actor
  version, world, input_hash, attempt), in the shape of 0025. Attempt = 1 + this actor's
  null-manifest proposals for (world, input_hash); at most 3. The writer of these is #301.

### Format 1: required and optional

- **Required:** `built_on` (1 to 32 `{source, mapping}` pairs, `mapping` a 16-hex mapping
  identity), `domain` (`name`, `summary`), `quintessential_projection` (`template`,
  `rationale`, `slots`), `roles` (1 to 8, **exactly one** `default: true`; each `id`, `name`,
  `default`, `questions` (0 to 5) and `projection` (`template`, `slots`)), and `types` (0 to
  64 rows, each with `type` and `primary`).
- **Optional:** `types[].noun`, `types[].label` (`{"attr": name}` or `{"key": index}`),
  `types[].kind`, and the whole of `events` (0 to 64 rows). An absent optional field is
  skipped in the canonical JSON, not written as `null`.
- **Closed sets:** templates `document`, `feed`, `graph`, `table`, `map`, `ladder`; kinds
  `person`, `document`, `category`, `place`, `organisation`, `event`, `other`. The viewer maps a
  kind to an icon, never to a domain.
- **Strings:** every string is at most 200 characters and holds no `<` or `>`. The viewer
  renders every manifest string as text, never as HTML: stream text reaches the model, so a
  manifest is untrusted. Ids and types are distinct within their list.
- **Sentences:** `events[]` rows are `{source, when?: {path, equals}, sentence: {text,
  fields}}`. `text` holds `{n}` placeholders naming `fields[n]`; every field is shown at least
  once and no placeholder is out of range. A field is a path, `{"delta": [a, b]}` (the signed
  integer `a − b`) or `{"truncate": path}` (cut at 120 characters); at most 8 fields. Paths
  never appear in free text.

### The slot table

Unknown slot names are refused when decoding; a slot outside the template's row is refused by
`validate_shape`. Roles' projections use the same table.

| Template | Slot | Required | Must reference |
|---|---|---|---|
| `document` | `subject_type` | yes | a type label |
| | `actor_type` | no | a type label ≠ `subject_type` |
| | `links` | no | 1 to 8 `[from type, to type]` pairs, each a relationship in an accepted mapping |
| `feed` | `subject_type` | no | a type label |
| | `actor_type` | no | a type label ≠ `subject_type` |
| `graph` | `types` | yes | 1 to 8 distinct type labels |
| `table` | `type` | yes | a type label |
| | `columns` | no | 1 to 8 attribute names of that type (omit the slot for none) |
| `map` | `lat`, `lon` | yes | paths present in the input profile |
| | `subject_type` | no | a type label |
| `ladder` | `price`, `quantity` | yes | paths present in the input profile |
| | `side` | no | a path present in the input profile |

A type label is the `type_label` of an entity rule in an accepted mapping of a `built_on`
source. (The write-path check looks in every accepted mapping it is given, not only the
`built_on` ones; `stale_entries` catches a type that later disappears.) **`document.links` is a list of pairs**, not the plan's single pair: a document view
of an article with both an author and a category links more than one relationship, and a list
of one expresses the single case.

### Validation: the write path and the read path

- `validate(&ctx)` runs on the write path. `ManifestContext` holds the accepted mappings of the
  world's member sources and the input profile's paths per source. It runs `validate_shape`
  (everything above that needs no context) and then every reference: each `built_on` pair
  names that source's accepted mapping; every type, attribute, key part and relationship exists
  in those mappings; every path is in the profile. It returns the first fault: shape faults (in field order) before reference faults.
- **The read path is `stale_entries(&current)`**, against the mappings in effect now only
  (the read has no profile, so paths are not checked). A manifest built on mapping A goes
  stale when mapping B lands: it is served with `stale: true` and a list of the entries the
  current mappings lack, and the viewer falls back for those entries. It is not re-proposed
  automatically in v0.

### Identity

`identity()` = FNV-1a 64 over length-prefixed (`format` as u32 little-endian, the canonical JSON
of `manifest`: `serde_json` of the struct in declaration order, `None` fields skipped). It is
pinned by a test for a full manifest and for a manifest with no optional fields.

### Resolution

0023's resolver moves from `s2w_app::routes` into one generic function,
`s2w_app::query::resolve_class(class, decode, proposals, decisions)`, over a proposal class and
the scope key its decoder returns. `routes` calls it with `stream-mapping` per source, and the
dashboard read calls it with `dashboard-manifest` per world. The rules are 0023's, unchanged:
the latest `human` decision per (key, identity) binds the identity, else the proposal's own
latest `policy` decision; the largest accepted `seq` wins; the reported proposal is the
earliest accepted one carrying the winning identity. A human reject of the effective manifest
falls back to the one before it.

### Surfaces (0017: core, view, agent)

- `GET /worlds/{world}/dashboard` → `{manifest, proposal_id, actor, identity, stale,
  stale_entries, excluded}`. Every field but `excluded` is `null`, `false` or empty when no
  manifest is in effect; a missing store reads that way and is never created. **`excluded` is
  a seventh field** beyond the plan's six: the plan says null-manifest rows are "excluded and
  reported", and this is the report. It lists this world's unusable rows (and rows whose world
  cannot be read at all) as `{proposal_id, reason}`, in proposal order.
- MCP `dashboard` (read-only, byte-equal to the route) is the eighth read-only MCP tool.
- `s2w dashboard show [--log-dir] [--world] [--json]`; `--json` prints the route's bytes.
- The proposals panel and `proposals_list` already list the new class.
- `record_decision` (`s2w proposals decide` and MCP `decision_record`) refuses an `accept` on
  a `dashboard-manifest` proposal whose envelope does not decode or whose manifest is null,
  as it does for an undecodable `stream-mapping`. A reject is always allowed.

## What it never does

Render a manifest string as HTML; put provenance into the identity; resolve a manifest across
worlds; create the proposal store on a read.

## What is not here

- **`?at=` on the dashboard.** A manifest is decided at a log position, not at a fold offset
  (0020: `snapshot_offset` is a log position and carries no epoch), so the route serves the
  manifest in effect now.
- **Automatic re-proposal.** A stale manifest is flagged, not replaced; triggers are a later
  #116 slice.
- **The proposers and the exec path** (#301): the deterministic fallback and the System 2
  proposer, the `dashboard-auto-apply/1` policy (accept every manifest that validates, reject
  every null-manifest row with basis `invalid: <error>`), and the operator's responsibility
  that the configured command is tool-less.
- **The viewer** (#302).
