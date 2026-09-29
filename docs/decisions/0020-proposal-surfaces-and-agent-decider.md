# 0020: Proposal surfaces, and an `agent` decider for MCP-recorded decisions

Date: 2026-09-28 · Status: accepted · Gate 3 · Issue #158 · Amends [0019](0019-system2-proposal-records.md) (decider vocabulary, schema version) and the read-only rule in [0009](0009-mcp-server.md) · Builds on [0017](0017-view-and-agents-first-class.md)

## Decision

**Surfaces.** The view reads proposals, decisions and grades through the query API:
`GET /worlds/{world}/proposals` and the MCP tool `proposals_list`, plus a web panel. Both are
built from stored rows only: `grade()` over `proposal_summaries()` and `decisions()`. A missing
`proposals.sqlite3` is an empty view; any other open or read failure is a storage error, never
an empty view. The view never serves payload bytes.

**A fourth decider, `agent`.** An MCP client is neither the human, nor the policy engine, nor
later evidence. Recording its opinion as `policy` would replace the real policy's latest row
and move `policy_applied`, the signal a consumer uses to revoke auto-apply; recording it as
`human` or `evidence` would inflate accuracy. So `Decider` gains `agent`, and the MCP tool has
no decider argument: it always records `agent`. `grade()` tallies it in its own `agent: Tally`
and never counts it in `policy_*`, `human`, `evidence`, `policy_applied`,
`policy_applied_ungraded` or `ungraded`. An agent opinion is context, not accuracy.
Human decisions need a human surface, which this issue does not add.

**Schema.** `decisions.decider` CHECK gains `'agent'` and `user_version` becomes 2. A version-1
store is unsupported (`Corrupt`), with no migration: no producer writes proposals yet, so no
version-1 data exists.

**Containing the MCP write.** `decision_record` (args `world`, `proposal_id`, `outcome`,
`basis`) exists only under `s2w mcp --log-dir DIR --allow-decisions`. Without the flag the tool
list is read-only, as before. It appends one decision row and never touches a proposal. It
opens the proposal writer per call: while another process holds the writer lock (a future
producer inside `serve`) it answers `store_locked`. A shared writer handle is that producer's
job. An unknown proposal, or an absent database, answers `unknown_proposal` and creates no file.
`decided_at_ms` is the server's clock and may precede `proposed_at_ms`. Identity is attribution,
not authentication.

**Payload-less read.** `proposal_summaries()` returns `ProposalSummary` (a proposal minus its
payload, keeping the stored `payload_hash`) so grading and the view never load BLOBs. It does
not recompute the hash; only `proposals()` verifies integrity. `grade` takes summaries.

**Shared helper.** `map_constraint` in `s2w-log` maps a SQLite constraint violation for both
`verdicts.rs` and `proposals/sqlite.rs`.

**`snapshot_offset` carries no epoch (2026-09-29, s2w#201).** A proposal's `snapshot_offset` is
a position in the append-only event log (discover writes its window's last position), not a
fold offset. The epoch (decision 0023) names a fold history: a rebuild re-folds the same log
under new routes, so fold offsets change meaning and log positions do not. The view therefore
serves the same bytes under every epoch, on HTTP, MCP and `s2w proposals list --json`, and a
client must not pass `snapshot_offset` as `at`. Pinned by
`snapshot_offset_is_a_log_position_with_no_epoch_and_survives_a_rebuild` in
`tests/proposals.rs`.

## Consequences

- Follow-up (not in this change): an `s2w proposals` CLI and a human-decision surface.
  - 2026-09-29 (#185): shipped as `s2w proposals list|grade|decide`. `decide` writes the
    `human` decider through `s2w_app::proposals`, the service `decision_record` now wraps, so
    the two write surfaces share one validation and one unknown-id rule (a missing store or id
    never creates the store). The store has no reviewer column, so a human basis is stored as
    `reviewer=<id>; <basis>`; that prefix is a human-decider convention, and an agent basis
    stays verbatim. The web view's human surface is still open.
- Every consumer of `grade()` must decide whether `agent` counts for anything; by default it
  does not.

verify: `cargo test -p s2w-log proposals` passes.
