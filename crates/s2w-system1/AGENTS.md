# s2w-system1

System 1: per-event engines behind one verdict / confidence / abstain trait.

## Allowed dependencies

- `s2w-model`; engine libraries chosen in decision records

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Abstaining is a first-class answer.
- Every verdict is persisted; replay never re-runs an engine to reconstruct a past verdict.
  - **Amendment, 2026-09-27 (#51, [decision 0011](../../docs/decisions/0011-system1-bridge.md)):
    not yet true.** The bridge re-evaluates on every start. That is equivalent to reading a
    persisted verdict only because both shipped engines are pure functions of the payload (no
    clock, RNG, or model file). An engine whose output depends on an input the log does not
    capture (a model file, as in local embeddings or Jev) must not ship without the persisted
    verdict log (#63), and neither may a persisted world (#33). Bumping an existing engine's
    `version()` also changes what a restart serves, silently, until that log exists.
- Two engines ship in the first slice: Wikimedia page-change rules and JSON claims. Local
  embeddings are next, after persisted verdicts; Jev joins behind the same trait.
- Never depends on the core or on another adapter.
