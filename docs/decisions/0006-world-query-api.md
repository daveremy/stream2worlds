# 0006: The world query API

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #36 · Research [0003 §6](../../research/0003-rust-substrate.md), [0005](../../research/0005-3d-exploration.md), [0006](../../research/0006-scaling.md)

## Decision

The web view, `--json` and the MCP server (#10) read the world through one API in
`s2w-app::query`. Its pure half (`world_view`, `diff`, `fold_with_delta`, `Timeline`) does no
I/O and is callable directly; `router` serves it over HTTP with `axum` 0.8 and axum's SSE
(research 0003 §6). The static-asset and MCP halves of that recommendation belong to #10.

| Endpoint | Returns |
|---|---|
| `GET /world?at=&branch=&lod=&focus=&hops=` | d3 `{nodes, links}` at a fold offset, plus `offset`, `fold_version`, `hub_in_degree_cap` |
| `GET /events?from=` (or `Last-Event-ID`) | SSE, exactly one typed delta per offset: `entity`, `link`, `hub_ref` (with `tripped`), `merge`, `split`, `noop`; `id:` is the offset after the event |
| `GET /branches` | `[{name: "actual", world_id: 0, head, fold_version, hub_in_degree_cap}]` |
| `GET /diff?from=&to=` | entity-level nodes, links and merges added, removed, changed |
| `GET /entity/{id}/history?to=` | every delta naming the id, or an id resolved to it at that moment |
| `GET /time?ts=` | the largest offset whose events were received at or before `ts` (ms); without `ts`, the range and the clamp count |

Errors are `{"error": <code>, "message": <text>}` with stable codes: `offset_beyond_head` and
`unknown_entity` (404), `bad_parameter` and `hops_too_large` (400), `branch_not_yet` and
`lod_not_yet` (501), `unavailable` (503).

## Contract details

- **Time is the fold offset** (`World::offset`), not a log position (decision 0005). The served
  `Timeline` is a stand-in log of timestamped `WorldEvent`s until snapshots map fold offsets to
  log positions (#33). Each request folds its prefix from scratch; #33 removes that cost.
- **Timestamps never refuse an event.** An append earlier than the previous event is clamped to
  it and counted (`/time` reports `clamped`), so the index stays sorted and the log stays total.
- **Only the actual world is served.** Any other `branch` is `501 branch_not_yet`; no fork
  semantics are invented before branches exist.
- **Ids are prefixed strings** (`e:<id>`, `type:<name>`), so d3's `===` never mixes types. Entity
  nodes carry their natural `keys` for labels and their merged `members`.
- **Merges resolve at read time.** Link endpoints are re-resolved through current merges and
  weights summed. A merge moves no attributes: a survivor's node shows its own state (0005).
- **Hubs are aggregates at every `lod`** (research 0006). An entity whose in-degree exceeds the
  cap is a `hub` node carrying `in_degree`, `by_kind`, `last_seen_offset` and, since #42, its own
  `hub_refs` into other hubs. At `lod=entity` no link into a hub is emitted; sources, hubs
  included, list it in `hub_refs`. At `lod=type` each group gets one aggregate link into the hub,
  weighted by distinct sources. Hubs are never counted in a type
  and never expanded by a `focus` walk.
- **`lod=cluster` is reserved** (`501 lod_not_yet`). Connected components collapse into one
  component on a live stream and their ids are unstable across offsets; the definition waits for
  structure discovery.
- `focus` is an entity id (natural keys contain colons), resolved through merges; `hops` defaults
  to 1, maximum 5.

## Alternatives considered

- **A separate `s2w-query` crate.** Rejected for now: a crate depending on the core that is not
  `s2w-app` needs a new layer in decision 0001. The module can move out if MCP and the view need
  it without the rest of the app.
- **Deltas as several messages per offset** (a hub trip plus a hub_ref). Rejected: `Last-Event-ID`
  resume would be ambiguous.

## Revisit when

Branches land (serve `branch=`), snapshots land (#33: fold offset ↔ log position, no refold per
request), or structure discovery defines clusters.
