# 0029: Dashboard manifest v0

Date: 2026-09-29 · Status: accepted · Gate 3 · Issue #300 (#288 PR 1) · Builds on [0019](0019-system2-proposal-records.md), [0020](0020-proposal-surfaces-and-agent-decider.md), [0021](0021-stream-mapping-v0.md), [0023 routes](0023-routes-from-stored-mappings.md)

## Decision

**A world's dashboard manifest is a `dashboard-manifest` proposal in the proposal store,
resolved per world by 0023's rule.** It says what the world is, the form a native of its domain
recognises it by (the quintessential projection), who looks at it (roles), and how each entity
type and event type reads. The record lives in `proposals.sqlite3` (0019): no new table, no
schema bump. A `world_presentation` row was rejected: that table is latest-row-wins with no
decisions, so a manifest there could not be graded by actor or revoked by identity.

This PR ships the record, its validation, resolution and the read surfaces. #301 PR 1 adds the
deterministic proposer and its filer (`### The proposer`, below); #311 adds the System 2
proposer (`### The System 2 proposer`, below); the viewer follows in #302. The plan is the `## Plan
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

### The proposer (#301)

`s2w dashboard propose --log-dir D [--world W] [--dry-run] [--json]` runs
`s2w_app::dashboard::propose` with a `ManifestProposer` (trait and input DTOs in `s2w-model`).
PR 1 ships one proposer, `FallbackProposer` in `s2w-discover` (actor `dashboard-fallback/1`):
the `feed` projection, one default role, a label per type where the statistics support one.
It abstains with no mapped source, with no type label a manifest can hold, and with more
mapped sources than `built_on`'s cap of 32.

- **Input.** The member sources (0025's membership; every source when the log has no
  membership rows) that have an accepted `stream-mapping`, sorted by id. Per source: its
  newest 2000 logged events (the tail), read backwards from the log's head in a window that
  starts at 4000 positions and doubles until every source has 2000 or the window covers the
  log; the profiler's per-path statistics over the tail (`count`, `distinct`, `str_count`,
  `str_len_mean`) and its event-type path; and a sample, the newest 40 events of the tail as
  JSON, root fields the profiler decodes parsed in place, strings cut to 200 characters. A
  mapped source with no events is left out. The proposal's `snapshot_offset` is the newest
  position the tails read.
- **Replay.** The filer reads the store first, and again under the writer lock before it
  appends. For this actor's rows on (world, input_hash): an undecided row gets its policy
  decision (the process stopped between two appends); any manifest row means nothing to do;
  3 null-manifest rows mean nothing to do; otherwise it asks the proposer (outside the lock)
  and files attempt n + 1. The same log therefore files nothing the second time.
- **Policy `dashboard-auto-apply/1`.** A manifest that passes `validate()` against the input's
  mappings and paths is accepted with basis `policy=dashboard-auto-apply/1 proposer=M/V
  world=W built_on=s:id,… types=n events=n roles=n`. An `Invalid` answer, or a manifest the
  validator refuses (error `validator: …`, the manifest kept in `raw`), is a null-manifest
  row rejected with basis `invalid: <error>`. An `Abstain` writes nothing.
- `EventLog::head()` and `LogReader::read_head()` give the tail its start. They are named
  apart for the same reason as `replay` and `read_after`: both traits are in scope in the
  same files, so one name would be ambiguous.

### The System 2 proposer (#311, 2026-09-30)

`s2w dashboard propose ... --system2-model M/V [--system2-env NAME]... --system2-cmd <program>
[<arg>...] [--]` runs `s2w_app::dashboard::propose_system2`: the same filer and policy as
above, with `System2Proposer` (`s2w-system2`) over an `ExecProvider` in place of the fallback.
Without `--system2-cmd` and `--system2-model` the fallback runs, as before.

- **One attempt, at most two calls.** The prompt (`crates/s2w-system2/prompts/manifest.txt`,
  scanned by check 9) carries the input as one line of JSON between two marker lines. A reply
  that is not JSON, does not decode, or fails `validate()` gets one repair call carrying the
  reply and the fault; its answer is final. A provider failure (timeout, exit status, output
  over the cap) is not repaired. Tokens and latency are summed over the calls; `raw` is the last
  reply (or the output a failed repair call captured). The prompt files' hash is the envelope's `prompt_hash` and is folded into the input
  hash, so a prompt edit is a new input.
- **The command.** Run without a shell, `argv` as given, in a fresh empty directory, with an
  empty environment plus the `--system2-env` variables (each must be set, or the run stops with
  `bad_parameter` before the log opens). The prompt goes to stdin; stdout is the reply. Limits:
  180 s, 1 MiB of stdout, 64 KiB of stderr. On unix the command gets its own process group and a
  timeout kills the group (best effort, `/bin/kill`), so a wrapper's child does not keep running.
  Each failure is a null-manifest row with a reject whose basis names it (`exec: timed out
  after 180 s`, `exec: stdout was still open after the command exited`, ...), with the latency
  and any stdout captured.
- **Actor.** `--system2-model` is split at its last `/` into model and version and recorded as
  given; a CLI may take an alias there, and the row records the alias. The model never names
  itself.
