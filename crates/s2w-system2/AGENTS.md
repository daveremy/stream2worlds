# s2w-system2

System 2: asynchronous, budget-capped model passes over world snapshots.

## Allowed dependencies

- `s2w-model`; provider clients chosen in decision records

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Never sits in the stream. Proposals never act on the stream from inside this crate;
  auto-apply is a policy decision recorded as a decision row. Outward effects (exports,
  alerts) always need approval through the export manifest (decision 0019).
- Stream text is untrusted: model workers hold no action credentials; proposals use a constrained, validated format.
- Every model output is persisted; replay never re-runs an LLM.
- Every proposal and decision is persisted as an append-only record (decision 0019).
- Never depends on the core or on another adapter.
- No domain knowledge in this crate; see decision 0018.
