1. **Moot** — Removing the scanner eliminates the `cfg(test)` truncation and test-file counting bugs.
2. **Moot** — Scanner bypasses and visibility counts are retired; `unimplemented` is now denied.
3. **Resolved** — The enforcement claim is narrowed, common prohibited APIs expanded, and printing denied.
4. **Moot** — Source discovery and traversal are no longer part of xtask.
5. **Partly** — Direct dependency declarations are checked, but Cargo overrides escape the new identity check.
6. **Resolved** — Required names must appear as exact backticked strings in their designated rows.
7. **Moot** — The mutable ratchet baseline and raise mechanism are removed.
8. **Resolved** — Every workspace manifest is checked for lint inheritance.
9. **Resolved** — xtask has an AGENTS.md and no longer receives a presence-check exemption.

New defects, ranked:

- **P1 — Inner allowances bypass the replacement escape-hatch policy.** [Cargo.toml:30](/home/dave/code/worktrees/stream2worlds-foundation/Cargo.toml:30)  
  `clippy::allow_attributes` deliberately ignores inner attributes. Consequently:
  ```rust
  #![allow(clippy::unwrap_used, reason = "validated upstream")]
  ```
  suppresses production unwrap diagnostics throughout a crate or module without an `expect` or an unfulfilled-expectation check. The reason satisfies `allow_attributes_without_reason`. This contradicts the new “only permitted exception” guarantee. Either explicitly permit reviewed inner allowances and narrow that guarantee, or enforce their exclusion with a compiler-aware check. Changing `allow_attributes` to `forbid` alone will not cover attributes the lint ignores. [Clippy documentation](https://rust-lang.github.io/rust-clippy/stable/index.html#allow_attributes)

- **P2 — The crates.io check inspects declared sources, not resolved sources.** [xtask/src/main.rs:256](/home/dave/code/worktrees/stream2worlds-foundation/xtask/src/main.rs:256)  
  A root `[patch.crates-io]` can replace `serde` with a Git fork while the workspace dependency’s `source` remains the crates.io identifier. With `--no-deps`, metadata supplies no resolution graph, so this branch accepts it. A metadata probe with a Git patch reproduced the unchanged source and `resolve: null`. Check resolved package identities alongside the declaration allowlist, or explicitly reject unsupported overrides, including configuration-based patches. [Cargo override documentation](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html#the-patch-section)

- **P2 — Reconnect checkpoints advance before the event is received completely.** [research/scripts/eventstreams_replay.py:26](/home/dave/code/worktrees/stream2worlds-foundation/research/scripts/eventstreams_replay.py:26)  
  `last_id` changes on the `id:` line, before the terminating blank line and output write. A disconnect between those points reconnects using the unfinished event’s cursor, risking a skipped edit or revert. A mocked disconnect reproduced this premature checkpoint. Keep a pending frame ID and commit it only after successfully processing the complete frame; reconnect from the previous committed cursor. The SSE parsing model likewise separates the pending ID buffer from the committed event ID. [SSE specification](https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation)

Validation used read-only inspection, Cargo metadata, and an in-memory SSE probe. Full compilation was blocked by the read-only filesystem.

**Verdict: REQUEST CHANGES.**