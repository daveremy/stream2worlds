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
- **No compiled domain code** (Dave, 2026-09-28; decision 0018). No crate may name a domain or key on a
  domain's field names. Domain knowledge is discovered and persisted as data in the log. Code knows
  protocols and formats, never what a stream is about. Recorded fixtures may contain domain data.

## Three surfaces per gate (Dave, 2026-09-28; decision 0017)
Visualization and agent use are first class. Every gate ships the core capability, the view that
shows it to a person, and the MCP surface that shows it to an agent. A capability without both
surfaces is not done unless the PR names the issue that adds the missing one. The renderer stays
domain-free: anything domain-specific belongs in a view spec, never in renderer code.

## Documentation: the README is the user manual
The README is the user documentation, so it must describe what the code does today. **Every
sprint ends with a doc scrub** (Dave, 2026-09-27), run before the sprint report:
1. README: interface examples, the Technical architecture table's statuses, the Evaluation
   section, and the Roadmap checkboxes match what merged this sprint.
2. `docs/evaluation-contract.md`: nothing built this sprint contradicts it. A contradiction is
   either a code bug or a dated amendment, never a quiet edit.
3. The design page: claims overtaken by decisions get a dated note.
4. Every crate's `AGENTS.md` still matches its dependencies and invariants.
5. Links and commands in the README still resolve and run.
6. Every research note's Design implications have a disposition (`research/README.md`).
7. `CHANGELOG.md` has this sprint's entry (Dave, 2026-09-27): shipped, learned, changed course,
   next. Written for someone following the project's arc, not a list of commits. A sprint with
   no merge still gets an entry.
8. README **Latest** (Dave, 2026-09-27): replace it with the 3 to 5 most interesting changes from
   the newest changelog entries, each one bold line saying what is now true for a user and one
   link. Keep a final **In progress** bullet. Interesting beats recent: a merged CI check is not
   news; a first live stream is.
The sprint report carries one line: `doc scrub: clean` or what changed. Once the workspace
exists, the mechanical parts (table vs `cargo metadata`, link check) run in `cargo xtask check`.
