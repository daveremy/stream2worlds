# xtask

The workspace's fitness functions: `cargo xtask check`.

## Allowed dependencies

- `serde`, `serde_json`, `toml`, `syn`, `proc-macro2`, `s2w-core` (the golden replay check folds the golden log)

The enforced list is `xtask/allowlist.toml`. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Rust source is read only as `syn` ASTs, never as text or regexes. Other inputs: Cargo metadata, this crate's own TOML config, rustc dep-info files (the walker backstop), and `git show`/`git log` for the exemption ratchet (decision 0001, amendment).
- Every violation message says what to do next.
- A new check is shown to fire (break the rule on purpose, watch it fail) before it is trusted.

## Internal modules

- `main.rs`: CLI, metadata, dependency and workspace checks.
- `golden.rs`: deterministic golden replay.
- `module_size.rs`: config, calibration table, exemption checks and `--tighten-baseline`.
  - `module_size/walk.rs`: `syn` AST traversal, test-only cfg exclusion, `#[path]`/`include!` refusal.
  - `module_size/depinfo.rs`: rustc dep-info backstop for compiled files the walker missed.
  - `module_size/ratchet.rs`: exemption-growth check against `origin/main` and the `Baseline-growth:` trailer.

`cargo xtask check --tighten-baseline` removes stale exemptions and lowers ceilings to actual
counts; it never raises them. Cap, exemption-shape and walker findings (`#[path]`, `include!`,
dep-info, build failure) are report-only until
`module-size.toml` enables enforcement; baseline growth always blocks without an authorized
`Baseline-growth: s2w#<N>` commit trailer in `origin/main..HEAD`. CI needs full git history.
