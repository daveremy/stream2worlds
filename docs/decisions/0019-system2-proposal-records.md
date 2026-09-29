# 0019: System 2 proposal and decision records — durable attribution and grading

Date: 2026-09-28 · Status: accepted · Gate 3 · Issue #88 · Amends the invariant in [s2w-system2/AGENTS.md](../../crates/s2w-system2/AGENTS.md) · Builds on [0012](0012-verdict-log.md)

## Decision

Every System 2 proposal and every decision on it is persisted as an append-only record before
its consumer relies on it. Replay reads the stored payload and decisions without calling the
model again. These records supply Gate 3's grading numerator and denominator, preserving
history across restarts and keeping each model version's denominator separate.

Human accept/reject is a **grading signal**, not a universal approval gate. This amends
"Proposals are inert until accepted; nothing auto-accepts": proposals never act on the stream
from inside `s2w-system2`; auto-apply is a policy decision recorded as a decision row. Outward
effects (exports, subscriber alerts) always need approval through the export manifest (a rule for the export path, not enforced by this store). Model
workers still hold no action credentials, and System 2 validates its constrained proposal
format. This store neither applies proposals nor grants authority.

**Records.** `NewProposal` supplies a stable `id`, opaque `class`, `Actor::Human { id }` or
`Actor::Agent { model, version }`, `snapshot_offset: LogPosition`, opaque `payload: Vec<u8>`,
and `proposed_at_ms: i64`. `StoredProposal` adds a write-order `seq` and `payload_hash: i64`.
The class is a caller-chosen tag, never interpreted here; policy consumers need it to revoke
auto-apply for a class whose graded accuracy falls below its threshold. Actor identity is
attribution, not authentication.

The store computes `payload_hash` using the event log's pinned FNV-1a content hash; callers
cannot supply it. Every proposal read recomputes it, and a mismatch is `LogError::Corrupt`.
It binds the stored bytes for integrity, not security. Payload encoding and validation belong
to System 2 and its composition layer; this crate has no domain knowledge or dependency on
`s2w-system2` ([0018](0018-no-compiled-domain-code.md)).

`NewDecision` supplies `proposal_id`, `decider`, `outcome`, opaque non-empty `basis`, and
`decided_at_ms: i64`. `StoredDecision` adds a separate write-order `seq`. A decider is `policy`
(auto-apply or decline for reversible, low-stakes proposals), `human` (sampled review or a
class routed to review), or `evidence` (later events confirm/refute: accept/reject). Outcomes
are `accept` or `reject`. Basis names a policy identity, reviewer or evidence reference; it
remains opaque, so per-policy-version grading would require parsing it and is deferred.
Multiple decisions per proposal are legal. Corrections append another row; they never edit
history. A proposal's sequence and a decision's sequence belong to independent tables.

**Where and durability.** `<log dir>/proposals.sqlite3` is a sibling of `events.sqlite3` and
`verdicts.sqlite3`, with its own writer lock (`PROPOSALS_LOCK`) and `user_version = 1`.
Independent writer/process ownership is the same reason for a sibling database as 0012.
`ProposalStore` has `append_proposal`, `append_decision`, `proposals`, and `decisions`;
`InMemoryProposalStore` meets the same append, retry and grading contract as `SqliteProposalStore::open(dir)`; the corruption checks (hash recompute, unknown enum strings) are SQLite-only because memory cannot be edited behind the store's back.
`ReadOnlySqliteProposalStore::open(dir)` provides matching reads without taking the writer
lock or creating/migrating a schema, and can coexist with the active writer.

Every writer open configures WAL, `synchronous=FULL`, `recursive_triggers=ON` and
`foreign_keys=ON`, using the shared connection helpers. Each append is one atomic SQLite
statement/transaction. The schema is initialized transactionally; an unsupported version is
`Corrupt`. Reads return rows in ascending sequence order.

**Schema.** `proposals` has an AUTOINCREMENT integer primary key `seq`, unique non-null text
`id`, non-null text `class` and `actor_kind`, nullable text `actor_id`, `model`, `model_version`,
and non-null `snapshot_offset INTEGER`, `payload_hash INTEGER`, `payload BLOB`,
`proposed_at_ms INTEGER`. CHECK constraints restrict `actor_kind` to `human`/`agent` and
require exactly the appropriate identity columns: a human has `actor_id` and no model fields;
an agent has model and version and no human id.

