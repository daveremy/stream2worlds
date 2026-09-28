# 0013: Local embeddings engine — `model2vec-rs`, vendored, `enwiki` only

Date: 2026-09-27 · Status: accepted · Gate 2-3 · Issue [#64](https://github.com/daveremy/stream2worlds/issues/64) · Amends [0011](0011-system1-bridge.md)

## Decision

A third System 1 engine, `LocalEmbeddingsEngine`, classifies an `enwiki` page-change's
`revision.comment` into one of six edit categories by cosine similarity to fixed prototype
phrases, using [`model2vec-rs`](https://crates.io/crates/model2vec-rs) with
[potion-base-8M](https://huggingface.co/minishlab/potion-base-8M) (static embeddings, no ONNX
runtime, ~30 MB of weights). It abstains — never guesses — below a similarity threshold or on a
near-tie with the runner-up category.

**Split: a plain classifier, and a thin `Engine` adapter.** `s2w-system1::embedding::
CommentClassifier` has no dependency on `Engine`, `Verdict`, or `AbstainReason` — it takes a
`&str`, returns a `ClassifyResult` (`Empty`/`Invalid`/`Match`/`NoMatch`), and nothing more. A
future System 2/Jev consumer can call it directly. `engines::embeddings::LocalEmbeddingsEngine`
wraps it behind the `Engine` trait: schema/canary/language-scope/structural-field checks, then
`classify`, then translating the result into a `Verdict`.

**Vendored, not fetched at runtime.** The three model files (`config.json`, `tokenizer.json`,
`model.safetensors`, revision `bf8b056651a2c21b8d2565580b8569da283cab23`) are committed under
`crates/s2w-system1/models/potion-base-8m/` and pulled in via `include_bytes!`, once, in the
`s2w` binary. No network fetch, no first-run download flakiness, one binary artifact.

**Scoped to `enwiki` only.** potion-base-8M is an English-only static-embedding model;
`wikipedia.*` routes every language wiki. Every non-`enwiki` `wiki_id` abstains `NotMine` — the
same reason the schema/canary check already uses, not a new abstain category.

**Decision order** (schema → canary → language scope → structural fields → empty-comment →
scoring): an out-of-scope event (wrong schema, canary domain, non-`enwiki`) reports `NotMine`,
never `Insufficient`, even if it also has no comment. `NotMine` means "not this engine's input";
`Insufficient` means "this engine's input, but incomplete" — the two must never be conflated,
because `NotMine` on every non-English wiki is the expected, permanent steady state, not a
degraded one.

**Comparisons happen on rounded basis points, not raw floats.** A raw similarity score is
rounded to `u16` bps exactly once; every later comparison (against `threshold_bps`, and the
margin against `margin_bps`) is an integer comparison. A non-finite raw score is caught before
rounding and reported as `Invalid`, never silently coerced.

**A dedicated `Ambiguous` abstain reason, never `BelowThreshold` reused.** A near-tie (top1
above threshold, but too close to top2) is a different failure mode from a low top1, and
`s2w-log`'s verdict store is append-only and persisted (decision 0012) — reusing
`BelowThreshold { score_bps, threshold_bps }` for a near-tie would write a row where
`score_bps >= threshold_bps` while the reason name says "below threshold," wrong forever once
persisted. `AbstainReason::Ambiguous { top1_bps, top2_bps, required_margin_bps }` is a distinct
variant, with its own match arms in `bridge/judge.rs` and its own `AbstainCounts` field.

**Provenance covers two hashes, not one.** `provenance()` returns `{"model_hash", "config_hash"}`
as JSON bytes. `model_hash` is an FNV-1a-64 hex over the three vendored files (tokenizer,
weights, config) in that order — the whole vendored model identity, not just the weights.
`config_hash` is an FNV-1a-64 hex over the label list, each label's example phrases (in table
order), and the threshold and margin — everything besides the model files that can change a
verdict. Splitting the two means a model swap and a taxonomy/threshold tweak are independently
visible in a stored verdict's provenance.

**Version-bump enforcement is a table, not a pinned pair.** A test that only recomputes
`model_hash`/`config_hash` and compares the result to itself can never fail: changing a label,
a phrase, a threshold, or a vendored file changes both sides of the comparison identically.
`embedding::VERSION_HISTORY` is an append-only `&[(u32, &str, &str)]` table of every
`(version, model_hash, config_hash)` triple ever shipped; `CURRENT_VERSION` is derived from the
table's last row (not a separate literal), and `LocalEmbeddingsEngine::version()` returns
`CURRENT_VERSION`. The test asserts no version number maps to two different hash pairs, and
that `CURRENT_VERSION`'s row matches the running classifier's actual hashes — so updating a
pinned hash literal without appending a new row and bumping the version fails, by construction
rather than by convention.

**A golden-output test is the real regression net for what the two hashes cannot see.** Neither
hash changes when a `Cargo.lock` bump of `model2vec-rs`/`tokenizers`, or a change to
normalization or scoring code, changes a verdict. `embedding::tests::
golden_classifications_for_fixed_real_comments` pins the full expected result (label, top1_bps,
top2_bps) for 8 fixed real English edit comments. Two of the eight — a revert comment and a
`/* History */`-prefixed section edit, both plausible but not clearly worded — land in
`NoMatch`: real evidence that the abstain-first design is doing its job on real text, not proof
by construction alone.

**Threshold and margin are named as unvalidated** (`threshold_bps = 4_000`, `margin_bps = 500`).
Static-embedding cosine similarity on short text tends to cluster high; these starting values
may be too permissive or too strict against real `enwiki` traffic. Per decision 0011
("thresholds are starting points, not published values"), this is not a blocker: a threshold set
too strict costs missed classifications (silence, the safe failure mode). A threshold set too
permissive is the opposite risk and is NOT fully covered by the abstain design alone — an
unrelated comment can score above both threshold and margin and pick the wrong one of the six
fixed categories with apparent confidence; abstention guards against low-similarity noise, not
against a confident wrong pick among the categories. **Follow-up, not built in #64:** calibrate
`threshold_bps`/
`margin_bps` against a sample of real `enwiki` comments; if that needs code changes beyond the
two constants, file it as a separate issue.

**Full boilerplate-template stripping is deferred.** `normalize_comment` strips one leading
`/* Section name */` MediaWiki marker; broader template-boilerplate stripping needs a real
comment corpus to characterize patterns against, which does not exist yet. Named alongside the
calibration follow-up above, not guessed at here.

**A lightweight throughput regression guard, not a load test.** model2vec's published
benchmark is ~8,000 strings/s single-threaded (research 0006) — a number about the library, not
a measurement of this engine end to end. `embedding::tests::
encoding_a_batch_of_comments_stays_within_a_generous_throughput_budget` asserts wall-clock for
encoding 10 representative comments stays under 1 ms/string (~8x slower than the published
figure), on the same single self-hosted CI runner as everything else. It catches a future
dependency bump or normalization change quietly regressing throughput; it does not claim to
validate the 1,000 events/s target (decision 0004) end to end.

**Dependency: `model2vec-rs = "0.3.0"`, `default-features = false`, `features = ["fancy-regex"]`
— a deviation from the original plan, forced by a build error.** `default-features = false`
alone fails to compile: `tokenizers` (model2vec-rs's own dependency) hard-errors
(`compile_error!`) unless either `onig` or `fancy-regex` is enabled. `fancy-regex` was chosen
over `onig` because it is pure Rust — no C toolchain dependency, matching this workspace's
single static-binary delivery goal (see the "Language and delivery" row of the README's
Technical architecture table).

**License and advisory findings.** `cargo deny check` (all four categories) passes:
`licenses ok`, `bans ok`, `sources ok`. `advisories` needed two `deny.toml` exceptions, both
unmaintained-not-vulnerable with no safe upgrade available upstream in `tokenizers`:

- `RUSTSEC-2024-0436` (`paste`) — via `tokenizers`, model2vec-rs's own dependency.
- `RUSTSEC-2025-0119` (`number_prefix`) — via `indicatif`, pulled in by the `fancy-regex`
  feature switch above (round 2 of plan review only checked the dependency tree for
  `default-features = false` with no feature enabled, which never actually compiles; this
  advisory only became visible once `fancy-regex` was the real, compiling dependency tree).

Both are named individually in `deny.toml` with their transitive path, per the file's existing
per-exception comment style — never a blanket `ignore = true`.

## Consequences

- `s2w-system1` now depends on `model2vec-rs`, allowlisted in `xtask/allowlist.toml` for
  `s2w-system1` and named against the README's "System 1 engines" row.
- Vendoring adds ~31 MB to the `s2w` binary (the three model files via `include_bytes!`); no
  runtime network dependency, no first-run download step.
- The README's "System 1 engines" row moves from "local embeddings (next)" to built, naming
  `model2vec-rs` and potion-base-8M directly.
- Two follow-ups are named, not built here: threshold/margin calibration against real `enwiki`
  traffic, and full boilerplate-template stripping beyond the single leading `/* */` marker.
  Neither blocks merging (abstain-first design; graceful failure mode).

verify: `cargo test -p s2w-system1 embedding:: && cargo xtask check && cargo deny check`
passes.
