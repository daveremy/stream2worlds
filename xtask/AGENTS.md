# xtask

The workspace's fitness functions: `cargo xtask check`.

## Allowed dependencies

- `serde`, `serde_json`, `toml`

The enforced list is `xtask/allowlist.toml`. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Checks read Cargo metadata and manifests, never Rust source text. A source-text scanner has an
  unbounded bypass space (decision 0001, amendment).
- Every violation message says what to do next.
- A new check is shown to fire (break the rule on purpose, watch it fail) before it is trusted.
