# xtask

The workspace's fitness functions: `cargo xtask check`, `cargo xtask scale` for the
Valgrind-measured scale numbers (s2w#32), and `cargo xtask h-measure` for grading a stream
mapping against an answer key (s2w#56).

## Allowed dependencies

- `serde`, `serde_json`, `toml`, `syn`, `proc-macro2`, `quote` (check 19 renders `syn`
  nodes as token text through `ToTokens`), `s2w-core` (the golden replay check folds
  the golden log), `s2w-model` (the obfuscation replay's engine-layer coverage builds
  `RawEvent`s), `s2w-system1` (same check, runs the golden log through `JsonClaimsEngine`; check
  11 runs a recorded raw stream through `MappingEngine`), `s2w-discover` (check 12 profiles a
  recorded raw stream twice), `s2w-sources` (checks 12 and 13 and `cargo xtask scale` cut a
  recorded stream into frames with the live SSE adapter's `replay_frames`, s2w#174), `sha2`
  (the crate-root `sha256` helper: `h_measure/pins.rs` checks answer keys and corpora against
  their sha256 pins, s2w#56; `contract_frozen.rs` hashes the signed evaluation contract, s2w#59)

The enforced list is `xtask/allowlist.toml`. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Rust source is read only as `syn` ASTs, never as text or regexes. Other inputs: Cargo metadata, this crate's own TOML config, rustc dep-info files (the walker backstop), `git show`/`git log` for the exemption and scale-baseline ratchets (decision 0001, amendment), and the JSON the scale measurements print or write (s2w#32).
- A measurement that cannot be read (no JSON line, a failed run, a stale or missing summary, a zero) is a failure, never a pass.
- Every violation message says what to do next.
- A new check is shown to fire (break the rule on purpose, watch it fail) before it is trusted.

## Internal modules

- `main.rs`: CLI, Cargo metadata, the shared helpers (`cargo`, `read_toml`, `crate_dir`,
  `sha256`) and the check orchestrator, including check 3 (AGENTS.md).
- `deps.rs`: the allowlist types, the per-crate edge check (check 1) and the README stack-table
  reader (check 2); `main.rs` still runs the allowlist's cross-crate loops and the row lookups.
- `lints.rs`: the manifest `[lints]` type `main.rs` reads for check 4, and check 5 (no
  dependency overrides).
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
  fixture (`crates/s2w-discover/testdata/recorded.raw.sse`, a link) with check 11's maps, except
  that RFC 3339 date-times are shifted by one constant instead of hashed (`Stamps::Shift`,
  decision 0030; check 11 still hashes every string), comparing the mapping, the event-type field
  and every path's role (decision 0022). Self-tests live in `discover_replay/tests.rs`.
- `module_size.rs`: config, calibration table, exemption checks and `--tighten-baseline`.
  `xtask/module-size.toml` keys are Rust module paths (`s2w_app::query::http`), never file
  paths; a bin target sharing its package's lib crate name keys as `name[bin]` (s2w#66). Each
  `[[exempt]]` row's `reason` and `issue` are fields, not comments: `--tighten-baseline`
  rewrites the file through serde and drops comments. Enforced since s2w#66 PR 4: a PR that
  shrinks an exempt module runs `--tighten-baseline` and commits the result; one that grows it
  raises the row's `lines` in a commit carrying the `Baseline-growth:` trailer. Burn-down: s2w#378.
  - `module_size/walk.rs`: `syn` AST traversal, test-only cfg exclusion, `#[path]`/`include!` refusal (`include_str!`/`include_bytes!` are expressions that add no module line and are not refused, s2w#66).
  - `module_size/depinfo.rs`: rustc dep-info backstop for compiled files the walker missed.
    Every walked target of a package is checked against the union of the package's walks: a
    lib+bin package's bin dep-info lists the lib's sources, and both targets share the
    package's name (s2w#66).
  - `module_size/ratchet.rs`: exemption-growth check against `origin/main` and the `Baseline-growth:` trailer; its `git` and `trailer` helpers are shared with `scale.rs`.
- `scale.rs`: `xtask/scale-baseline.toml` (every key required), the recorded fixture's pin check (`[recorded]` FNV-1a 64, `[ir.recorded] events` and `[parse] events`), the pure `[ir]`/`[parse]` and `[memory]` judges, the gungraun summary reader, the scale-baseline growth check and `[memory]` tightening (s2w#32, decision 0004).
  - `scale/supply.rs`: the two event supplies each scale number is measured on, the seeded generator (`[ir]`, `[memory]`) and the recorded fixture (`[ir.recorded]`, `[memory.recorded]`), gated side by side (s2w#174).
  - `scale/ir_bench.rs`: the three gated instruction counts (fold on each supply, and System 1's parse of the recorded fixture, s2w#166), each with its table, key and gungraun summary path.
- `expect_count.rs`: check 18, a shrink-only per-lint count of `#[expect]` attributes in every
  workspace package's `*.rs` files, tests and benches included, against
  `xtask/expect-baseline.toml` (s2w#156). Walks the package directories itself, because
  `module_size::walk` skips test targets and `vocabulary.rs`'s walker skips `tests/` and
  `benches/`; reads each file as a `syn` AST (`cfg_attr` arms
  included); reuses `module_size::{git, trailer}` for the baseline-growth rule. Self-tests live
  in `expect_count/tests.rs`, whose sample sources spell the attribute `EXPECT` so a text search
  never counts them.
- `public_api.rs`: check 19 and `cargo xtask api [--update]` (s2w#68): every lib crate's `pub`
  items (from the `module_size::walk` ASTs, test code skipped) match `xtask/public-api/<crate>.txt`.
  After an intended API change run `cargo xtask api --update` and commit the snapshot diff; there
  is no exemption and no growth rule. `public_api/render.rs` turns items into sorted
  `<module>: <tokens>` lines (bodies, docs, lint attributes and const values dropped; derives,
  `cfg`, `repr`, `non_exhaustive`, `must_use`, `doc(hidden)` kept; trait impls by header plus
  associated types and consts, unless the self type is a private local type; private `type`
  aliases marked `(private alias)`). Stable `syn`, not nightly rustdoc JSON, so it is sound only
  because the workspace forbids `unreachable_pub`. **Blind spots, accepted** (karpathy ruling on
  s2w#68): a `use` or re-export path change that re-points a spelled name, and re-exports are
  recorded as their `pub use` line rather than expanded; auto traits (`Send`, `Sync`, ...);
  blanket impls; items made by macros other than `#[derive]` (item-level `macro_rules!` calls,
  attribute macros) and `include!`d code; `#[macro_export]` macro bodies (name only); and
  `const`/`static` values. Self-tests in `public_api/tests.rs`.
- `readme_scale.rs`: check 17, the README Scale row's figures equal `xtask/scale-baseline.toml` (s2w#324). Reads the row's prose (number before a fixed phrase, per supply segment); a phrase it cannot find fails.
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
  - `h_measure/key.rs`: the answer-key spec, format versions 0, 1 and 2 (`decode`, `types` with
    mention rules `{path, identity}`, `unscored`; format 1 adds an optional `no_identity` list of
    sentinel values on a mention rule whose path is an identity path: a record holding one there
    has no mention, compared as key parts; format 2 adds the `unscored` prefix form, s2w#224),
    its fail-closed validation, `from_mapping` (writes the newest format), and `oracle`: the
    oracle-v0 mapping for the key, a reference not a proven best (one rule per mention rule
    whose path is an identity path, mention path last; alias mentions get no rule, and two mention rules on one
    multi-path identity are split by the reordering), graded as the ceiling row.
    Domain knowledge lives in the spec file, never here.
    - `h_measure/key/unscored.rs`: `unscored` entries, an exact path (every format) or
      `{"prefix": path}` (format 2: the path and every path under it), and `Unscored`, the
      mention path ids `score` drops predictions by. Prefixes match whole `rule_id` segments.
      Validation refuses a prefix before format 2, an entry listed twice, and an entry at or
      under another entry's prefix. Self-tests in `key/unscored/tests.rs`.
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
    have recorded; other rows are ignored; a corpus whose role went from `reserved` to
    `heldout` with the same file, events and sha256 is an opened span, not a change, s2w#277),
    and a file that is not what `freeze` (via
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
    a span changes only its role, and `score` accepts a freeze made before the opening
    (s2w#277). Before that fix #244 re-froze v4 under the new pins to score it
    (`h-lite-v4.dev-N.pins-244.json`); those files stay as the record of what #244 scored.
  - The ceiling row is `KeySpec::oracle()`'s mapping, not the best v0 mapping for the key: a
    mapping that joins two alias paths holding equal values can beat it on that type (H-lite
    does on the plain key's `wiki`). Reports and notes call it the oracle-v0 ceiling (#245).
- `scale_mem_check.rs`: check 13, heap bytes per entity. It spawns a nested `cargo test -p s2w-app --test scale_mem -- --ignored --exact …` once per event supply (s2w#174) and needs the JSON line each test prints. It does not check the fixture against the baseline's `[recorded] fixture_fnv1a64`: the recorded test is protected by the same pin compiled into `crates/s2w-app/tests/support/recorded.rs` (`FIXTURE_HASH`, checked by `load()`). Keep the two values equal; `cargo xtask scale` checks the baseline key.
- `decision_numbers.rs`: check 14, no two `docs/decisions/` files share a numeric prefix (`0021-x.md` and `21-y.md` count as the same number); the failure names every file holding it. A missing directory fails.
- `private_capture.rs`: check 20, no private-stream capture in the repository (s2w#371): no `*.sse` or `*.provenance.jsonl` under `research/` except the synthetic fixture, which must start with the synthetic header, and no working-tree file (outside `.git`, `target`, `node_modules`) with a line starting with the private capture header. An unreadable file or a missing `research/` fails.
- `contract_frozen.rs`: check 16, the signed `docs/evaluation-contract.md` only grows by dated
  notes. Every byte above `## Dated notes after sign-off` must hash to `SIGNED_SHA256`; changes
  at or below that heading are free. The pin is in the source rather than a tagged commit so the
  check needs no git history; `.gitattributes` pins the file to LF. A missing file or heading fails. Update the pin only when Dave
  re-signs the contract (s2w#59).
- `module_cycles.rs`: check 15, no dependency cycle between the modules of one crate target
  (s2w#67, plan #44 §1c). Edges run leaf to leaf, from the module naming a path to the module
  that defines the item; an edge to or from an ancestor is containment and is dropped. Tarjan
  SCCs are taken on the leaf graph and again on the graph projected onto each depth's
  ancestors, so a cycle that closes through an item defined in a `mod.rs` is seen. Test-only
  code is skipped. Paths that exist only after macro expansion are an accepted gap; macro
  bodies that parse as comma-separated expressions are read. A cycle fails the check, enforced since s2w#240
  (`s2w_app`) and s2w#241 (`h_measure`) broke their cycles. Self-tests in `module_cycles/tests.rs`.
  - `module_cycles/resolve.rs`: per-module items, `use` entries and paths from the walker's
    ASTs (`module_size::walk::Scan::asts`), and name resolution through `use`/`pub use` and
    globs to the defining module. Extern crates (Cargo metadata), prelude names, primitives,
    generic parameters, `Self`, leading `::` and extern-crate globs are not this crate's; any
    other multi-segment path that does not resolve is a finding.
- `scale_run.rs`: `cargo xtask scale`. Preflight (`valgrind` and `gungraun-runner` on PATH, the runner at the `gungraun` pin in `crates/s2w-app/Cargo.toml`; missing is a failure with the install command), then `cargo bench -p s2w-app --bench scale_ir` from a deleted output directory after checking the recorded fixture's pin, the `[ir]`, `[ir.recorded]` and `[parse]` judgments, and the `scale_wall` append rate, which must run (a failed run or unreadable JSON line fails) but whose value is reported, not judged; on tmpfs it prints the bench's own `warning` field. Linux only; CI job `scale`. The `[ir]` and `[parse]` baselines belong to that job's image.

`cargo xtask check --tighten-baseline` removes stale exemptions and lowers ceilings to actual
counts, and also rewrites `[memory]` in `xtask/scale-baseline.toml` down to the measurement;
it never raises anything and never touches `[ir]` or `[parse]`. Each ratchet refuses to tighten over its own findings only, and the refusal carries those findings' severity (s2w#192): module-size findings leave `module-size.toml` untouched, and since module sizes are enforced (s2w#66) that refusal blocks and the run exits non-zero. The asymmetry is deliberate: `[memory]` is
measured by `cargo xtask check` on any machine, so tightening it is automatic, while `[ir]` is
owned by the CI image, so an `[ir]` or `[parse]` improvement past tolerance stays a printed hint to lower
`fold_ir_per_event` or `parse_ir_per_event` by hand from the CI job's number. Cap, exemption-shape and walker findings (`#[path]`, `include!`,
dep-info, build failure) block, because `module-size.toml` has `enforce = true` since s2w#66
PR 4; baseline growth always blocks without an authorized
`Baseline-growth: s2w#<N>` commit trailer in `origin/main..HEAD`; the same trailer rule covers
raising `fold_ir_per_event`, `parse_ir_per_event`, any events or entities size, `bytes_per_entity`, `target_bytes_per_entity`,
`budget_bytes_per_entity` or `tolerance_percent` in `xtask/scale-baseline.toml` (a file absent on `origin/main` is all growth). CI needs full git history.
- No domain knowledge in this crate; see decision 0018.
