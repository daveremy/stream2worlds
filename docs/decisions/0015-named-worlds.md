# 0015: Named worlds as the container — world-scoped API, membership history, deployment modes

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #92 · Amends [0006](0006-world-query-api.md), [0009](0009-mcp-server.md)

## Scope note — implementation plan v3 (2026-09-27)

The binding #95 implementation plan stores immutable manifest tables in `events.sqlite3`
and records the raw event-log head inside the membership writer transaction. Raw event
positions and timeline fold offsets are different spaces (one event can yield zero or many
claims), so `/sources?at=` currently has that display limitation; a live mutation API must
resolve it. Re-add with `Now` is rejected until adapters can resolve a live tail; re-add with
an explicit cursor takes effect on restart. Admin mutation and live worker resume remain
follow-ups. These implementation limits do not weaken commit-time ingestion rejection.

## Decision

A **world** — one directory holding a manifest, its event log and its verdict store (already
the unit 0014 locks and serves) — becomes a named, addressable container. Every HTTP route and
MCP tool is **world-scoped from the start**: `/worlds/{world}/…` and a required `world` string
parameter on every MCP tool, even though today exactly one world is served per process. `s2w
serve <source>` keeps its current single-source, single-directory shape and needs no new
flag: it serves world `default` unless `--world <name>` is given. Nothing about 0014's process
model, locks or lifecycle changes; this decision adds identity and a scoped API in front of it.
0006's and 0009's unscoped routes and tool signatures are removed, not aliased — every client
of the query API, including issue #10 PR 2's web view, is written against the scoped shape from
its first line; no vestige of the unscoped paths ships (NO VESTIGES).

The new `world` identifier is a different thing from the existing `WorldId` in `s2w-core`
(0006's `world_id: 0`, the branch identity `/branches` and the `branches` tool already return).
That field is an unrelated, pre-existing `u64` for branch/fork identity and is untouched by
this decision; naming the new string identifier `world` rather than `world_id` keeps the two
apart in both the route table and the MCP schema, so a reader of `/branches` inside a
`/worlds/{world}/branches` response is never left wondering which "world" a bare `world_id: 0`
refers to.

A world's source set is not fixed at creation. **Source membership is its own append-only,
ordered log**, following the event log's own precedent (0002: INSERT-only, SQLite triggers
reject UPDATE and DELETE) rather than a mutable field on the manifest. A new source can fold
from `now` or from a chosen offset; a removed source's past stays in the world's history and
only its future stops. "Which sources were live at offset N" is answered by folding this log
up to N, exactly as the world itself is a fold over events (0005) — membership is a second,
smaller fold alongside the first, not a mutation of the first. Membership rows are never
`WorldEvent`s and never advance the world's own fold offset (0005/0006); they are a parallel,
smaller history read only by membership queries and by ingestion (below), never by the world
fold itself.

Two worlds may read the same stream: sources are addresses (URIs), not owned resources, and a
source has no notion of which worlds consume it.

## Why a manifest plus a separate membership log, not one mutable file

The manifest (`world`, display name, created-at, engine set, policy set) is small,
whole-file-replaceable identity — nothing here needs history. Membership is exactly the kind
of fact this codebase already refuses to store as a mutable row: 0002 and the `s2w-log`
AGENTS.md invariants exist because "what happened, in order, permanently" is cheaper to answer
correctly from an append-only log than from a value that gets edited in place and loses its
past. Putting "sources: [...]" in the manifest and editing it on add/remove would answer "what
is the world's source set now" but not "what was it at offset 4,000" — which query-at-time
requires, per Dave's ruling (issue #92, 2026-09-27 20:49): *"i envision a world being
incrementally improved by adding or removing sources over time... query-at-time must know which
sources were members then."*

The membership log lives beside the event log and verdict store in the same directory,
following 0014's one-directory-one-world shape. Its rows are written inside the same SQLite
database and transaction as the event log's own writes (never a second store with its own
writer lock) — 0002 rejected a two-store design for exactly this reason: keeping an admin
action's recorded offset consistent with the log it describes needs one commit, not
cross-store coordination. The implementation issue below picks the table shape; this record
fixes that it is one transactional home, not two.

**Membership gates ingestion; it does not filter the fold.** Removing a source stops new
events from that source from ever being read and appended — the event log never receives them,
so the world fold (0005) stays a pure function of the event log alone, unchanged by this
decision. A removed source's past events are already in the log from while it was a member,
and are folded exactly as before; nothing about `world_view` or `diff` needs to consult
membership. Membership answers a narrower question — "which sources were live at offset N" —
for display and audit (a `/worlds/{world}/sources?at=` query, added to the follow-up issue
below), not "which events count."

