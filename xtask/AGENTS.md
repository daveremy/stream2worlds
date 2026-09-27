# xtask

The workspace's fitness functions: `cargo xtask check`.

## Allowed dependencies

- `serde`, `serde_json`, `toml`, `syn`, `proc-macro2`, `s2w-core` (the golden replay check folds the golden log)

The enforced list is `xtask/allowlist.toml`. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Structural-check inputs: never text or regexes; `syn` ASTs, Cargo metadata, and this crate's own TOML config only. Existing manifest checks retain their TOML inputs (decision 0001, amendment).
- Every violation message says what to do next.
- A new check is shown to fire (break the rule on purpose, watch it fail) before it is trusted.

## Internal modules

- `main.rs`: CLI, metadata, dependency and workspace checks.
- `golden.rs`: deterministic golden replay.
- `module_size.rs`: AST module traversal, calibration, dep-info backstop and exemption ratchet.

`cargo xtask check --tighten-baseline` removes stale exemptions and lowers ceilings to actual
counts; it never raises them. Ordinary cap and exemption-shape findings are report-only until
`module-size.toml` enables enforcement; baseline growth always blocks without an authorized
`Baseline-growth: s2w#<N>` commit trailer in `origin/main..HEAD`. CI needs full git history.
