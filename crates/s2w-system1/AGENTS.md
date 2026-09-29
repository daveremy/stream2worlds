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
- Two engines ship today. `JsonClaimsEngine` reads a claim's own declared shape, not a
  domain's. `MappingEngine` executes a `StreamMapping` (decision 0021): paths, type labels and
  kinds are data handed to it at construction, never code; it is not registered in any route
  yet (s2w#163 PR 2). The Wikimedia-bound page-change rules engine and the `enwiki`-only local
  embeddings engine (decision 0013) were retired under decision 0018 (no compiled domain code).
  Jev joins behind the same trait next.
- Obfuscation replay covers both: `cargo xtask check` 10 runs claim-reading engines over the
  golden log, and check 11 runs `MappingEngine` over `testdata/raw-sample.jsonl` with
  `testdata/sample.mapping.json`. `testdata/` holds recorded domain data (decision 0018 §4);
  change a fixture only with the command recorded in decision 0021.
- `decode` (path lookup and in-place decode) is public because check 11 runs it too: one
  implementation, so the check cannot drift from `MappingEngine`. Keys are built with
  `s2w_model::NaturalKey::from_parts`, never by hand.
- Never depends on the core or on another adapter.
- No domain knowledge in this crate; see decision 0018.