Each row: source id, `Added { effective_from } | Removed`, the **world offset** at which the
row itself was appended (the same offset space 0006 defines — a fold offset, not a raw log
position), and a monotonic sequence number breaking ties between rows recorded at the same
offset. Membership at offset N is derived by folding these rows **by that recorded offset**, up
to and including N; a row's own recorded offset is never revised by a later fact, so the past
never changes retroactively.

Only `serve` writes membership rows — it is the only command that holds a `QueryState`
(0006/0014); `watch` holds the same writer locks but has no query state and is not where an
admin action is applied. 0014 already puts `serve` in sole possession of the event log's, the
verdict store's and the world's writer locks, so add/remove a source is routed to that running
process, never applied directly by an external tool against a directory the server holds open.
`serve` reads its `Timeline`'s current head offset directly when it writes a membership row —
cheap, since it already holds that state to answer queries (0006) — rather than a fresh fold
pass or the raw log-position-to-fold-offset mapping 0006 defers to #33, which is only needed by
a reader with the log but no already-running fold.

**Membership enforcement is commit order, not an offset comparison.** The bridge assigns fold
offsets after ingestion, with a lag behind the raw log (0014) — a fold offset recorded on a
`Removed` row and a log append's own position are not directly comparable, the same
log-position-vs-fold-offset distinction 0006 draws. So enforcement is stated at the level that
is actually true: the `Removed` row and the group-commit pump's batches commit through the same
writer lock, in the same database, on the same thread (0014); the pump checks current
membership before assembling each batch and never starts a fetch against a removed source, so
any batch that would contain that source's events simply never commits after the `Removed`
row's own commit. "Future stops" is a claim about commit order — no batch from a removed source
commits after its `Removed` row — not a claim compared across fold offsets and log positions.
A recorded fold offset on a membership row is audit and display data (what a
`/worlds/{world}/sources?at=` reader sees), not the mechanism that stops ingestion.

`effective_from` on `Added` answers a different question and is never confused with the row's
recorded offset: it names a position in the *source's own stream* (a Kafka partition offset, an
SSE cursor) to start reading from — a backfill instruction to ingestion, not a world offset. A
newly added source's events land in the world at whatever offset ingestion assigns them as they
arrive (the same as any other newly connected source), regardless of how far back into its own
stream `effective_from` told it to start reading. `Now` means "start reading new events only,
no backfill" — ingestion opens the source at its live tail instead of a stored or requested
cursor.

Membership intervals are half-open: a source is a member from its `Added` row's recorded
offset up to, but not including, its matching `Removed` row's recorded offset; the event at the
removal offset itself falls outside the span. Removal is only ever effective at its own
recorded offset — there is no retroactive removal, matching the append-only rule above. Because
the bridge assigns fold offsets after ingestion with some lag (0014), an event already committed
to the log just before a `Removed` row's own commit can still be folded at an offset numerically
past that row's recorded offset; commit order, not the recorded offset, is what guarantees it is
never dropped (previous section). A `sources?at=` read is therefore accurate for anything except
this narrow race at a removal boundary, matching the spirit of 0006's own clamped-timestamp
caveat — a display precision limit, never a correctness one, since no ingested event is ever
lost or double-counted by it.

A re-added source after removal is a new `Added` row, never a revived one — its prior history
stays exactly where it was, under its own membership span. Re-adding reuses the same
`SourceId`, so it shares that id's `(source, content hash)` dedupe history (`s2w-log`
AGENTS.md): re-ingesting a stream position already stored during the source's earlier
membership span dedupes exactly as any other repeat read would, which is correct — the payload
is already durable from the earlier span, not lost.

It does **not** inherit the stored cursor as-is. `effective_from` on the new `Added` row is the
explicit, administrator-supplied instruction for where re-ingestion resumes, and resolving it
writes the source's stored cursor to match, in the same transaction as the `Added` row itself —
the one case allowed to override a stored cursor (the existing invariant, "a stored cursor
beats `--since`; passing both is a usage error", governs the default `watch`/`serve` path, not
this explicit administrative action). Doing this atomically with the `Added` row matters: if
the resolved cursor were written later, or only in memory, a process restart between re-add and
the source's first new event would resume ingestion from the stale pre-removal cursor under the
ordinary "restarts resume from cursors" path — silently backfilling the removed span into
what `Now` promised was a clean restart.

## World-scoped API

