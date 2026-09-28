# s2w-system1

System 1: per-event engines behind one verdict / confidence / abstain trait.

## Allowed dependencies

- `s2w-model`; `serde`, `serde_json`, `thiserror` (decision 0011); `model2vec-rs` (decision 0013, local embeddings); further engine libraries chosen in decision records

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Abstaining is a first-class answer.
- Every verdict is persisted; replay never re-runs an engine to reconstruct a past verdict.
  The bridge stores each verdict before serving it and replays stored verdicts on start
  ([decision 0012](../../docs/decisions/0012-verdict-log.md)).
- Three engines ship: Wikimedia page-change rules, JSON claims, and local embeddings
  ([decision 0013](../../docs/decisions/0013-local-embeddings-engine.md), `enwiki` only). Jev
  joins behind the same trait next.
- `embedding::CommentClassifier` has no dependency on `Engine`/`Verdict`/`AbstainReason` — a
  future System 2/Jev consumer can call it directly. `engines::embeddings::LocalEmbeddingsEngine`
  is the thin `Engine` adapter around it.
- Never depends on the core or on another adapter.
