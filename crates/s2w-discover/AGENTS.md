# s2w-discover

Structure discovery: profiles a window of raw payloads and proposes a `StreamMapping`
(decision 0021), or abstains. The H-min heuristic profiler of decision 0022 (H-lite before
`PROFILER_VERSION` 5).

## Allowed dependencies

- `s2w-model`, `serde_json`

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Pure, like the core: no I/O, no async, no clock, no randomness, no `HashMap`/`HashSet`
  (this crate's `clippy.toml` enforces the same bans).
- Every decision reads value equality, presence or stream order, never a key's name or a
  value's text. Renaming keys and hashing strings changes the proposal only by the same
  renaming; `tests.rs` asserts it, and `cargo xtask check` 12 does on the recorded fixture
  (`testdata/recorded.raw.sse`, a link to the s2w-sources fixture). No tie is ever broken by a name.
- Type labels and attribute names are built from the stream's own key names, as data.
- Abstaining is a first-class answer: every role that cannot be decided says so in `Profile`,
  and no mapping is emitted without an entity type or below `min_events`.
- Thresholds live in `Config` and decision 0022; change one there, with the fixture numbers.
- `PROFILER_MODEL` (`h-min`) and `PROFILER_VERSION` are recorded on every proposal `serve`
  files from this crate (decision 0025). Bump the version with any change to
  `Config::default()` or to a rule; the model name is not a rule.
- Unit tests use synthetic streams with neutral names; no domain knowledge in this crate
  (decision 0018).
