# s2w-system1

System 1: per-event engines behind one verdict / confidence / abstain trait.

## Allowed dependencies

- `s2w-model`; `serde`, `serde_json`, `thiserror` (decision 0011); further engine libraries chosen in decision records

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Abstaining is a first-class answer.
- `Engine::name()` borrows from the engine and may carry an identity; it must be stable for the
  engine's lifetime, because it keys stored verdicts and the feed fingerprint.
- Every verdict is persisted; replay never re-runs an engine to reconstruct a past verdict.
  The bridge stores each verdict before serving it and replays stored verdicts on start
  ([decision 0012](../../docs/decisions/0012-verdict-log.md)).
- Two engines ship today. `JsonClaimsEngine` reads a claim's own declared shape, not a
  domain's. `MappingEngine` executes a `StreamMapping` (decision 0021): paths, type labels and
  kinds are data handed to it at construction, never code. Its name is `mapping-<identity>`
  (decision 0023), so each mapping is its own engine to the verdict log; `serve` registers it
  per source from stored proposals. The Wikimedia-bound page-change rules engine and the `enwiki`-only local
  embeddings engine (decision 0013) were retired under decision 0018 (no compiled domain code).
  Jev joins behind the same trait next.
- Obfuscation replay covers both: `cargo xtask check` 10 runs claim-reading engines over the
  golden log, and check 11 runs `MappingEngine` over `testdata/raw-sample.jsonl` with
  `testdata/sample.mapping.json` and the linked `testdata/sample-links.mapping.json`
  (decision 0027; a link claims `EntitiesMerged` between the entities and the relationships). `testdata/` holds recorded domain data (decision 0018 §4);
  change a fixture only with the command recorded in decision 0021.
- `decode` (path lookup, in-place decode, `key_part`, and `natural_key` with its `entity_key`
  wrapper, the engine's one key builder) is public because check 11 and `cargo xtask
  h-measure` run it too: one implementation, so neither can drift from `MappingEngine`. Keys
  are built with `s2w_model::NaturalKey::from_parts`, never by hand.
- Never depends on the core or on another adapter.
- No domain knowledge in this crate; see decision 0018.
