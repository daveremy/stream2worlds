# Stream2Worlds (s2w)

An experiment: point it at any event stream and a living world model forms (System 1 per event,
System 2 in the background, forecasts graded by reality). Design: `docs/design/`. Decisions:
`docs/decisions/`. Status and gates: README "Roadmap".

## Forge and branches
GitHub (`gh`), private until launch. Work on a branch (`feat/`, `fix/`, `chore/`, `pair/`), open a
PR; never commit to `main` directly after the bootstrap commit.

## Build gates (once the workspace exists)
`~/.cargo/bin/cargo fmt --check && ~/.cargo/bin/cargo clippy --all-targets -- -D warnings && ~/.cargo/bin/cargo test && ~/.cargo/bin/cargo xtask check`
(`cargo` is not on the default PATH on the hub.) Rust target dir: `CARGO_TARGET_DIR=~/.cache/cargo-target/stream2worlds/<branch>`.

## Architecture rules that fail the build
- The core (`s2w-core`, `s2w-model`) is pure: no I/O, no async, no wall clock, no RNG, no HashMap iteration order.
- Adapters depend only on `s2w-model`; never on the core or on each other.
- Every crate has an `AGENTS.md` with its allowed dependencies and invariants. Read it before editing the crate.
- Golden replay files are human-owned: never regenerate one to make a test pass; flag it instead.
