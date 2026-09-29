# 0023: Routes from stored stream mappings

Date: 2026-09-29 · Status: accepted · Gate 3 · Issue #163 (PR 2a of 5) · Amends [0021 stream mapping](0021-stream-mapping-v0.md), [0011](0011-system1-bridge.md) (routing), [0006](0006-world-query-api.md) (epoch, PR 2b-i of #184) · Builds on [0012](0012-verdict-log.md), [0019](0019-system2-proposal-records.md), [0024 snapshots](0024-snapshots.md)

## Decision

**Routes are data.** At start-up `serve` reads the proposal store (read-only, so it coexists
with any writer, 0019), resolves the effective `stream-mapping` proposal per source, and
registers `Route::Exact(source) -> MappingEngine` for each one on top of
`EngineRegistry::with_defaults()`. A source with no accepted mapping stays unrouted, exactly
as before. A mapping for a source a default already routes (`stdin`) runs next to the default
engine, not instead of it; both feed the world. The routes are reported at start-up, one line per routed source and one per
proposal excluded from routing (an unusable payload), with the reason.

**Envelope.** One proposal class, `stream-mapping`. The payload is
`{"format":1,"source":"<source id>","mapping":<StreamMapping v0>}`, with unknown fields
refused. The source lives in the payload, not the class, because 0019 grades and revokes
auto-apply per class; one class per source would split that denominator.

**Resolution** (`s2w_app::routes::resolve`, pure):

1. Group `stream-mapping` proposals by the envelope's source.
2. Per proposal, take the latest decision per decider by decision `seq` (the selection
   `grade()` uses; sequence, never timestamp, 0019).
3. **Human decisions bind the mapping identity, not the proposal id.** For each
   (source, identity), the latest `human` decision by `seq` across every proposal carrying that
   identity decides: `accept` accepts all of them, `reject` rejects all of them. With no `human`
   decision on the identity, a proposal is accepted when its own latest `policy` decision is
   `accept`. A human reject beats a policy accept in either write order. `agent` decisions never count (0020: context, not authority).
   `evidence` decisions never count in v0; a policy that acts on them is a later record.
4. The effective mapping is the accepted proposal with the **largest proposal `seq`**.
   `proposed_at_ms` is caller-supplied (0019) and never breaks a tie.
5. **Revoke** is one appended `human` `reject` on the effective proposal. By rule 3 it binds
   the identity: a later proposal for the same source with the same bytes is not accepted
   whatever `policy` says, so a producer that re-proposes them cannot undo a revoke. A later
   human accept on any proposal of that identity lifts it for all of them.
6. Two accepted proposals with the same identity are one mapping. The proposal id reported and
   written into the engine's provenance is the **earliest-seq accepted proposal** with that
   identity, so a same-bytes re-proposal changes nothing a restart can see. After a revoke is
   lifted, that earliest proposal may itself carry the human reject; it is still the one named.
7. A payload that does not decode, names an invalid source, or holds an invalid mapping is
   excluded and reported by proposal id; other rows still route. A proposal store that exists
   but cannot be read stops `serve` (`AppError::Proposals`); a missing store means no routes.

## Identity

`StreamMapping::identity()` is FNV-1a 64, as 16 hex digits, over three length-prefixed fields
in order: `KEY_FORMAT` (little-endian `u32`), `MAPPING_VERSION` (little-endian `u32`), and the
mapping's canonical JSON (`serde_json` of the struct, fields in declaration order). Whitespace
and key order in the source text never change it; a key-format or mapping-format bump always
does. The fixture mapping's identity is pinned in a test.

**The engine name carries it: `MappingEngine::name()` is `mapping-<identity>`.** The verdict
key is (position, engine name, version) (0012) and the bridge cursor only rises, so a new
mapping must be a new name: it has no stored rows, the bridge evaluates every position under it,
and the old mapping's rows stay in the file unserved (0012: rows of an engine no longer
registered are not served). No verdict-schema change and no migration. `Engine::name` now
returns `&str` borrowed from the engine, and `Route` holds owned strings. `provenance()` also
names the proposal id. Its `mapping_hash` (0021: FNV-1a 64 of the canonical JSON alone) is a
different digest from the identity, which also covers `KEY_FORMAT` and `MAPPING_VERSION`. It
stays unchanged so provenance written before this decision still compares with provenance
written after it; the identity is already on every verdict row, in the engine name. A second
identity, over labels `serde_json` must escape or encode as multi-byte UTF-8, is pinned too.

**`KEY_FORMAT`** (`s2w_model::natural_key`, now 1) versions the natural-key text. A test pins a
known-answer key, so an encoding change without a bump fails the build. Mixed key formats break
entity joins (one entity becomes two), which is why the format is part of the identity.

**`MappingEngine::version()` stays out of the identity.** 0012 never re-runs history on a
version bump, and the asymmetry is deliberate: mixed executor versions only mix claim quality on
old positions, which the version column and `replayed_stale_version` make visible. An executor
fix that must re-type history has no lever today, by 0012's design; re-accepting the same
identity serves the same stored rows. If that is ever needed, it is its own record, not a second
hash input added here.

