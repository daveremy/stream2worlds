# xtask

The workspace's fitness functions: `cargo xtask check`, `cargo xtask scale` for the
Valgrind-measured scale numbers (s2w#32), and `cargo xtask h-measure` for grading a stream
mapping against an answer key (s2w#56).

## Allowed dependencies

- `serde`, `serde_json`, `toml`, `syn`, `proc-macro2`, `s2w-core` (the golden replay check folds
  the golden log), `s2w-model` (the obfuscation replay's engine-layer coverage builds
  `RawEvent`s), `s2w-system1` (same check, runs the golden log through `JsonClaimsEngine`; check
  11 runs a recorded raw stream through `MappingEngine`), `s2w-discover` (check 12 profiles a
  recorded raw stream twice), `s2w-sources` (checks 12 and 13 and `cargo xtask scale` cut a
  recorded stream into frames with the live SSE adapter's `replay_frames`, s2w#174), `sha2`
  (`h_measure/pins.rs` checks answer keys and corpora against their sha256 pins, s2w#56)

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
  stream and two mappings, version 1 and a linked version 2 whose merges must change the world
  (decisions 0021, 0027). Self-tests live in `obfuscation_raw/tests.rs`.
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
- `h_measure.rs`: `cargo xtask h-measure selftest | freeze | score` (s2w#56, contract B3). The selftest runs the mapping
  executor over `crates/s2w-system1/testdata/raw-sample.jsonl` with `sample.mapping.json` and
  checks it against `MappingEngine` (every predicted cluster is an entity the engine proposes
  for that record, and the reverse), then reads the mapping as its own key spec and checks the
  key executor gives the same partition. An empty result fails. It then grades the mapping
  against its own key (mapping and oracle ceiling must both score 1.0) and scores and prints the
  contract's frozen fixtures (the 4/9 case, all-singletons). Self-tests live in
  `h_measure/tests.rs`, which also runs the selftest, so `cargo test` enforces parity.
  The measurement's data (answer keys, `keys.toml` pins, the corpus manifest) lives in
  `research/h-measure/`; a test in `h_measure/tests.rs` checks that every key `keys.toml` pins
  parses, validates and yields an oracle mapping.
  - `h_measure/key.rs`: the answer-key spec, format versions 0 and 1 (`decode`, `types` with
    mention rules `{path, identity}`, `unscored`; format 1 adds an optional `no_identity` list of
    sentinel values on a mention rule whose path is an identity path: a record holding one there
    has no mention, compared as key parts), its fail-closed validation, `from_mapping` (writes
    the newest format), and
    `oracle`: the oracle-v0 mapping for the key, a reference not a proven best (one rule per mention rule whose path is an
    identity path, mention path last; alias mentions get no rule, and two mention rules on one
    multi-path identity are split by the reordering), graded as the ceiling row.
    Domain knowledge lives in the spec file, never here.
  - `h_measure/mentions.rs`: the key and mapping executors. A mention is `(record index,
    s2w_discover::rule_id(path))`. A mapping rule mentions its entity at its **last** key path:
    a composite key lists context parts first, and the context usually keys a type of its own.
    Two rules that place one mention in different clusters are an error naming both. Both
    build keys with `s2w_system1::decode::natural_key`, the engine's own builder, never a copy.
    `key_mentions` validates its spec first, reports as abstained paths the mentions it
    skips because an identity path held no key part, and as excluded mentions the ones whose
    path held a `no_identity` value. `Decoded` applies one list of decode steps
    to a corpus once; every executor whose steps match shares it.
  - `h_measure/score.rs`: B-cubed P/R/F1 (micro, per type, and without singleton-only types),
    the mention-weighted false-merge rate, entity recovery (≥ 90% both ways, integer
    comparisons), the per-path table and spurious mentions. A zero denominator is `None`
    (undefined). Fixtures live in `h_measure/score/tests.rs`.
  - `h_measure/grade.rs`: `grade` scores a mapping and the oracle ceiling on one corpus,
    dropping the key's excluded mentions from both predictions first (the key has no mention
    there; a v0 mapping cannot exclude a value), and adds the `context` rows. It sits above
    both `score` and `context` so neither imports the other (s2w#241).
  - `h_measure/pins.rs`: reads `research/h-measure/keys.toml` and `corpora.toml` (with each
    corpus's `role`: development, heldout, reserved). A key or corpus that is not pinned, or
    whose sha256 does not match its pin, is refused, as is a corpus whose SSE frame count is not
    its pinned `events`.
  - `h_measure/freeze.rs`: `cargo xtask h-measure freeze --corpus NAME --window N --out FILE
    [--dir DIR]`. Refuses an `--out` that exists, checks every pinned key, refuses any corpus
    whose role is not `development`, checks the corpus against its pin, then profiles the first N events and writes
    the mapping (or the abstain reason) with the corpus hash, window, profiler version, config,
    every pin (a corpus pin includes its role, file and event count), and the profile's abstained paths by role. Refusals are tested in
    `h_measure/freeze_tests.rs` on a temporary root.
  - `h_measure/report.rs`: `cargo xtask h-measure score --mapping FROZEN --corpus NAME --key
    FILE... [--json FILE] [--dir DIR]`. Refuses, before reading the scored corpus: a mapping
    not frozen on the pinned development corpus, a `reserved` corpus, no key, a key given
    twice, an unpinned or mismatched key, a change since the freeze to a pin it uses (the
    freeze corpus, the scored corpus if the freeze recorded it, each key, which the freeze must
    have recorded; other rows are ignored), and a file that is not what `freeze` (via
    `freeze::derive`) writes for its recorded corpus and window under this build (s2w#238).
    That last check re-reads the development corpus. An abstention is graded as the
    empty prediction. Writes a markdown report (alias limit first; per key the mapping and
    ceiling rows, per type, per path, context collisions, spurious, abstained and excluded
    counts) and, with `--json`, every number. Refusals are tested in
    `h_measure/report_tests.rs`.
  - `h_measure/context.rs`: the context-collision rows (the unfloored composite-key
    sub-metric; definition in `research/h-measure/README.md`), added to `Grade.contexts`.
    Fixtures live in `h_measure/context/tests.rs`.
  - The first pre-registered run (s2w#56 PR 3, research 0009): frozen files in
    `research/h-measure/frozen/`, reports in `research/h-measure/results/`. `score` re-runs the
    freeze, so a later `PROFILER_VERSION` or `Config` change, or any `s2w-discover` change that
    alters the mapping written, makes those files refuse to score; that is intended (the commit that froze them names the build). A new profiler version
    is frozen to a new file and scored on a span pinned before it and never read, never on the
    spans 0009 already read (#244 used `reserved`, #250 `reserved-2` and `reserved-3`). Opening
    a span changes its pin, so a freeze made before the opening must be re-frozen under the
    current pins to score it (#244: `h-lite-v4.dev-N.pins-244.json`).
  - The ceiling row is `KeySpec::oracle()`'s mapping, not the best v0 mapping for the key: a
    mapping that joins two alias paths holding equal values can beat it on that type (H-lite
    does on the plain key's `wiki`). Reports and notes call it the oracle-v0 ceiling (#245).
- `scale_mem_check.rs`: check 13, heap bytes per entity. It spawns a nested `cargo test -p s2w-app --test scale_mem -- --ignored --exact …` once per event supply (s2w#174) and needs the JSON line each test prints. It does not check the fixture against the baseline's `[recorded] fixture_fnv1a64`: the recorded test is protected by the same pin compiled into `crates/s2w-app/tests/support/recorded.rs` (`FIXTURE_HASH`, checked by `load()`). Keep the two values equal; `cargo xtask scale` checks the baseline key.
- `decision_numbers.rs`: check 14, no two `docs/decisions/` files share a numeric prefix (`0021-x.md` and `21-y.md` count as the same number); the failure names every file holding it. A missing directory fails.
- `module_cycles.rs`: check 15, no dependency cycle between the modules of one crate target
  (s2w#67, plan #44 §1c). Edges run leaf to leaf, from the module naming a path to the module
  that defines the item; an edge to or from an ancestor is containment and is dropped. Tarjan
  SCCs are taken on the leaf graph and again on the graph projected onto each depth's
  ancestors, so a cycle that closes through an item defined in a `mod.rs` is seen. Test-only
  code is skipped. Paths that exist only after macro expansion are an accepted gap; macro
  bodies that parse as comma-separated expressions are read. Report-only (`ENFORCE`) until
  s2w#240 (`s2w_app`) and s2w#241 (`h_measure`) break their cycles. Self-tests in `module_cycles/tests.rs`.
  - `module_cycles/resolve.rs`: per-module items, `use` entries and paths from the walker's
    ASTs (`module_size::walk::Scan::asts`), and name resolution through `use`/`pub use` and
    globs to the defining module. Extern crates (Cargo metadata), prelude names, primitives,
    generic parameters, `Self`, leading `::` and extern-crate globs are not this crate's; any
    other multi-segment path that does not resolve is a finding.
- `scale_run.rs`: `cargo xtask scale`. Preflight (`valgrind` and `gungraun-runner` on PATH, the runner at the `gungraun` pin in `crates/s2w-app/Cargo.toml`; missing is a failure with the install command), then `cargo bench -p s2w-app --bench scale_ir` from a deleted output directory after checking the recorded fixture's pin, the `[ir]` and `[ir.recorded]` judgments, and the `scale_wall` append rate, which must run (a failed run or unreadable JSON line fails) but whose value is reported, not judged; on tmpfs it prints the bench's own `warning` field. Linux only; CI job `scale`. The `[ir]` baseline belongs to that job's image.

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
