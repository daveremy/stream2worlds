**Verdict: REQUEST CHANGES.** The dependency gate is a useful foundation, but the ratchet has routine Rust-syntax bypasses, and the purity guarantees exceed what Clippy enforces.

Review based on the supplied diff and official Cargo/Clippy documentation. I checked scanner examples with an in-memory translation; no Rust toolchain was available to compile them. Line numbers refer to the new files.

1. **P1 — `cfg(test)` stops scanning unrelated production code.**  
   `xtask/src/main.rs:292–293`

   This skips the remainder of the file, not the attributed item. For example:

   ```rust
   #[cfg(test)]
   const _: () = ();

   /// Extract a previously validated value.
   #[allow(clippy::unwrap_used, reason = "validated upstream")]
   pub fn value(x: Option<u64>) -> u64 {
       x.unwrap()
   }
   ```

   The scanner reports **zero for all four metrics**. The reason satisfies the configured allowance lint. Clippy’s `items_after_test_module` does not address this example: the attributed item is a constant.

   The opposite bug also exists: with `#[cfg(test)] mod tests;`, the directory walker scans `tests.rs` independently and counts its permitted unwraps.

   **Fix:** Parse items and propagate test-only status through their scopes and external modules. Skip the test item, then continue scanning its siblings.

2. **P1 — Text matching misses allowances, calls, visibility changes, and macro expansion.**  
   `xtask/src/main.rs:298–306`

   Concrete holes:

   - `#[cfg_attr(target_pointer_width = "64", allow(..., reason = "..."))]` adds an effective allowance without incrementing `allow`.
   - `Option::unwrap(value)` and `Result::expect(value, "...")` do not increment `unwrap`. Clippy can independently catch calls, but combining them with the preceding conditional allowance defeats that protection on the matching target.
   - `pub(crate)` and `pub(super)` are invisible. This directly permits the visibility widening described in decision 0001.
   - `pub use s2w_model::*;` in `s2w-core` exports the model’s API while keeping its public-item count at zero. `pub mod` is explicitly excluded too.
   - A macro with `$visibility:vis` can generate public items from `make_item!(pub Example)` without a counted `pub` declaration.
   - `unimplemented! { "later" }` is invisible. The workspace denies `clippy::todo`, but does not enable `clippy::unimplemented`, which is allow-by-default. [Clippy lint reference](https://rust-lang.github.io/rust-clippy/stable/index.html#unimplemented)

   There are false positives too: `let text = ".unwrap()";` increments the counter. Ordinary multiline `#[allow(\n...)]` **is counted**; splitting the opening tokens is what defeats that particular matcher, although formatting may normalize some such cases.

   **Fix:** Use a Rust parser for attributes—including nested `cfg_attr`—calls, macro invocations, and visibility. Define whether restricted visibility, re-exports, fields, and implicitly public trait members belong in the metric. Enable `unimplemented = "deny"`. An AST alone does not solve generated code: either inspect expansion or explicitly constrain and review generators.

3. **P1 — The pure-crate configuration does not enforce the stated purity contract.**  
   `crates/s2w-{core,model}/clippy.toml:5–14`; `docs/decisions/0001-workspace-layers.md:15–16`

   An existing function can add:

   ```rust
   let _ = std::fs::read("/etc/hostname");
   ```

   This performs I/O without explicitly using any banned type or method and without changing a ratchet metric. Environment reads, printing, process execution, UDP, and ordinary `async` code are also outside the list. `Instant::elapsed()` and `SystemTime::elapsed()` read clocks without calling the prohibited `now()` methods in source.

   **Fix:** Describe these rules as checks against selected APIs, rather than enforcement of complete purity. Expand the common prohibited APIs and add negative fixtures. If complete purity is essential, establish a substantially narrower capability boundary; a seven-entry denylist cannot provide it.

4. **P2 — Source discovery omits compiled code and silently treats traversal failures as success.**  
   `xtask/src/main.rs:278–287`

   Only `<crate>/src/**/*.rs` is scanned. This excludes:

   - `build.rs`;
   - custom `[lib]`/`[[bin]]` paths;
   - modules or `include!` files outside `src`;
   - generated source.

   Moving a library to a configured root-level `lib.rs` can make its count zero. A build script can contain reasoned allowances and unwraps that the ratchet never sees. Build **dependencies** are checked separately; that does not cover build-script source.

   `read_dir` errors are discarded, and `entries.flatten()` drops individual entry errors. An incomplete scan can therefore return a successful, artificially low count.

   **Fix:** Discover source roots from Cargo targets, follow modules with their attributes, and explicitly handle or reject unsupported source generation/layouts. Propagate traversal errors. Bound or reject symlink traversal.

5. **P2 — Dependency identity is reduced to package name; path does not mean workspace member.**  
   `xtask/src/main.rs:75–79, 154–158`

   An allowed `s2w-model` edge need not point to the actual workspace model. It can point to an excluded/out-of-workspace package with the same name. The comparison still accepts its name, its path prevents classification as external, and `--no-deps` excludes that package’s own dependency graph from inspection.

   Similarly, changing an approved external dependency to a same-named Git fork preserves the checked name.

   **Fix:** Require internal edges to resolve to the corresponding workspace member’s manifest/package identity. Classify external dependencies by absence from that member set, not by `path.is_none()`. Decide explicitly whether external source changes require approval.

   **The listed Cargo declaration edge cases mostly work correctly:** renamed dependencies retain their actual package name in metadata; target-specific, inherited, optional, and build dependencies appear in package dependency declarations. A new unapproved package name in those forms is caught. The checker does collapse multiple declarations of the same name and kind, so it does not distinguish their targets, features, versions, or sources. [Cargo metadata documentation](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html)

6. **P2 — The README check verifies labels and substrings, not the claimed documentation relationship.**  
   `xtask/src/main.rs:185–190, 203–212, 248–255`

   Remove `serde`, `serde_json`, and `thiserror` from the “Serialization and errors” row: the check still succeeds because the row label remains. An external dependency can point to any existing row regardless of its contents.

   Likewise, remove the standalone `s2w` binary from the Workspace row: `stack.contains("s2w")` still succeeds because names such as `s2w-model` contain it.

   **Fix:** Parse the table into row labels and cells. Require each external package’s exact name in its mapped row, and each workspace package’s exact token in the Workspace row. The current README promises more than row-label existence.

7. **P2 — The ratchet is a mutable ceiling, not “counts may only fall.”**  
   `xtask/src/main.rs:331–349, 356–384`

   Three distinct problems:

   - A count can fall from seven to five, merge without tightening, then rise back to seven without a record.
   - `ratchet --raise README.md` satisfies the record check. So does any existing file, including an unrelated or previously accepted record.
   - Editing `ratchet.toml` directly bypasses `--raise`; `check` never verifies that a baseline increase has an associated decision.

   **Fix:** Require committed baselines to reflect decreases. In CI, compare baseline increases against the PR base and require a tracked decision reference for those increases. Validate references beneath `docs/decisions/`; leave assessment of the record’s substance to review.

   Also replace `read_toml(&path).unwrap_or_default()` at line 360 with error propagation: malformed baselines should not be silently replaced. Preserve raise provenance when tightening; line 384 currently removes it.

8. **P2 — Workspace lint inheritance is optional and is not checked.**  
   `Cargo.toml:18–28`; `xtask/src/main.rs:133–193`

   Current members opt in, but a new member—or an edited existing one—can omit `[lints] workspace = true`. The fitness checks do not notice. This removes `unsafe_code = "forbid"` and the selected Clippy restrictions without adding a Rust allowance attribute.

   **Fix:** Validate lint inheritance in every workspace manifest, with explicit exceptions if needed. Cargo documents workspace lint inheritance as opt-in. [Cargo workspace lints](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-lints-table)

9. **P3 — “AGENTS.md in every crate” explicitly excludes xtask.**  
   `xtask/src/main.rs:179–180`; `xtask/`

   The tooling crate has neither the file nor enforcement, despite the decision record saying every crate. This is particularly useful documentation for the crate implementing these checks.

   **Fix:** Add `xtask/AGENTS.md` and remove the exception, or document the narrower policy. Empty files passing is not itself a presence-check bug; validating their content would be a separate requirement.

The **per-crate Clippy lookup is correct** under normal `cargo clippy --workspace` execution: lookup starts at `CARGO_MANIFEST_DIR`, unless `CLIPPY_CONF_DIR` overrides it, then walks upward. The nearest file replaces rather than merges with the root configuration. Both disallowed lints are warn-by-default, so CI’s `-D warnings` makes them failures. [Configuration lookup](https://doc.rust-lang.org/clippy/configuration.html), [disallowed methods](https://raw.githubusercontent.com/rust-lang/rust-clippy/master/clippy_lints/src/disallowed_methods.rs), [disallowed types](https://raw.githubusercontent.com/rust-lang/rust-clippy/master/clippy_lints/src/disallowed_types.rs)

For a skeleton, the **public-item ratchet is the most over-engineered part**: zero caps force decision records for ordinary implementation of already-approved APIs, while aggregate counts poorly measure architectural drift. I would keep the dependency boundaries, documentation checks, and focused compiler lints; defer the public-API ratchet or explicitly narrow its purpose. The crate split is defensible given the recorded layer constraints.