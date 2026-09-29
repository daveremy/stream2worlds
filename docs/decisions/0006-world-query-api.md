# 0006: The world query API

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #36 · Research [0003 §6](../../research/0003-rust-substrate.md), [0005](../../research/0005-3d-exploration.md), [0006](../../research/0006-scaling.md) · Amended by [0015](0015-named-worlds.md), [0016](0016-web-delivery.md), [0021](0021-snapshots.md)

> **Amended by 0015 (2026-09-27):** every route below moves under `/worlds/{world}/…`; the
> unscoped paths in this record are removed, not aliased. The endpoint semantics, error codes
> and contract details below are otherwise unchanged.
>
> **Amended by 0016 (2026-09-27):** `/events` also takes an optional `at=`, which bounds the
> replay: it sends the deltas after `from` through `at`, then closes (`at` below `from` is
> `bad_parameter`, past the head is `offset_beyond_head`). `/events` holds one of 32 concurrent
> stream slots and answers `stream_limit` (503) when none is free. Every route refuses an
> `Origin` header other than `http://` plus a loopback name on the served port with 403
> `origin_rejected`; a request without `Origin` is unaffected.
>
> **Amended by 0021 (2026-09-28):** a timeline restored from a world snapshot starts at a base
> offset `b`. Any offset below `b` (`/world?at=`, `/diff?from=`/`to=`, `/events?from=` and
> `Last-Event-ID`, `/entity/{id}/history?to=`) answers 410 `offset_before_base`, as does
> `/time?ts=` before the base's last event. `/time` gains `base` (0 without a snapshot), and
> entity history covers only offsets after `b`. The world at the head is no longer refolded
> per request.

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
| `GET /time?ts=` | the largest offset whose events were received at or before `ts` (ms); without `ts`, the range, the clamp count and `base`, the earliest servable offset (decision 0021) |

Errors are `{"error": <code>, "message": <text>}` with stable codes: `offset_beyond_head` and
`unknown_entity` (404), `bad_parameter` and `hops_too_large` (400), `branch_not_yet` and
`lod_not_yet` (501), `unavailable` (503, poisoned lock), `stream_limit` (503, event-stream cap),
`offset_before_base` (410, below a restored snapshot's base; decision 0021).

## Contract details

- **Time is the fold offset** (`World::offset`), not a log position (decision 0005). The served
  `Timeline` is a stand-in log of timestamped `WorldEvent`s until snapshots map fold offsets to
  log positions (#33). Each request folds its prefix from scratch; #33 removes that cost.
  *(2026-09-28, decision 0021: the head world is kept live and served as a clone; an offset below
  the head still folds from the timeline's base.)*
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
*(2026-09-28: snapshots landed in part in decision 0021, which removes the refold at head and
adds `offset_before_base`; the refold below head stays until a scrub index is measured to be
needed.)*
