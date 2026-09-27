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

## Enforcement (fitness functions, `cargo xtask check`)

1. **Dependency allowlist** (`xtask/allowlist.toml`): every edge listed, nothing listed unused.
   An allowlist rather than a denylist, because agents add dependencies to make code compile and
   a denylist permits whatever it failed to anticipate.
2. **Stack table:** every external dependency names its row in the README's Technical
   architecture table, and every crate is named there. The README stays true because the build
   checks it.
3. **AGENTS.md** in every crate: allowed dependencies and invariants, where the coding agents
   doing the work will read them.
4. **Escape-hatch ratchet** (`xtask/ratchet.toml`): per-crate counts of `allow`/`expect`
   attributes, `unwrap`/`expect` calls, `todo!`/`unimplemented!` and `pub` items may only fall.
   Raising one needs a decision record (`cargo xtask ratchet --raise <record>`).

Workspace lints: `unreachable_pub`, `unsafe_code = forbid`, clippy
`allow_attributes_without_reason`, `unwrap_used`, `expect_used`, `todo`, `dbg_macro`.

Replay determinism, the design's other first fitness function, is added with the fold: golden
replays across partition interleavings, thread counts and restarts.

## Why

Most of this code will be written by coding agents, which drift in predictable ways: duplicate
types, widen visibility to make tests compile, reach for `unwrap` and `#[allow]`, add a
dependency to get past a compile error. Each check turns one of those drifts into a failing
build with a message that says what to do. (Design page, "Built mostly by agents"; round-2
critic review.)

## Alternatives considered

- **One core crate plus one app crate.** Simpler, but it cannot express "adapters never depend on
  the core", and each seam ships with two implementations in the first slice, so the seams are
  real from the start.
- **A denylist of forbidden edges.** Rejected: fails open.

## Revisit when

A layer needs an edge this record forbids. Write a new record, then change the allowlist.
