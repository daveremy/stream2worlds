# xtask

The workspace's fitness functions: `cargo xtask check`.

## Allowed dependencies

- `serde`, `serde_json`, `toml`, `syn`, `proc-macro2`, `s2w-core` (the golden replay check folds
  the golden log), `s2w-model` (the obfuscation replay's engine-layer coverage builds
  `RawEvent`s), `s2w-system1` (same check, runs the golden log through `JsonClaimsEngine`; check
  11 runs a recorded raw stream through `MappingEngine`)

The enforced list is `xtask/allowlist.toml`. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Rust source is read only as `syn` ASTs, never as text or regexes. Other inputs: Cargo metadata, this crate's own TOML config, rustc dep-info files (the walker backstop), and `git show`/`git log` for the exemption ratchet (decision 0001, amendment).
- Every violation message says what to do next.
- A new check is shown to fire (break the rule on purpose, watch it fail) before it is trusted.

## Internal modules

- `main.rs`: CLI, metadata, dependency and workspace checks.
- `golden.rs`: deterministic golden replay.
- `obfuscation.rs`: check 10, obfuscation replay of the golden log through the fold and the
  claim-reading engines.
- `obfuscation_raw.rs`: check 11, raw obfuscation replay of `MappingEngine` over a recorded raw
  stream and a mapping (decision 0021). Self-tests live in `obfuscation_raw/tests.rs`.
  - `obfuscation_raw/rename.rs`: applies the maps to payloads, the mapping and claims; the
    non-vacuity checks. Decodes with `s2w_system1::decode` and reads keys with
    `s2w_model::NaturalKey::parts`, never its own copy; a key that does not read fails the check.
- `clippy_config.rs`: check 8, every crate's effective clippy config carries the root size thresholds.
- `module_size.rs`: config, calibration table, exemption checks and `--tighten-baseline`.
  - `module_size/walk.rs`: `syn` AST traversal, test-only cfg exclusion, `#[path]`/`include!` refusal.
  - `module_size/depinfo.rs`: rustc dep-info backstop for compiled files the walker missed.
  - `module_size/ratchet.rs`: exemption-growth check against `origin/main` and the `Baseline-growth:` trailer.

`cargo xtask check --tighten-baseline` removes stale exemptions and lowers ceilings to actual
counts; it never raises them. Cap, exemption-shape and walker findings (`#[path]`, `include!`,
dep-info, build failure) are report-only until
`module-size.toml` enables enforcement; baseline growth always blocks without an authorized
`Baseline-growth: s2w#<N>` commit trailer in `origin/main..HEAD`. CI needs full git history.
- No domain knowledge in this crate; see decision 0018.