| Today (0006/0009, unscoped) | This decision |
|---|---|
| `GET /world` | `GET /worlds/{world}/world` |
| `GET /events` | `GET /worlds/{world}/events` |
| `GET /branches` | `GET /worlds/{world}/branches` |
| `GET /diff` | `GET /worlds/{world}/diff` |
| `GET /entity/{eid}/history` | `GET /worlds/{world}/entity/{eid}/history` |
| `GET /time` | `GET /worlds/{world}/time` |
| (none) | `GET /worlds` — lists worlds this process serves: `world` (the string id), `name`, `head` offset |

MCP: the same five tools (`world_view`, `world_diff`, `entity_history`, `branches`, `time`)
each gain a required `world` string parameter, resolved the same way the HTTP path segment
is. One tool set, not one registered per world — rmcp's typed macros make a per-world dynamic
tool registry far more machinery than a parameter for a value that changes per call, not per
process.

`s2w serve <source>` maps to `world = "default"` unless `--world <name>` is passed, so a
single-source user's URLs are `/worlds/default/world`, etc. — one more path segment than today.
This is a breaking change to the routes and MCP schema themselves (0006/0009's unscoped shape
no longer exists, per NO VESTIGES above), but not to observable single-world behavior: the same
data is served, one path segment deeper. `GET /worlds` on a `serve` process reports the one
world it holds. The API surface is already shaped for more than one world when a process is
asked to hold more than one, which this decision does not yet build.

This settles the blocker on issue #10 PR 2 (web view): its routes and MCP calls are written
against `/worlds/{world}/…` from the first line of code, never against the unscoped paths.

## Tenancy and deployment modes

A tenant owns worlds; s2w itself carries no global, cross-world state, so isolation between
tenants is structural rather than enforced by an access-control layer inside the binary.
Deployment modes — local, self-hosted enterprise, third-party-over-Tailscale, hosted cloud —
run the same binary and differ only in the fronting layer: auth, TLS and network policy live
outside s2w (reverse proxy, Tailscale, a cloud gateway), never inside it. For a cloud offering,
start with one process or container per tenant — the strongest isolation and the simplest
implementation — and share processes across tenants only if cost measurements force it later.

Bias to automation, not per-proposal approval (Dave, issue #92): humans write policies (which
sources a world may add, what an engine is allowed to act on); day-to-day source add/remove
and engine evaluation run without a human clicking approve on each one.

## Not now

Auth, billing, and a shared multi-tenant process are explicitly deferred, matching the
proposal in issue #92. Also deferred by this decision specifically: **serving more than one
world from a single process.** 0014 already commits one process to one directory's locks;
extending that to many directories in one process is a real feature (a routing layer over
several `QueryState`s, several lock sets, lifecycle for opening/closing a world without
restarting the process) and nothing here should be read as having built it. Nothing in the
world-scoped API precludes it later — the path and MCP parameter already carry the id a
future multi-world process would dispatch on.

## Alternatives considered

- **Sources fixed at world creation.** Rejected: Dave's ruling is explicit that a world is
  incrementally improved by adding and removing sources over time, and the design must carry
  that cost rather than assume it away.
- **Membership as a mutable manifest field, with a separate change-history file for
  auditing.** Rejected: two sources of truth for the same fact, one of which (the manifest)
  would need to stay in lockstep with the other by convention rather than by the log's own
  append-only enforcement (0002's triggers). A single append-only log is both the record and
  the query surface.
- **One MCP tool per world (dynamically registered as worlds are created).** Rejected:
  worlds come and go at runtime; a tool set that must be re-registered on every world create/
  delete is more moving parts than a parameter, and rmcp's tool discovery is not designed to
  churn per world.
- **Unscoped routes now, world-scoped routes later when a second world exists.** Rejected per
  the issue's own reasoning and Dave's framing: cheap now, a breaking change once #10 PR2 and
  any external client exist against the unscoped shape.

## Revisit when

- Multi-world-per-process serving is needed (a demonstrated cost or operational reason to share
  one process across worlds, per the cloud-offering note above) — tracked by a follow-up issue
  (below), not built speculatively.
- The membership log's exact table shape, inside the event-log database this record fixes as
  its home (above), is decided at implementation time; this record settles the semantics and
  the transactional home, not the schema.
- Cross-world queries (a join or diff across two worlds) are asked for; nothing here defines
  one, and `/diff` remains scoped to a single `world`.

## Follow-up issues (Part of #92)

- World manifest + membership log: implement the directory manifest and the append-only
  membership log described above, with fold-to-offset membership queries and the
  `/worlds/{world}/sources?at=` read endpoint.
- World-scoped HTTP routes + MCP `world` parameter: apply the route table above across
  `s2w-app::query`, unblocking issue #10 PR 2's web view.
- Multi-world single-process server: the "eventually one server hosting many worlds" case
  from issue #92, deferred here.

verify: none — this record contains no code change; the follow-up issues each carry their own
build gate.