**Replay reads registered names only.** `VerdictStore::read_range_of(after, through, names)`
filters on the registry's engine names (SQLite in the query, via `json_each`), so after k
mapping changes a replay batch does not read and discard k orphaned rows per position. It is
an optimization: the bridge already served only rows of the engine it is judging. One
consequence: the per-position integrity checks (position, event hash) no longer see rows of
unregistered engines, so corruption confined to orphaned rows goes unreported.

**`serde_json` becomes a normal dependency of `s2w-model`** (it was a dev-dependency): the
canonical bytes are `serde_json` output. The allowlist entry is updated. `s2w-model` still
depends on nothing else new.

## Snapshots

`EngineRegistry::feed_fingerprint()` hashes every route and engine name, so a registry built
under mapping B has a different fingerprint from A's. A snapshot written under A fails validity
rule 4 ([0024 snapshots](0024-snapshots.md)) and is reported and ignored; `serve` replays from position 0 under B.
No new snapshot field. This needs one ordering rule: **`serve` resolves routes and builds the
registry before it restores a snapshot**, because restore needs the fingerprint.

Tests pin both halves: a restart under the same stored mapping replays every stored verdict
(`evaluated == 0`) and serves the same world hash; a snapshot taken under A is ignored under B
and the world equals a cold fold under B. A mutant engine with the bare name `mapping` shows why
the name matters: A's verdicts replay under B's name (`replayed > 0`, `evaluated == 0`), A's
snapshot is restored under B, and both worlds differ from the cold fold under B. A non-vacuity
check first asserts the two mappings fold different worlds.

## Epoch (PR 2b-i)

A restart under another mapping serves another history under the same offsets: offset `k`
under B is not offset `k` under A. The query contract names the history. The **epoch** is
`EngineRegistry::feed_fingerprint()`, printed as 16 hex digits; `snapshots::prepare` installs
it on the empty timeline before anything is served, and a restored timeline carries it too
(`Timeline::with_epoch`). `/world`, `/time` and each SSE `id:` (`<epoch>:<offset>`) report it;
every offset-taking HTTP route and MCP tool takes an optional `epoch`, and a supplied epoch that
is not the served one is 410 `stale_epoch` before any bounds check (decision 0006's amendment).
A bare offset opts out. The viewer pins the epoch of its first snapshot on its stream and
probes, and rebuilds from a fresh snapshot on `stale_epoch`.

The epoch is stateless: nothing is stored, so it is the same across restarts under the same
routes. A restart A, then B, then A serves A's epoch again. That reuse is sound because the
fold is deterministic: the same routes over the same log fold the same world at every offset,
so a URL pinned under A's first run means the same thing under A's third. Epoch `0` is reserved
for "no serving registry": a fresh `Timeline::new`, and the standalone `s2w mcp` replay, where
the epoch argument is additive (a caller may pass it; omitting it changes nothing).

Tests: `tests/epoch.rs` (every surface answers 410, not 404, after a swap to a shorter history;
bare forms unchanged; SSE ids; a follower ends with one `stale_epoch`), `tests/mcp.rs` (MCP
errors byte-equal to HTTP), and `serve/tests.rs` (a restart under B serves B's fingerprint and
refuses A's; a restored timeline serves the registry's epoch; two mutants, both histories under
epoch 0 and an id-only design that serves a bare reconnect, show what the epoch rules out).

## The world manifest

The manifest records `EngineRegistry::with_defaults().names()` at world creation and is
immutable. With routes from data it keeps naming the defaults while `mapping-<identity>` engines
feed the world. **The manifest's engine list is historical**, not the live registry. The live
per-source mapping state is #163 PR 5's sources surface.

## What is not here

- **Live rebuild** (#163 PR 2b): an accept while `serve` runs changes nothing until a restart.
  PR 2b rebuilds from position 0 in-process, adds an epoch to `/time` and SSE, and adds snapshot
  validity rule 5b (a stored verdict row exists at the snapshot position for each registered
  name routed to that source). It also records the measured backfill throughput and memory.
- **Producer** (#163 PR 4): nothing writes `stream-mapping` proposals yet, so no demo route
  exists until then. *2026-09-29: built, [decision 0025](0025-learned-mapping-auto-apply.md).*
- **Surfaces** (#163 PR 5, [0017](0017-view-and-agents-first-class.md)): the view, MCP and the
  sources API do not show which mapping a source runs or how many proposals were excluded.
  Start-up notes and `s2w proposals list` (#185) are the only report; `s2w proposals decide`
  also prints what the decided mapping's source runs after the write. `mcp/replay.rs` still
  serves every stored engine's verdicts, including engines no longer registered.
- No verdict compaction: each mapping change leaves about one row per event under the old name
  (#33 part 2). `evidence` does not resolve. The MCP `decision_record` tool still records
  `agent`.

## Consequences

- `s2w-model`: `KEY_FORMAT`, `StreamMapping::identity()`, `serde_json` as a normal dependency.
- `s2w-system1`: an engine's name may carry an identity; `MappingEngine` is named by its
  mapping.
- `s2w-app`: `routes` (envelope, resolution, registry, start-up report); `serve` builds its
  registry from the proposal store; replay reads registered names only.

verify: `cargo test -p s2w-app routes && cargo test -p s2w-app --test bridge_mapping_routes` passes.