- **Replay.** `ReplayProvider` answers from recorded replies keyed by the prompt's hash; a miss
  is a null-manifest row. Tests use it and local `sh` scripts only; no test calls a model.

**Operator recipe.**

1. Pick a CLI that reads a prompt on stdin and prints only the reply on stdout, and run it with
   its tools off (no shell, no file or network tools): the prompt carries stream data, and a
   tool-less command is the operator's responsibility, not something `s2w` can check.
2. Give `argv[0]` as an absolute path, or pass `PATH` with `--system2-env PATH`: the command
   sees no variable it is not given.
3. Pass what the CLI needs to run and authenticate: usually `--system2-env HOME
   --system2-env PATH` plus its API-key variable.
4. Put `--system2-cmd` last, or end it with `--`: every token after it up to a standalone `--`
   is the command's, so the command cannot receive a literal `--`.
5. Try it with `--dry-run --json`: when the run would file, the command runs (and costs what a
   call costs), nothing is written, and the report shows the envelope and the decision it would
   get. A log that already has this actor's row for the input runs nothing.

```sh
s2w dashboard propose --log-dir ./s2w-data --json \
  --system2-model <model>/<version> \
  --system2-env HOME --system2-env PATH --system2-env <API_KEY_VARIABLE> \
  --system2-cmd /absolute/path/to/model-cli <its flags for stdin prompt, text reply, no tools>
```

Tokens are null on this path: a generic CLI's stdout carries no token count `s2w` can trust.
Which provider and model class the first real run uses is s2w#288's open question.

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
  in the accepted mappings the check is given; every path is in the profile. It returns the first fault: shape faults (in field order) before reference faults.
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

### Labels and sentences (#302, 2026-09-30)

This update adds what the viewer needs to name entities and to read events. A later
re-proposal of the manifest carries all of it; a manifest filed before it still loads.

- **Type rows.** Each `types[]` row may carry `noun` (one entity of the type), `label`
  (`{"attr": name}` or `{"key": i}`) and `kind` (a closed set; the viewer maps it to an icon).
  A label never reads a date-time path: the profiler marks those `"timestamp": true`.
- **Sentences.** `events[]` holds `{source, when?, sentence}`. For one event the renderer takes
  the source's entries whose `when` matches the payload, in manifest order, then its entries
  with no `when`; the first that renders wins, and none gives `null`. A field shows a string
  as it is and a number or bool as its JSON text; `delta` shows a signed integer difference;
  `truncate` keeps 120 characters and adds `…`. An absent, null, array or object field, or a
  non-integer `delta` operand, fails that entry.
- **Fallback v2** (`dashboard-fallback/2`). A type's label is its attribute with the most
  distinct values, the shorter mean string length breaking a tie, among attributes with at
  least half as many distinct values as the type has entities (no attribute qualifies when
  the type's key was not profiled); else its first key part when
  that is mostly strings and not a date-time; else none, and the type is not primary. The
  half-share rule came from the demo's mapping, where an editor's only attributes were its
  edit kind and content model: without it every editor was labelled "edit". The noun is the
  type label. Per source, one or two entries for its busiest primary type (a date-time key
  never counts): `"{0}: <type> {1}"` over the event type and the first key when the event type
  is profiled, then `"<type> {0}"`.
- **Fallback v3** (`dashboard-fallback/3`, 2026-09-30, s2w#288). Two more tests on a label
  attribute, both on statistics only. **Coverage:** an attribute of a rule is a candidate only
  when it is on at most twice as many events as the rule's first key path, and a rule with no
  profiled key offers none. **Floor:** a candidate needs at least 8 distinct values as well as
  the half share. Measured cause: on the demo, the two rare `redirect_page_link/wikibase_*`
  types have their key on 40 of 2,000 tail events (16 distinct), while the content-model
  attribute is on all 2,000 with 4 values. The share compared a count over 2,000 events with
  one over 40; when the rare key had 8 or fewer values in the window, 4 cleared half and the
  category became the label. Coverage is the root-cause rule. The floor is a sample-size
  guard, since a share over fewer than 8 values is noise; it alone would also have caught this
  case. Both thresholds are heuristics checked against one stream.
- **Surfaces.** `GET /worlds/{world}/sentences?last=N` (`last` required, 1 to 200) returns
  `{rows}`, newest last, each `{position, source, sentence, entities}`; an entity carries
  `type`, `key`, and `entity` when the head world holds the key. MCP `sentences` is byte-equal
  and the ninth read-only MCP tool.
- **System 2 prompt.** The prompt asks two more questions: a noun, label and kind per type
  (never a timestamp path), and a sentence per event type.

## What it never does

Render a manifest string as HTML; put provenance into the identity; resolve a manifest across
worlds; create the proposal store on a read.

## What is not here

- **`?at=` on the dashboard.** A manifest is decided at a log position, not at a fold offset
  (0020: `snapshot_offset` is a log position and carries no epoch), so the route serves the
  manifest in effect now.
- **Automatic re-proposal.** A stale manifest is flagged, not replaced; triggers are a later
  #116 slice.
- **A token count from the exec path.** Tokens are null for `--system2-cmd`; reading them
  needs a known CLI's output format (s2w#288).
- **The first real System 2 run** on the demo world: it waits on s2w#288's provider and model
  class.
- **The viewer** (#302).
