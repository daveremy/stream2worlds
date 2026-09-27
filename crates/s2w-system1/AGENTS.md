# s2w-system1

System 1: per-event engines behind one verdict / confidence / abstain trait.

## Allowed dependencies

- `s2w-model`; engine libraries chosen in decision records

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Abstaining is a first-class answer.
- Every verdict is persisted; replay never re-runs an engine to reconstruct a past verdict.
- Two engines ship in the first slice (rules, local embeddings); Jev joins behind the same trait.
- Never depends on the core or on another adapter.
