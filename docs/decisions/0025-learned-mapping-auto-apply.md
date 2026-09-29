# 0025: Learned-mapping auto-apply policy v1

Date: 2026-09-29 · Status: accepted · Gate 3 · Issue #197 (#163 PR 4a) · Builds on [0019](0019-system2-proposal-records.md), [0022 profiler](0022-discover-profiler.md), [0023 routes](0023-routes-from-stored-mappings.md)

## Decision

**`serve` proposes and accepts a learned mapping for an unrouted source at start.** Before the
registry is built, `s2w_app::discover` profiles the first `DISCOVER_WINDOW` (10,000) logged
events of each member source that has no effective mapping, with `s2w-discover` and its default
`Config`. A mapping becomes a `stream-mapping` proposal (0023's envelope) from
`Actor::Agent { model: "h-lite", version: PROFILER_VERSION }`, with `snapshot_offset` at the
window's last position, and a `policy` accept in the same run. `serve` then resolves routes
again, so the source is routed, backfilled and live in that same start. Dave's ruling on #163
(2026-09-28): learned mapping only, auto-applied by `policy` and logged, reviewable and
revocable.

This is the first writer of a `policy` decision. 0019 names `policy` for reversible, low-stakes
proposals; a mapping qualifies because one appended human reject revokes it and 0023 rebuilds
the world by identity.

**The policy, version 1.** Every accept carries this basis, so the rule that made it is on the
record:

```
policy=learned-mapping-auto-apply/1 profiler=h-lite/<PROFILER_VERSION> window=<first>..<last> events=<n> types=<t> entity_rules=<e> relationship_rules=<r>
```

`PROFILER_VERSION` is a constant in `s2w-discover`, bumped with any threshold or rule change,
so grading by (actor, version) never pools two profilers.

**Rules.**

1. Only a source with no effective mapping is profiled. A routed source is never re-profiled.
2. Idempotent by lookup. Under the writer lock, if a `stream-mapping` proposal for the same
   (source, identity) exists from any actor, nothing is written. A restart, a human-rejected
   identity and a same-bytes proposal filed through `s2w proposals` are all left alone.
3. The proposal id is `fnv1a64_hex` over the actor, source, first and last window position, and
   identity, each length-prefixed. The same log gives the same id; a moved window (after
   retention truncates the first positions, #33) gives a new one.
4. The writer is opened per run and dropped, never held by `serve`, so `s2w proposals decide`
   and MCP `decision_record` are never locked out by a running server. A held lock is a
   `store_locked` note; the source keeps its routes until the next start.
5. An abstaining profiler writes nothing and says why, with the event count. The window is
   fixed, so it is not retried in the same process.
6. A producer failure (log read, store I/O) is a `discover:` note, never fatal. A corrupt
   proposal store still stops `serve` at the resolution, as 0023 chose.

**Revoking.** `s2w proposals decide <id> --outcome reject` (#185). 0023 binds a human decision
to the (source, identity), so rule 2 never re-files it and a later `policy` accept on the same
identity would not route either.

## What it never does

Re-file a human-rejected identity; re-profile a routed source; hold the proposal writer; decide
on a key's name (checks 11 and 12 still gate the engine and the profiler).

## What is not here

- **In-run trigger** (#197 PR 4b): a source that reaches the window while `serve` runs is
  profiled only at the next start.
- **Claim-volume measurement** (#197 PR 4b): world size and memory under the discovered mapping
  at 10^5 events, and whether 10,000 events gives the same identity as 20,000. The deploy gate
  stays at ~350 MiB resident for the world (issue #197 ruling).
- **End-to-end obfuscation invariance through `serve`** (#197 PR 4b).
- **Grading** the policy's accuracy is #33/#56. No automatic revoke exists yet.
- **Surfaces** ([0017](0017-view-and-agents-first-class.md)): which mapping a source runs is
  shown only in start-up notes, `s2w proposals list` and the proposals panel; the per-source
  mapping state on `/sources`, MCP `sources` and the view is #163 PR 5.
