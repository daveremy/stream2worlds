# s2w-system1

System 1: per-event engines behind one verdict / confidence / abstain trait.

## Allowed dependencies

- `s2w-model`; `serde`, `serde_json`, `thiserror` (decision 0011); further engine libraries chosen in decision records

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Abstaining is a first-class answer.
- Every verdict is persisted; replay never re-runs an engine to reconstruct a past verdict.
  The bridge stores each verdict before serving it and replays stored verdicts on start
  ([decision 0012](../../docs/decisions/0012-verdict-log.md)).
- Two engines ship in the first slice: Wikimedia page-change rules and JSON claims. Local
  embeddings are next; Jev joins behind the same trait.
- Never depends on the core or on another adapter.
