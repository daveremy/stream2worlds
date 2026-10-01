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
  value's text, with one exception: whether a string is an RFC 3339 date-time (`stamp.rs`, a
  format, decision 0030). Renaming keys, shifting every date-time by one constant and hashing
  every other string changes the proposal only by the same renaming; `tests.rs` asserts it, and
  `cargo xtask check` 12 does on the recorded fixture (`testdata/recorded.raw.sse`, a link to the
  s2w-sources fixture), comparing roles as well. No tie is ever broken by a name, with one
  documented exception: `link::link_member` picks the member a link names, and when two
  members of a class tie on events carried and distinct values it takes the first in table
  order, which can follow key names (decision 0022, `PROFILER_VERSION` 9). It is
  deterministic, and the tied members are aliases, so either choice merges the same entities
  in at least `alias_pct` of events. A new format
  is a new decision record, never a quiet addition to `stamp.rs`.
- The second entity test carries a churn guard and a leaf cap (decision 0022, `PROFILER_VERSION`
  6): a key whose values move on and never return fails it, and a type only it admits relates
  once. Both read stream order and equality only; no counter is named anywhere. An integer-valued
  second-test key must also come back under some follower (`return_pct`, `PROFILER_VERSION` 8,
  s2w#327); it reads the value's kind, which obfuscation leaves unchanged, never its text.
- `diag::key_report` prints the entity tests' per-key numbers for a person (s2w-app's ignored
  `discover_diag` test). No rule reads it.
- Stage 6 links (decision 0027, `PROFILER_VERSION` 9): a 1:1 loser stays a key path under the
  winner's label, linked into the class with the most distinct values; the mapping is then
  version 2. `Config::links` turns this off (every loser an attribute, version 1, exactly
  version 8's output); `serve`'s auto-apply runs with it off, so links are measurement-only
  until s2w#392 (decision 0022's s2w#245 amendment).
- Type labels and attribute names are built from the stream's own key names, as data.
- Abstaining is a first-class answer: every role that cannot be decided says so in `Profile`,
  and no mapping is emitted without an entity type or below `min_events`.
- Thresholds live in `Config` and decision 0022; change one there, with the fixture numbers.
- `PROFILER_MODEL` (`h-min`) and `PROFILER_VERSION` are recorded on every proposal `serve`
  files from this crate (decision 0025). Bump the version with any change to
  `Config::default()` or to a rule; the model name is not a rule.
- `manifest` (decision 0029, s2w#301) holds `path_stats` (the profiler's numbers copied into
  `s2w_model::PathStats`) and `FallbackProposer` (actor `dashboard-fallback/3`): the `feed`
  projection, one default role, a label per type only where the statistics pick one uniquely.
  The same name-blindness rule applies; `tests.rs` asserts it under the obfuscation. Bump
  `FALLBACK_VERSION` with any change to what it proposes. `PathProfile`'s `str_count` and
  `str_len_mean` feed it; they change no rule, so `PROFILER_VERSION` did not move.
- Unit tests use synthetic streams with neutral names; no domain knowledge in this crate
  (decision 0018).
