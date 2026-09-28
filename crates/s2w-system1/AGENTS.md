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
- One engine ships today: `JsonClaimsEngine`, which reads a claim's own declared shape, not a
  domain's. The Wikimedia-bound page-change rules engine and the `enwiki`-only local embeddings
  engine (decision 0013) were retired under decision 0018 (no compiled domain code) — see the
  follow-up issue for a generic field-filter replacement. Jev joins behind the same trait next.
- Never depends on the core or on another adapter.
- No domain knowledge in this crate; see decision 0018.
