# 0001: The workspace is the architecture

Date: 2026-09-27 · Status: accepted · Gate 2

## Decision

`s2w` is a Cargo workspace whose crate boundaries are its layers:

```
s2w (bin) → s2w-app → s2w-core, s2w-log, s2w-sources, s2w-system1, s2w-system2 → s2w-model
s2w-testkit (dev only) → s2w-model
xtask: the fitness functions
```

- `s2w-model` and `s2w-core` are pure: no I/O, no async, no clock, no randomness, no hash-order
  iteration. Clippy's `disallowed-methods` and `disallowed-types` enforce it in those two crates.
- Adapters (`s2w-log`, `s2w-sources`, `s2w-system1`, `s2w-system2`) depend only on the model:
  never on the core, never on each other.
- `s2w-app` is the only crate that composes layers. The binary only parses arguments and formats
  output.

## Enforcement

`cargo xtask check` reads Cargo metadata and manifests, never Rust source text:

1. **Dependency allowlist** (`xtask/allowlist.toml`): every edge listed, nothing listed unused.
   Internal edges must resolve to the workspace member of that name; external dependencies must
   come from crates.io. An allowlist rather than a denylist, because agents add dependencies to
   make code compile, and a denylist permits whatever it failed to anticipate.
2. **Stack table:** every external dependency is named, as an exact backticked token, in the
   README Technical architecture row it declares, and every crate is named in the Workspace row.
   The README stays true because the build checks it.
3. **AGENTS.md** in every crate, `xtask` included: allowed dependencies and invariants, where the
   coding agents doing the work will read them.
4. **Lint inheritance:** every crate manifest has `[lints] workspace = true`.
5. **No dependency overrides:** no `[patch]` or `[replace]` in the workspace manifest and no
   `[patch]` or `paths` in `.cargo/config.toml`, since an override changes a dependency's
   resolved source without changing its declared identity.

The compiler enforces the rest. Workspace lints **forbid** `unsafe_code`, `unreachable_pub`,
`unwrap_used`, `expect_used`, `todo`, `unimplemented` and `dbg_macro`; no attribute, inner or
outer, can override a forbid, so relaxing one means editing the workspace manifest under a new
decision record. Other lints may be relaxed locally, but outer `#[allow]` is denied in favour of
`#[expect(lint, reason = "...")]`, and every allowance needs a reason. Crate-level `#![allow]`
is not caught by that lint; it is limited to non-forbidden lints and is caught in review. In `s2w-model` and `s2w-core`, clippy bans common clock,
thread, file, network, environment, process and hash-order APIs, and printing is denied. That is
a denylist of common APIs, not proof of purity; the dependency allowlist (no async runtime, no
I/O crates in those two) and review cover the rest.

Replay determinism, the design's other first fitness function, is added with the fold: golden
replays across partition interleavings, thread counts and restarts.

### Amendment, 2026-09-27: the escape-hatch ratchet is dropped

The first version counted `allow` attributes, `unwrap`, `todo!` and `pub` items by scanning
source text, and allowed counts only to fall. Review ([skeleton round 1](../reviews/skeleton-round1-codex.md))
found routine bypasses: `cfg_attr(..., allow(...))`, `Option::unwrap(x)`, `pub(crate)`, macros,
files outside `src/`, and a `#[cfg(test)]` line that stopped the scan for the rest of a file. A
text scanner over Rust has an unbounded bypass space, so patching it one bypass at a time does
not converge (the lesson of lifeos#1034). The forbidden lints above replace it, and the public-API
count is dropped as over-engineered for a skeleton; `unreachable_pub` covers visibility drift in
private modules.

## Why

Most of this code will be written by coding agents, which drift in predictable ways: duplicate
types, widen visibility to make tests compile, reach for `unwrap` and `#[allow]`, add a
dependency to get past a compile error. Each check or lint turns one of those drifts into a
failing build with a message that says what to do. (Design page, "Built mostly by agents"; round-2
critic review.)

## Alternatives considered

- **One core crate plus one app crate.** Simpler, but it cannot express "adapters never depend on
  the core", and each seam ships with two implementations in the first slice, so the seams are
  real from the start.
- **A denylist of forbidden edges.** Rejected: fails open.

## Revisit when

A layer needs an edge this record forbids. Write a new record, then change the allowlist.
