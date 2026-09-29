# xtask

The workspace's fitness functions: `cargo xtask check`, and `cargo xtask scale` for the
Valgrind-measured scale numbers (s2w#32).

## Allowed dependencies

- `serde`, `serde_json`, `toml`, `syn`, `proc-macro2`, `s2w-core` (the golden replay check folds
  the golden log), `s2w-model` (the obfuscation replay's engine-layer coverage builds
  `RawEvent`s), `s2w-system1` (same check, runs the golden log through `JsonClaimsEngine`; check
  11 runs a recorded raw stream through `MappingEngine`), `s2w-discover` (check 12 profiles a
  recorded raw stream twice), `s2w-sources` (checks 12 and 13 and `cargo xtask scale` cut a
  recorded stream into frames with the live SSE adapter's `replay_frames`, s2w#174)

The enforced list is `xtask/allowlist.toml`. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Rust source is read only as `syn` ASTs, never as text or regexes. Other inputs: Cargo metadata, this crate's own TOML config, rustc dep-info files (the walker backstop), `git show`/`git log` for the exemption and scale-baseline ratchets (decision 0001, amendment), and the JSON the scale measurements print or write (s2w#32).
- A measurement that cannot be read (no JSON line, a failed run, a stale or missing summary, a zero) is a failure, never a pass.
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
- `discover_replay.rs`: check 12, profiler obfuscation replay of `s2w-discover` over the recorded
  fixture (`crates/s2w-discover/testdata/recorded.raw.sse`, a link) with check 11's maps
  (decision 0022). Self-tests live in `discover_replay/tests.rs`.
- `module_size.rs`: config, calibration table, exemption checks and `--tighten-baseline`.
  - `module_size/walk.rs`: `syn` AST traversal, test-only cfg exclusion, `#[path]`/`include!` refusal.
  - `module_size/depinfo.rs`: rustc dep-info backstop for compiled files the walker missed.
  - `module_size/ratchet.rs`: exemption-growth check against `origin/main` and the `Baseline-growth:` trailer; its `git` and `trailer` helpers are shared with `scale.rs`.
- `scale.rs`: `xtask/scale-baseline.toml` (every key required), the recorded fixture's pin check (`[recorded]` FNV-1a 64 and `[ir.recorded] events`), the pure `[ir]` and `[memory]` judges, the gungraun summary reader, the scale-baseline growth check and `[memory]` tightening (s2w#32, decision 0004).
  - `scale/supply.rs`: the two event supplies each scale number is measured on, the seeded generator (`[ir]`, `[memory]`) and the recorded fixture (`[ir.recorded]`, `[memory.recorded]`), gated side by side (s2w#174).
- `scale_mem_check.rs`: check 13, heap bytes per entity. It spawns a nested `cargo test -p s2w-app --test scale_mem -- --ignored --exact …` and needs the JSON line that test prints.
- `decision_numbers.rs`: check 14, no two `docs/decisions/` files share a numeric prefix (`0021-x.md` and `21-y.md` count as the same number); the failure names every file holding it. A missing directory fails.
- `scale_run.rs`: `cargo xtask scale`. Preflight (`valgrind` and `gungraun-runner` on PATH, the runner at the `gungraun` pin in `crates/s2w-app/Cargo.toml`; missing is a failure with the install command), then `cargo bench -p s2w-app --bench scale_ir` from a deleted output directory, the `[ir]` judgment, and the `scale_wall` append rate, which must run (a failed run or unreadable JSON line fails) but whose value is reported, not judged; on tmpfs it prints the bench's own `warning` field. Linux only; CI job `scale`. The `[ir]` baseline belongs to that job's image.

`cargo xtask check --tighten-baseline` removes stale exemptions and lowers ceilings to actual
counts, and also rewrites `[memory]` in `xtask/scale-baseline.toml` down to the measurement;
it never raises anything and never touches `[ir]`. Each ratchet refuses to tighten over its own findings only, and the refusal carries those findings' severity: report-only module-size findings leave `module-size.toml` untouched with a `[report-only]` line while `[memory]` still tightens and the run exits 0 (s2w#192). The asymmetry is deliberate: `[memory]` is
measured by `cargo xtask check` on any machine, so tightening it is automatic, while `[ir]` is
owned by the CI image, so an `[ir]` improvement past tolerance stays a printed hint to lower
`fold_ir_per_event` by hand from the CI job's number. Cap, exemption-shape and walker findings (`#[path]`, `include!`,
dep-info, build failure) are report-only until
`module-size.toml` enables enforcement; baseline growth always blocks without an authorized
`Baseline-growth: s2w#<N>` commit trailer in `origin/main..HEAD`; the same trailer rule covers
raising `fold_ir_per_event`, `bytes_per_entity`, `target_bytes_per_entity`,
`budget_bytes_per_entity` or `tolerance_percent` in `xtask/scale-baseline.toml` (a file absent on `origin/main` is all growth). CI needs full git history.
- No domain knowledge in this crate; see decision 0018.