`decisions` has an independent AUTOINCREMENT primary key `seq`, non-null text `proposal_id`
referencing `proposals(id)`, non-null text `decider`, `outcome`, `basis`, and non-null integer
`decided_at_ms`. CHECK constraints restrict decider and outcome to the vocabulary above.
Both tables have triggers refusing UPDATE and DELETE. Raw external connections must enable
recursive triggers to prevent INSERT OR REPLACE from bypassing delete triggers, and enable
foreign keys to enforce references; those pragmas are per connection. Unknown actor-kind,
decider or outcome strings on read are `Corrupt`, even if a raw writer bypassed CHECKs.

**Validation and retry.** Before writing, both implementations require non-empty proposal
id/class, human id or agent model/version, and decision proposal id/basis. Payloads larger
than the event log's `MAX_PAYLOAD_BYTES` (8 MiB) return `TooLarge`. A decision referencing an
unknown proposal returns `Corrupt` (foreign key in SQLite, explicit check in memory).
Invalid metadata or an unrepresentable snapshot offset is `Corrupt`; refusal stores nothing.
Empty payloads are legal. No domain meaning is inferred from any string.

`append_proposal` is idempotent for crash-retry: the same id, class, actor, snapshot offset,
payload hash **and payload bytes** returns the original stored row. A conflicting identity
is `Corrupt`; comparing bytes prevents a hash collision from silently dropping a different
proposal. A retry ignores `proposed_at_ms`, retaining the original timestamp. Decision retries
append rows; identical duplicates are harmless because grading selects the latest sequence.
A crash can leave a proposal with no decision, which is explicitly represented in the grade.
No cross-store or proposal-plus-decision transaction is promised.

**Snapshot and time boundaries.** The store does not cross-check `snapshot_offset` against
the event log; that is the consumer's job. Both timestamps are caller-supplied Unix
milliseconds, and this crate reads no clock and generates no random identity. A decision
with `decided_at_ms < proposed_at_ms` is allowed because caller clocks can differ. Sequence,
not timestamp, determines which correction wins.

**Grading.** The sole entry point is the pure free function
`grade(&[StoredProposal], &[StoredDecision]) -> Vec<ActorClassGrade>`, not a trait method.
It groups by `(class, actor)` in deterministic BTreeMap order. Different model versions,
different models, human identities, and classes have separate denominators. The input is
stored rows with unique proposal ids and decision sequences; decisions whose proposal is
absent from the supplied proposal slice are ignored. For each `(proposal, decider)`, the
largest decision sequence wins regardless of slice order or timestamps.

Each `ActorClassGrade` contains:

- `class`, `actor`, `proposed` (all proposals in the group), and `ungraded` (no human or
  evidence decision; policy-only proposals are still ungraded).
- `policy_accepted` and `policy_rejected`: latest policy routing counts, explicitly **not
  accuracy**. Policy cannot grade its own auto-apply as successful.
- `human` and `evidence`: independent `Tally { accepted, rejected }` values for the latest
  decision from each source. Never add these tallies or their denominators together: one
  proposal can have both signals. There is no precedence between human and evidence here.
- `policy_applied`: among proposals whose latest policy decision is accept, rejected if
  **any** latest human/evidence decision rejects; otherwise accepted if at least one accepts.
  This conservative precedence applies only inside this cross-tab. It is **not time-ordered**:
  a human/evidence decision may precede the policy decision in sequence or caller time.
- `policy_applied_ungraded`: policy-accepted proposals with neither human nor evidence
  decisions, excluded from the cross-tab's accuracy denominator.

`Tally::fraction()` returns `(accepted, accepted + rejected)` as `(u64, u64)`, without floats.
An empty tally returns `(0, 0)`, not a perfect score. `policy_applied.fraction()` is the
numerator/denominator for a consumer deciding whether to revoke auto-apply for this class and
actor. Threshold selection, review sampling, policy application and revocation ship in the
consumer, not here. Historical decisions remain readable even when later rows supersede them.

## Consequences

- This storage prerequisite lands before the first System 2 producer. No producer exists yet,
  so this change wires nothing into serve or CLI. `s2w-system2` cannot depend on `s2w-log`;
  `s2w-app` will compose them while preserving the layer rule.
- Gate 3 follow-up **#13** owns producer integration, the proposal/decision view and its MCP
  surface, as required by [0017](0017-view-and-agents-first-class.md). This issue supplies the
  records and pure grade, not those surfaces; no renderer change or web build is needed.
- Restart reconstructs both records and grades from storage alone. Consumers must persist
  records before applying or serving their effects; replay never reconstructs a stored
  proposal by asking its model again.
- Contract tests cover both stores, invalid writes leaving nothing stored, both actor kinds,
  retries, corrected decisions, restart equality, read-only coexistence, independent locks,
  schema checks, append-only triggers and tamper detection. Grading tests cover actor/version
  separation, policy-only ungraded rows, independent signals and conservative policy grades.

verify: `cargo test -p s2w-log proposals` passes.
