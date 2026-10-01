# H-measure data (s2w#56)

Data for `cargo xtask h-measure`, which grades a stream mapping against an answer key
(contract B3). The code is domain-free (decision 0018); everything that knows what
`mediawiki.recentchange` means is in this directory.

| File | What |
|---|---|
| `corpora.toml` | The corpora (dev, heldout, heldout-2, reserved, reserved-2 for s2w#250 PR 1, reserved-3 for s2w#250 PR 2, reserved-4 for s2w#291 PR 2, reserved-5 for s2w#245 PR 4, and reserved-6 for s2w#375 (pinned by PR 1, still unopened); `reserved` opened as held-out by s2w#244, the others by the PRs named), and the private stream's `private-dev` and `private-test` (s2w#371) and their second capture `private-dev-2` and `private-test-2` (s2w#375, pinned by PR 1), each with its provenance sidecar: role, window, event count, byte size, sha256. The corpora themselves are not committed. |
| `capture.sh` | The command that produced them, with `research/scripts/eventstreams_replay.py --all-wikis --raw-sse --max-events N`. |
| `private/` | The private-stream capture (s2w#371): `capture.ts` (sources to SSE), `scrub.ts` (the fail-closed gate), `extract.ts` (the published `detail` regex table), `events.ts` (pure builders), `fixture.ts` and `fixture/synthetic-20.sse` (the only committed capture-format file: fake numbers and shas), `capture.test.ts`. See "Private stream" below. |
| `prices.toml` | The public price table `cargo xtask gate3` charges a run by: one row per model snapshot, USD per million tokens, with its source page and date. |
| `committed/` | Committed gate-3 replicates (`<arm>.<corpus>.r<k>.json`) and their transcripts (`.transcript.json`), written by `cargo xtask gate3 commit`. See "Committing a System 2 mapping" below. |
| `keys.toml` | Every answer key's sha256, pinned before any score is run, and the reading of #17 it encodes. |
| `dev-key-v0.json` | dev-key v0: the base key (key-spec format v0, `xtask/src/h_measure/key.rs`). |
| `dev-key-v0.<variant>.json` | Sensitivity variants. Each differs from the base only as `keys.toml` says; `diff` the files to see the variant. |
| `dev-key-v1.json` | dev-key v1: the v0 base key in key-spec format 1, with `"no_identity": [0]` on the `log_id` mention (#225). The v0 files stay as they are. |
| `dev-key-v1.<variant>.json` | Each v0 sensitivity variant in format 1, with the same `no_identity` on `log_id`, pinned before any score. |
| `dev-key-v2.json`, `dev-key-v2.<variant>.json` | Each v1 key in key-spec format 2, with its 84 exact `log_params` entries replaced by one prefix entry (#224). The v1 files stay as they are. |
| `frozen/h-lite-v2.dev-N.json` | H-lite (`PROFILER_VERSION` 2) frozen on `dev` at N = 10^4 and 2×10^5 (commit 5e74d8f, built at eeeced7), before any held-out corpus was scored. Score them with a build whose profiler and `Config` match, or `score` refuses. |
| `results/h-lite-v2.dev-N.<corpus>.md` | The pre-registered held-out reports, one per frozen file and held-out corpus, read in research [0009](../0009-h-min-plain-wikipedia.md). |
| `frozen/h-lite-v3.dev-N.json`, `results/h-lite-v3.*` | H-lite `PROFILER_VERSION` 3 (s2w#250 PR 1): frozen on `dev` at the same windows, its `dev` profile table, and its `reserved-2` reports next to v2's on the same span. |
| `frozen/h-lite-v4.dev-N.json`, `results/h-lite-v4.*` | H-lite `PROFILER_VERSION` 4 (s2w#250 PR 2, the second entity test): frozen on `dev` at the same windows, its `dev` profile table, and its `reserved-3` reports, with v3's reports on the same span beside them. |
| `frozen/h-min-v5.dev-N.json`, `results/h-min-v5.*` | H-min `PROFILER_VERSION` 5 (s2w#244, stage 5b containment): frozen on `dev` after `reserved` was opened, its `dev` profile table, and its `reserved` reports. |
| `frozen/h-lite-v4.dev-N.pins-244.json`, `results/h-lite-v4.dev-N.reserved.md` | v4 re-frozen on `dev` under the pins after `reserved` was opened (only that pin line differs from `h-lite-v4.dev-N.json`, which `score` refused for `reserved` until s2w#277), and its `reserved` reports beside v5's. *Correction 2026-09-30 (s2w#277): `score` now accepts a freeze made before its span was opened, so `h-lite-v4.dev-N.json` scores on `reserved` directly; re-scored from 455c547 plus the fix, both windows' reports differ from the committed ones only in the line naming the mapping file. These files stay as the record of what #244 scored.* |
| `frozen/h-min-v6.dev-N.json`, `frozen/h-min-v7.dev-N.json`, `results/h-min-v{6,7}.dev-N.reserved-4.md` | H-min `PROFILER_VERSION` 6 (s2w#291 PR 1, frozen before any PR 2 change) and 7 (s2w#291 PR 2, RFC 3339 date-times are a format, decision 0030; frozen after `reserved-4` was opened), and their `reserved-4` reports under `dev-key-v1.json` and `dev-key-v1.user-global.json`. v6 was scored from a build of a280af2 plus the `reserved-4` opening. The demo's date-time types (#291 item 5) reproduce only on live page-change logs captured by `s2w watch` (21,528 + 5,232 events, 2026-09-27, kept outside the repo); the recorded page-change fixture (11,667 events) cannot show them, so research 0009's #291 addendum reads them from those logs. |
| `frozen/h-min-v8.dev-N.json`, `results/h-min-v8.dev-N.reserved-4.md` | H-min `PROFILER_VERSION` 8 (s2w#327, the integer return floor): frozen on `dev` after every span was opened, and its `reserved-4` reports. Each freeze equals v7's except the version and config text; research 0009's #327 addendum has v8 on every opened span. |
| `frozen/h-min-v8.private-dev-8067.json`, `results/h-min-v8.private-dev-8067.profile.md` | H-min `PROFILER_VERSION` 8 frozen on the whole `private-dev` span (s2w#371 PR 2), after the corpus pins and the private key pin, and its `private-dev` profile table. The freeze `private-test` is scored against. |
| `frozen/h-min-v9.dev-N.json`, `results/h-min-v{8,9}.dev-N.reserved-5.md` | H-min `PROFILER_VERSION` 9 (s2w#245 PR 4, stage 6 links, a version-2 mapping): frozen on `dev` at both windows before `reserved-5` was opened, and the v8 and v9 `reserved-5` reports under the base, `user-global` and `canonical-mention` keys. v8 was scored from a saved build of 1e26249. Research 0009's #245 addendum has the predictions and the per-type ceiling with links. |

## Rules

- A key file never changes once scored. A change is a new file (`dev-key-v1.json`) and a new row
  in `keys.toml`, with the reason; the report lists every key it scored.
- The held-out and reserved corpora are opened only by a score run, never while building,
  tuning or writing a key. Their sha256 was committed before any of those (git log).
- Rebuild the corpora with `capture.sh 2026-09-28T00:00:00Z <dir>` only while EventStreams still
  retains that window (~7 days). The files carry a capture-time header, so a re-capture has new
  hashes; check them against `corpora.toml` and record any change as a new manifest entry.

## Private stream (s2w#371)

The gate-3 private stream (contract §B2.3) is the dev-worker and sprint log of `daveremy/lifeos`
and `daveremy/stream2worlds`: leg status rows, review seats, issues, pull requests and their
timelines, merges, commits and sprint boundaries, one JSON event per frame in the same SSE format
as the Wikipedia corpora, so `freeze`, `profile` and `score` read it unchanged. Plan and rulings:
the s2w#371 issue comments.

- **Capture:** `node --experimental-strip-types research/h-measure/private/capture.ts --name <corpus>
  --since <UTC> --until <UTC> [--dir DIR] [--copy-dir DIR]`. Fetch both clones first (shas are
  resolved there). It writes `<corpus>.raw.sse` and `<corpus>.provenance.jsonl` (the source row id
  and every join resolved at capture, for the answer key) at mode 0600, never over an existing
  file and never inside a git work tree, then prints the `corpora.toml` stanza.
- **What survives:** fields only. Titles, bodies, comment text, commit subjects, the raw `detail`
  text, paths and emails are dropped, and a failed capture prints only its own refusals (never a
  child's stderr or a parse excerpt); `detail` contributes only the fields in `extract.ts`'s
  table, which the capture header repeats. Logins other than the maintainer's public handle
  become `other`. Review-seat fields keep only their logged shape (a word, a verdict, a hex sha, a
  branch, a script basename); anything else becomes `null`. An `*.opened` event carries only what
  was true at open time: labels (and `pts`) arrive as timeline `labeled` events, the head sha on the
  merge or close. Two exceptions GitHub does not timestamp: a PR's `refs` (from its body) and
  `base_ref` are as of capture, not as of open. `pts` appears only on `labeled` events, so an
  issue labelled before the span starts has no `pts` inside it. `refs` (on `pr.opened` and
  `commit`) is a list of `{repo, number}`, deduplicated and sorted by repo then number (s2w#395):
  a bare `#n` is the event's own repo, `lifeos#n`, `s2w#n` and their `daveremy/...` long forms
  name that repo, and any other prefix is dropped and counted as `ref-other-repo`. A frame keeps
  at most 8 refs; the rest are counted as `refs-overflow`. Captures before s2w#395 carry `refs` as
  bare numbers with every prefixed ref dropped uncounted. Rows from other projects are dropped;
  every drop is counted in the header.
- **Scrub gate:** every line the capture would write passes `scrub.ts` first: home paths, `~`,
  `obsidian`, `op://` references, emails, phone numbers and the high-confidence secret shapes.
  One match and nothing is written; the error names the rule and line, never the text. Each rule
  has a planted-sample test.
- **Never in the repository:** `cargo xtask check` (check 20), over every file `git ls-files -co
  --exclude-standard` lists, fails on any `*.sse` under `research/` except the synthetic fixture,
  any `*.provenance.jsonl` anywhere, and any non-Rust file with a line that starts with the
  private capture header.
- **Tests:** `node --experimental-strip-types --test research/h-measure/private/capture.test.ts`
  (CI runs it in the `bundle` job). The fixture test fails if `fixture/synthetic-20.sse` differs
  from what `fixture.ts` prints; regenerate with `fixture.ts --write`, never by hand.

**The capture (s2w#371 PR 2, run 2026-10-01 UTC after both spans had ended):**

| Corpus | Role | Span (UTC) | Events | Dropped (capture header) |
|---|---|---|---|---|
| `private-dev` | development | 2026-09-21T00:00Z to 2026-09-28T12:00Z | 8,067 | 1,109 rows from other projects, 2,720 timeline events of other kinds, 6,632 frames outside the span |
| `private-test` | reserved | 2026-09-29T00:00Z to 2026-10-01T00:00Z | 5,501 | 191 rows from other projects, 1,168 timeline events of other kinds, 717 frames outside the span |

The scrub gate refused nothing (a refusal writes nothing at all), and no review-seat field lost its
shape. Both files and their sidecars are in the default `--dir`, with a byte-identical second copy
in `~/.lifeos/s2w-private/`; `--dir ~/.lifeos/s2w-private` scores from the copy against the same
pins. The order is in the git log: the corpus pins, then `frozen/h-min-v8.private-dev-8067.json`
and its profile table. The private key (`private-key-v0.json`, s2w#372) was pinned before both,
so this freeze is the one `private-test` is scored against. `private-test` stays `reserved` until
that score opens it.

**The second capture (s2w#375 PR 1, run 2026-10-01 UTC):** the same spans and command, taken after
s2w#398 (sprint rows), s2w#405 (a sprint's actual `(slot HH:MM–)` start) and s2w#395 (`refs` as
`{repo, number}`) merged. It supersedes the first capture for scoring; the pinned first captures
stay as the record.

| Corpus | Role | Span (UTC) | Events | `sprint.boundary` frames | Dropped (capture header) |
|---|---|---|---|---|---|
| `private-dev-2` | development | 2026-09-21T00:00Z to 2026-09-28T12:00Z | 8,072 | 62 (sprints 4 to 65) | 1,109 rows from other projects, 2,811 timeline events of other kinds, 6,907 frames outside the span, 116 refs to other repositories, 27 refs over the cap |
| `private-test-2` | reserved | 2026-09-29T00:00Z to 2026-10-01T00:00Z | 5,524 | 23 (sprints 73 to 95) | 191 rows from other projects, 1,262 timeline events of other kinds, 987 frames outside the span, 17 refs to other repositories, 22 refs over the cap |

A frame-level diff against the first capture, reported as counts only, adds 5 and 23
`sprint.boundary` frames and removes nothing; the only changed field is `refs` on `commit` and
`pr.opened` frames, as s2w#395 intends. `outside-span` grew with the sprint rows outside each span
and with timeline events GitHub recorded after the first capture; `timeline-other-kind` grew with
the latter. Every sprint row parsed (`unparsed` 0); seven rows carry an off-hour
`(slot HH:MM–)` start (sprints 66 and 68 to 73), which the capture reads since s2w#405, so sprint 66
(12:30Z) falls outside `private-dev-2` and sprint 72 (2026-09-28T23:36Z) outside
`private-test-2`. `key.ts --check` on `private-dev-2` fails no rule. `private-test-2` stays
`reserved`: only its stanza and the counts-only diff were read.

## Private answer key (s2w#372)

`private-key-v2.json` is the private stream's current answer key, in key format 3; `score` reads
it as it reads `dev-key-v3.json`. It scores captures whose `refs` are `{repo, number}`
(`private-dev-2` onward). `private-key-v0.json` (format 2) and `private-key-v1.json` (format 3)
stay pinned for the first capture. Identity is the source system's own identifier, never a
judgment. Plan and rulings: the s2w#372 issue comments of 2026-10-01, and for v2 the s2w#395
plan comment of 2026-10-01T12:46Z.

| Type | Identity | Mention paths (alias paths join on equal identity values) |
|---|---|---|
| `item` | `(repo, number)`: issues and PRs share one number space per repository | `number`, `issue`, `key` (`"s2w#372"` on a leg), `pr`, `ref_number` (identity `(ref_repo, ref_number)`), and from v2 `refs.0.number` to `refs.7.number` (identity `(refs.i.repo, refs.i.number)`) |
| `commit` | the 40-hex sha | `sha`, `head_sha`, `merge_commit_sha`, `commit_sha`, `parents.0`, `parents.1` |
| `branch` | `(repo, name)` | `branch`, `head_ref` |
| `sprint` | the sprint number | `sprint`, `slot`, `file_id` |
| `seat` | `seat_id` | `seat_id` (one seat across its fallback or re-run attempts, so it can have several mentions; a `null` id is no mention) |
| `comment` | `comment_id` | `comment_id` (singleton-only type) |

- **Unscored:** plumbing and values (`kind`, `ts`, `step`, `verdict`, `engine`, `model`, `pts`,
  `base_ref`, `row_id` and the rest of the key file's list). v0 and v1 also leave all of `refs`
  unscored: the captures they were pinned against keep a bare `#n` and drop every prefixed ref
  (dropped, not stripped: `lifeos#5` is lost), so no field names a ref's repository there.
- **`refs` (v2, s2w#395):** the capture now writes each ref as `{repo, number}` (Private stream
  above), at most 8 per frame. v2 makes each slot `refs.i.number` an `item` mention with the
  same `(repo, number)` identity as every other item path, so `lifeos#900` and `s2w#900` named
  in one PR body join two different items. The base key lists `refs.i.repo` as unscored, as it
  lists `ref_repo`. `key.ts --check` with v2 on `private-dev-2` fails no rule (`ref-repo-known` included):
  1,589 ref mentions (base key: 15,246 mentions under v1, 16,835 under v2), 57 more `item`
  entities (items named only by a ref in the span), and 629 `names` plus 960 `commit-names`
  gold edges.
- **Two pinned readings:** the base key leaves `repo` and `actor` unscored (two repo values and
  one observable actor would carry a large share of the micro score for a trivially keyed
  field). Each version's `.context-scored.json` (variant `context-scored`) scores `repo` (alias
  `ref_repo`, and from v2 `refs.i.repo`) and `actor` (alias `author`, `other` excluded). Every
  version has both files. A score report names the variant each number came from.
- **Relationships (`private-key-v1.json`, s2w#388):** `EDGES` in `private/key.ts` lists the
  typed directed edges (§B3) between mentions of one frame, with `key.ts --edges` printing them
  as JSON. `private-key-v1.json` and `private-key-v1.context-scored.json` are v0 in key format 3
  (below): the same types and `unscored`, plus one relationship row per edge (`label` as the
  type; the two `names -> refs` rows `unobservable`, since `refs` drop the repo prefix). An
  edge's `kinds`, `event` and note stay in `key.ts` as notes: format 3 has no guard, so an edge
  is the co-occurrence of its two mentions in one frame, and `head-commit` also fires on a
  `pr.opened` frame that carries `head_sha` (a true edge, ruling 3). `reviews-item` has no edge
  on a seat whose `issue` is null (marked `issue_unobservable`): there is no `issue` mention.
  v0 stays pinned and scores identity exactly as before. In v2 the two `names -> refs` rows
  become 16 observable rows, one per slot: `names` (`number -> refs.i.number` on `pr.opened`,
  item to item) and `commit-names` (`sha -> refs.i.number` on `commit`, commit to item). They
  carry two labels because the edge scorer aligns each key edge type with one predicted
  `(from type, to type, kind)`; one label over both type pairs would cap any mapping, the
  oracle included, at the larger of the two.
- **Generated, never hand-edited:** `node --experimental-strip-types
  research/h-measure/private/key.ts --write` writes the v2 key files and
  `private/fixture/synthetic-20.key-shape.json` (v2's shapes, and v1's under `v1.base` and
  `v1.context-scored`, each with `edges_per_type`, the unique gold edges per relationship type);
  `key.test.ts` also checks that `spec(variant, 0)` and `spec(variant, 1)` still render the
  pinned v0 and v1 files byte for byte. A
  changed key is a new file and a new `keys.toml` row, never an edit. The key must be pinned
  before the H freeze that `private-test` is scored against: `score` refuses a key the freeze did
  not record.
- **Verifier:** `key.ts --check --corpus <name>.raw.sse --provenance <name>.provenance.jsonl`
  executes the key with the Rust executor's semantics and checks each mention against the
  capture's sidecar: `sidecar-aligned` (one sidecar line per frame, same id), `sha-shape` (every
  commit mention is 40 lowercase hex), `sha-resolved` (a leg's `sha` or a seat's `head_sha` is
  the sidecar's `sha_resolved`), `ref-repo`, `ref-repo-known` (every `refs.i.repo` is `lifeos` or
  `s2w`; a bare-number ref, the first capture's shape, is skipped), `seat-issue` (a seat without an issue is marked
  `issue_unobservable` exactly when no PR has its branch) and `leg-key` (a leg's `key` is
  `repo#issue`). It prints counts (mentions per path, entity sizes per type, abstentions, rule
  failures, observed edges, and legs naming a PR opened outside the span, which is counted but
  not a failure) and the capture header's command, window and drop counts, never a frame's
  value, and exits 1 on any failure; that report is the publishable summary of the key.
- **Hand-inspection sample:** `key.ts --sample N --seed S --corpus … --provenance … --out FILE`
  draws N mentions stratified by mention path and writes a worksheet with each mention's frame
  id, value, sidecar line and the exact source lookup to run. It holds values, so it refuses a
  path inside a git work tree and carries the private capture header (check 20 refuses it in the
  repository).
- **Tests:** `node --experimental-strip-types --test research/h-measure/private/key.test.ts` (CI
  `bundle` job) and `h_measure/private_key_tests.rs`. The Rust executor must reproduce the
  partition shape `key.ts` committed for the synthetic fixture, and the fixture reproduces the
  scoring: an item mapping keyed by number alone merges lifeos#900 with s2w#900 and scores a
  false merge. The fixture's fake sidecar is `fixture/synthetic-20.fixture-provenance.jsonl`,
  written by `fixture.ts --write`.

The checked sample and its note (`results/private-key-v0.note.md`) are s2w#372 PR 2, run on
`private-test` after s2w#371 PR 2 pins it.

## Freezing a mapping

```
cargo xtask h-measure freeze --corpus dev --window 10000 --out FILE [--dir DIR]
```

`--dir` defaults to `~/.local/share/stream2worlds/h-measure`. `freeze` checks every key pin
and the corpus pin, and refuses: a corpus whose `role` is not `development` (a mapping
is never discovered on held-out or reserved data), an `--out` that exists (a frozen mapping is
never overwritten), a corpus whose frame count is not its pinned `events`, and a `--window`
outside 1 to that count (the mapping is discovered on the first `--window` events). The file
records the corpus sha256, the window, the profiler version and config, every key and corpus
pin (a corpus pin includes its role, file and event count), and the profile's abstained paths, so a score can prove what it was frozen against.

## Profiling the development corpus

```
cargo xtask h-measure profile --corpus dev --window 10000 [--dir DIR]
```

Prints the profiler's per-path table (path, count, distinct values, role), the decode steps and
the event-type field for the first `--window` events, as markdown. It checks the corpus pin and
refuses any corpus whose `role` is not `development`, as `freeze` does.

## Scoring a frozen mapping

```
cargo xtask h-measure score --mapping FILE --corpus NAME --key FILE [--key FILE ...] [--json FILE] [--dir DIR]
```

`score` refuses, before it reads the scored corpus: a mapping not frozen on the pinned
development corpus, the `reserved` corpus, no `--key`, a `--key` given twice, a key that is not
pinned or does not match its pin, and a pin it uses that changed since the freeze. The pins it
uses are the freeze corpus, the scored corpus if the freeze recorded it, and each `--key`, which
the freeze must have recorded (a key pinned after the freeze could be written to fit the
mapping). Any other row may be added or changed without affecting an existing freeze. Last,
`score` runs the freeze again on the file's recorded corpus and window and refuses a file that
differs from what that run writes, so a hand-written or edited mapping never scores (s2w#238).
The development corpus must therefore be in `--dir` for every score. A freeze from a different
profiler version or config is refused with the advice to score with the build that froze it: the
commit that adds the frozen file names that build. `score` proves the mapping and profile the
file records, not when it was written: the recorded pins are taken as written, and the commit
history proves the timing of the freeze and of each pin. An abstaining mapping is graded
as the empty prediction. The report marks a score **In sample** when the scored corpus is the
freeze corpus by name or by bytes (two `corpora.toml` names may pin one file).
The markdown report opens with the alias limit, then gives per key the mapping and ceiling
rows (with and without singleton-only types), per type, per path, context collisions, and the
spurious, abstained and excluded counts; `--json` writes every number, replacing FILE if it
exists (a report is recomputable; a frozen mapping is the file that is never overwritten).

## Committing a System 2 mapping (gate 3)

```
cargo xtask gate3 commit --corpus NAME --window N --replicate K --model SNAPSHOT [--arm h-s2|b3] [--h-s2 FILE] [--out FILE] [--dir DIR] [--claude PATH] [--credentials PATH]
```

One replicate of an arm of gate 3 (s2w#373, decision 0032): `h-s2` ("H plus System 2", the
default) or `b3` (the raw-sample baseline, below). `commit` derives H exactly
as `h-measure freeze` does, builds the arm's input from the same profiler run (the 60 newest
events of the window as the sample), and asks the model for a mapping through the Claude CLI in a
clean session: no tools, no MCP servers, no saved session, and a scratch `HOME` holding only a
copy of `--credentials` (default `~/.claude/.credentials.json`). It refuses to start when that
file's access token expires within 90 minutes, so the CLI never refreshes it in the copy. The
first call is a probe; the run stops, writing nothing, unless the model replies `none`; the
error names what the probe was charged. Both arms check the attempt's first valid mapping against
the arm's sampled records as stored (`gate3/no_match.rs`); a mapping that claims nothing from any
of them gets one further no-match repair call, and the committed file's `no_match` block records
the result before and after (decision 0032, dated note 2026-10-01, s2w#409).

Every call goes through a $5 budget gate charged at `prices.toml` (an unknown `--model` is
refused). A call that could take the replicate past $5 is not made, and the replicate is
committed with `failure: "budget: ..."`. `commit` writes the committed file and its transcript,
both new (an existing file is refused before any call), to
`committed/<arm>.<corpus>.r<K>.json` by default.

`--arm b3` gives the model raw events of the same window instead of H's result, up to the
input-token budget of the h-s2 replicate with the same corpus, window, replicate and model
(`--h-s2 FILE`, default `committed/h-s2.<corpus>.r<K>.json`). Before any call it replays that
file as `score` would and takes its budget from it: T, the prompt tokens the h-s2 first call
reported (input plus cache read plus cache write), and B, the h-s2 first prompt's length in
bytes. The sample is every k-th event of the window from the first, each the stored envelope
byte for byte (the record the executor applies a mapping to), for the smallest k whose prompt is
at most B bytes. When the model reports that a fit's first prompt read more than 105% of T, that
fit is spent and the refit shrinks the sample by the measured ratio (stride
`ceil(k × tokens / (T × 0.97))`), under the same $5 gate; when no refit gives a sample, the replicate is
committed with `failure: "budget-fit: ..."`. The committed file records the budget and every
fit in `budget`, and the transcript holds every fit's calls.

Commit both files to git before scoring: the git log is the order proof. Then score the committed
file like a frozen mapping (`score --mapping committed/...json`). `score` checks its `heuristic`
as a freeze, checks the transcript against its sha256, rebuilds the input, replays the probe and
the transcript, and refuses unless the result, attempts and spend match. It grades the committed
mapping, or the empty mapping when the replicate failed. For a b3 file it also replays every
fit and refuses unless the fits match `budget`.

## Context collisions

The unfloored composite-key sub-metric (evaluation contract, cross-wiki composite-key
discovery; accepted as proposed on s2w#56, 2026-09-29). A row is one key type and one
**context path**: an identity path of a mention rule whose identity has two or more paths and
includes the rule's own path, other than that path. Drop the context part from each gold
entity's identity; gold entities of the type that are then equal form a **collision group**.
An entity is found through any gold mention at a path whose rule has the row's identity, an
alias path included. The groups are
the entities a mapping keying the type without the context would merge. The row scores B-cubed
on every gold mention (alias paths included) of the entities in groups of two or more, with
the graded mapping's and the ceiling's predictions both restricted to those mentions. A row
with no group is listed with every metric undefined. The markdown table shows P, R and F1 for
both. A key's excluded (`no_identity`) mentions are not gold mentions and are dropped from every
prediction (#225), so they never form a group.

For the plain stream's key, the cross-wiki sub-metric is the `<type> @ data.wiki` rows. The
rows are reported beside B3's metrics and are never a floor.

## Obfuscating a replicate

```
cargo xtask h-measure obfuscate --rules FILE --key-file FILE --replicate NAME \
  --corpus NAME [--corpus NAME ...] [--key FILE ...] [--meta FILE] [--dir DIR]
```

The keyed transformation of contract B2.2 (s2w#370). The key file holds 64 hex digits and
must be outside the repository; one key is one replicate. On hub a replicate's key lives at
`~/.local/share/stream2worlds/h-measure/replicates/<replicate>.key` (for example `r1.key`). Each `--corpus` (a pinned corpus,
checked against its pin) is written to `DIR/<corpus>.obf-<replicate>.raw.sse`, and each `--key`
(a pinned answer key) is renamed to the obfuscated paths and written to
`research/h-measure/<key stem>.obf-<replicate>.json`. The metadata file (default
`research/h-measure/obfuscation/<replicate>.meta.json`) records the field table, how each path
was treated, undeclared numeric paths, URL values that fell back to a whole-text hash, values
hashed as their own for an absent `from` path, rules that matched no path, the unobservable
rows, and the sha256 of the key, the rules, every input and every output. It never records the key itself.

A replicate's windows share one key and one field table, but they need not run together: the
test window is obfuscated later than the development window, after the mappings are committed.
With `--meta` naming an existing metadata file, the run reuses its field table and refuses a
field path the table does not hold, a different key, a different rules file, a different
replicate, or a corpus or answer key the metadata already records. Such a run may name only
`--key` files, with no `--corpus`: it renames keys under the recorded table (a format-3 key added
after its window was obfuscated, s2w#388); without an existing metadata file `--corpus` is
required. It then appends its inputs
and outputs (window name, file, sha256) to the metadata, unites the per-path treatments and
undeclared numbers, sums the fallback and own-value counts, and keeps a rule listed as unused only if no
run's input held its path, so the metadata stays the complete record of
the replicate. An output that exists is refused; nothing is written unless every input
transforms.

The collision check holds within one run only: the metadata stores no plaintext, so a later
run cannot compare its values with an earlier run's. A cross-run collision of the 64-bit
truncated hash is possible in principle. For n distinct values across all runs of a replicate
its probability is at most n²/2⁶⁵; our pinned corpora hold 800,000 events, so n is under
2×10⁷ even at 20 distinct hashed values per event, and the probability is under 1.1×10⁻⁵.

What the run does to an event:

- Field names become `f1`, `f2`, ... by a keyed ranking of every distinct field path (a path,
  not a key name: `id` and `meta.id` get different names).
- A value at a path with a `domain` rule becomes `h` and 16 hex digits of a keyed hash of the
  domain and the value. One value in one domain gets one hash at every path; one value in two
  domains gets two. A collision between two different values fails the run.
- Every other string is hashed whole in a text domain; a category value stays one category.
- RFC 3339 strings, and integer paths with `unix_seconds = true`, move by one shift per
  replicate, so differences between times are kept.
- Numbers, booleans, nulls and structure are kept; numbers at undeclared paths are listed in
  the metadata for the rules author to check.
- The SSE `id:` cursor is hashed whole.

### Rules file (format 1)

```toml
version = 1

[[rule]]
path = ["wiki"]            # a path is a list of object keys; array indexes are not part of it
domain = "wiki"

[[rule]]
path = ["server_name"]
domain = "wiki"
from = ["wiki"]            # alias: hash the value at `from`, so the two are byte-equal

[[rule]]
path = ["meta", "domain"]
domain = "wiki"
from = ["wiki"]
own_if_absent = true       # a record with no `wiki` hashes its own value instead (counted)

[[rule]]
path = ["title"]
domain = "title"
fold = [["wiki"]]          # context values hashed in first: one title on two wikis, two hashes

[[rule]]
path = ["title_url"]
domain = "title"
fold = [["wiki"]]
url_path = { base = ["server_url"], marker = "/wiki/", replace = [["_", " "]] }

[[rule]]
path = ["notify_url"]      # each listed query parameter hashed into its domain, joined by `/`
url_query = [
  { param = "diff", domain = "revision", fold = [["wiki"]] },
  { param = "rcid", domain = "rcid" },
]

[[rule]]
path = ["timestamp"]
unix_seconds = true

[[unobservable]]           # added to the renamed key's `unscored`
path = ["comment"]
reason = "names inside free text are destroyed with the text"

[[unobservable]]           # marks a matching format-3 key row unobservable
from = "comment"
to = "title"
kind = "names"
reason = "a title named inside a comment"
```

A rule sets exactly one of `domain`, `url_query` and `unix_seconds`; `fold`, `from` and
`url_path` need a `domain`. A `url_path` value of any other shape is hashed whole as text and
counted in the metadata. A `from` path a record lacks fails the run, unless the rule sets
`own_if_absent`: then that record's own value is hashed in the rule's domain and counted in the
metadata's `own_values`. A `from` path that is present but holds no string or number still fails.
A rule's path must hold scalars (alone or in arrays); an object there fails the run, unless the
rule sets `scalars_only`, which hashes the scalars at the path and walks an object as if no rule
named it (`log_params` is an object on most events and a bare array on a few).

A format-3 key's relationship rows are renamed with its mention paths. Every `[[unobservable]]`
row is recorded in the metadata, and the renamed key marks a relationship row unobservable (with
the rule's reason) when a relationship rule's `from` and `to` each name a field holding the row's
endpoint (`revision` holds `revision.new`), or a path rule holds either endpoint. Matching is on
the endpoints only; a row the key already marks keeps its own reason.

A fold changes what the context-collision rows can measure: one name under two contexts gets
two hashes, so a folded type has no collision groups on the obfuscated stream. Every other
number is the same as on the plain stream up to renaming; `h_measure/obfuscate/tests.rs` checks
this on a synthetic stream.

Replicate r1 (s2w#370) covers the development window: `[corpus.obf-r1-dev]` in `corpora.toml`,
`dev-key-v2.obf-r1.json` in `keys.toml`, and `obfuscation/r1.meta.json`, from
`obfuscation/recentchange.rules.toml`. Its key is outside the repository; the test window is
obfuscated later under `--meta obfuscation/r1.meta.json`. `dev-key-v3.obf-r1.json` (s2w#388) is
`dev-key-v3.json` renamed by a keys-only run under that metadata (`--replicate r1 --key
dev-key-v3.json`); its identity rows equal `dev-key-v2.obf-r1.json`'s, and no rule in the rules
file hides any of its seven edges (the four relationship rules name text and URL fields).

## How the base key was written

From the published `mediawiki/recentchange` schema and #17's proposed answers, before any
H-lite output on these corpora existed:

- **wiki**: mentions at `wiki`, `server_name`, `server_url` and `meta.domain`, identity `wiki`
  (#17 Q4: deterministic aliases, one entity).
- **page**: mentions at `title` and `title_url`, identity (`wiki`, `namespace`, `title`)
  (#17 Q1, `wiki` folded in).
- **user**: `user`, identity (`wiki`, `user`); `user-global` drops `wiki`.
- **revision**: `revision.new` and `revision.old`, one type, identity (`wiki`, value)
  (#17 Q2a one domain, Q2b `wiki` folded in).
- **event**: `meta.id`. **log**: `log_id`, identity (`wiki`, `log_id`).
- **Unscored**: `meta.uri` (equals `title_url` for most event types, not provably for all),
  `id` (rcid, #17 Q3), `meta.request_id` (one request can write several events), `comment`,
  `parsedcomment`, and `log_params` with every path under it.
- **Unscored transport and plumbing fields** (ruling 2026-09-29, item 4): `meta.topic`,
  `meta.partition`, `meta.offset` (Kafka position, not entity identity), `notify_url` (a
  per-event diff URL) and `server_script_path` (one value per wiki family).

Key format v0 matches unscored paths exactly, with no prefix form, so `log_params` is listed as
the 84 paths it takes in the **dev** corpus (itself, its keys, and array indexes). A
`log_params` path that occurs only in a held-out corpus is still scored; the report counts any
predicted mention there. This is an accepted limit of v0 (ruling 2026-09-29, item 2). A prefix
form for `unscored` is a key-format change: #224.
*Update 2026-09-30 (#224): key format 2 lifts this limit. `dev-key-v2*.json` list
`{"prefix": ["data", "log_params"]}` instead of the 84 paths, so every `log_params` path in any
corpus is unscored. The v0 and v1 keys keep the limit; see "Key format 2" below.*

Canary events (`meta.domain` = `canary`, hourly, in either topic) carry only `$schema` and
`meta`, so no wiki, page, user, revision or log mention exists in them. Every key mentions each
canary once, as a singleton `event` entity (`meta.id`); in `q4-separate` the `domain` type also
makes them one `canary` entity (5 events in dev).

## Key format 1: value exclusion (#225)

1,348 of the dev corpus's 19,684 log events are AbuseFilter hits with `log_id` 0 (no log row is
written). Base key v0 reads them as one log entity per wiki: false gold merges. Format v0
cannot exclude a value, so the fix (karpathy, 2026-09-29) is key format 1 and a new key file,
not a key that drops `log_id`.

Format 1 is format 0 plus one optional field on a mention rule, `no_identity`: a list of
sentinel values that mean "no identity". A record whose value at the rule's `path` is one of
them has no mention there: not a singleton, not a merge, and not abstained. The executors
report these as excluded mentions, per path. Rules:

- Values compare as key parts, so `0` and `"0"` are different sentinels. Each value must be a
  string, an integer or a boolean, listed once.
- `no_identity` is allowed only on a rule whose `path` is one of its `identity` paths, and only
  in a spec of version 1 or later. An alias rule (path not in its identity) cannot carry it, and an
  alias mention of a sentinel identity is **not** excluded; a key that needs that is another
  format change.
- `grade` drops the key's excluded mentions from **every** prediction before scoring, the
  graded mapping's and the oracle ceiling's alike (karpathy, 2026-09-29): the key has no
  mention there, so a predicted one is neither spurious nor abstained, as on an unscored path.
  The v0 mapping format cannot exclude a value, so every expressible mapping mints the
  sentinel; filtering only the oracle would make the ceiling unreachable. The cost: a future
  method that correctly declines to mint the sentinel gets no credit for it. Crediting that
  needs a richer mapping format, like the alias limit above.
- A format-0 file reads exactly as before, so every v0 pin still validates unchanged.

`dev-key-v1.json` is the v0 base key with `"version": 1` and `"no_identity": [0]` on the
`log_id` mention. On the dev corpus (key executor, 200,000 records): v0 gives 19,684 log
mentions in 18,376 log entities; v1 gives 18,336 log mentions in 18,336 entities, with 1,348
excluded. On this corpus every v1 log entity is then a singleton (no non-zero `log_id` repeats
within a wiki), so `log` is a singleton-only type in the dev score; the key does not force
it. The v0 sensitivity variants still merge `log_id` 0; a variant that needs the exclusion is
a new format-1 file.

## Key format 2: prefix form for unscored paths (#224)

Format 2 is format 1 plus a second form of `unscored` entry. An entry is either a path, as
before, matched exactly, or `{"prefix": [...]}`, which covers that path and every path under
it: its keys, their keys and array indexes, at any depth. Rules:

- Paths compare as mention path ids (`s2w_discover::rule_id`), as exact entries always did, so
  `["a", 1]` and `["a", "1"]` are one path. A prefix covers only whole segments: the prefix
  `["data", "p"]` covers `data.p.x` but not the key `px`, nor the one key `p.x`.
- A prefix is allowed only in a spec of `"version"` 2 or later. An entry listed twice, or at or under
  another entry's prefix, is refused: the prefix already covers it. A mention path at or under
  a prefix is refused, as a mention path that is also unscored always was.
- An exact entry in a format-2 file behaves as in format 0: it covers itself only.
- A format-0 or format-1 file reads exactly as before, so every v0 and v1 pin still validates
  unchanged.

`dev-key-v2.json` and its five variants are the v1 files with `"version": 2` and the 84 exact
`log_params` entries replaced by one `{"prefix": ["data", "log_params"]}` entry; nothing else
differs (`diff` them). On the dev corpus, v1 and v2 give the same thing: for each of the six
pairs, the key executor's partition and the grade of `frozen/h-min-v8.dev-200000.json` are
equal (checked 2026-09-30 by a one-off test at commit 896fc12d, not kept: it needs the
uncommitted corpus). A v2 key differs from v1 only on a corpus
that holds a `log_params` path the dev corpus does not. `score` refuses a key that the freeze
did not record, so a v2 key grades mappings frozen after these pins, not the files already in
`frozen/`.

## Key format 3: relationships (#388)

Format 3 is format 2 plus a `relationships` table: typed, directed edges between two mention
paths of one record (contract §B3 "Relationships"). Plan and rulings: the s2w#388 issue
comments of 2026-10-01.

```json
"version": 3,
"relationships": [
  { "type": "parent", "from": ["data", "sha"],    "to": ["data", "parents", 0] },
  { "type": "parent", "from": ["data", "sha"],    "to": ["data", "parents", 1] },
  { "type": "names",  "from": ["data", "number"], "to": ["data", "refs"],
    "unobservable": "refs drop the repo prefix" }
]
```

- **Semantics.** A record holds a row's edge when both of its paths hold a gold mention there:
  the test the engine applies to a `RelationshipRule` (both endpoint rules matched in one
  payload), with no guard field. The edge is `(type, from entity, to entity)`; the key's edges
  are the unique set over the corpus, so an edge seen in 400 records is one edge. Direction is as
  written: the reverse is another edge. An abstained or excluded endpoint is no mention, so it
  places no edge. Rows sharing a `type` form one key edge type (the two `parent` rows above).
- **Validation.** `relationships` needs `"version"` 3 or later. A `type` is non-empty without
  U+001F; `from` and `to` are well-formed and differ; an observable row's `from` and `to` are
  each a mention path of the key (of any type). A row listed twice, or two rows on one
  `(from, to)` pair, is refused: one pair carries one edge type.
- **Unobservable rows.** `"unobservable": "<reason>"` (the reason is required) marks an edge the
  stream cannot show. Such a row may name any path, a mention path or not; it places no gold edge
  and is counted, never scored.
- **Predicted edges.** The mapping executor turns each relationship rule whose two endpoint
  rules matched (the engine's `RelationshipObserved` claim) into `(kind, from cluster, to
  cluster)`, each endpoint resolved through the same fold as its mention, so links join edge
  endpoints too.
- **Oracle.** The oracle-v0 mapping gets a relationship rule per observable row whose two
  mention paths both got an oracle rule; a row touching an alias path gets none, so the ceiling
  shows that limit as it shows the alias limit. The oracle with links reaches alias endpoints
  through its alias rules.
- **Back-compat.** A format 0–2 file has no `relationships`, reads exactly as before, writes no
  such field, and its oracles have no relationship rules, so nothing a pinned v0–v2 key scores
  changes. `from_mapping` writes format 3, one row per relationship rule, and the selftest checks
  that a mapping read as its own key (and that key's oracle) places exactly the mapping's edges,
  and that those equal `MappingEngine`'s relationship claims.
- **Pinned format-3 keys (s2w#388).** `dev-key-v3.json` and `dev-key-v3.canonical-mention.json`
  are the v2 files plus `"version": 3` and seven Wikipedia rows: `revision.new -> title` (`edits`),
  `revision.new -> revision.old` (`follows`), `revision.new -> user` (`by`), `log_id -> title`
  (`targets`), `log_id -> user` (`logged-by`), `title -> wiki` (`page-on`) and `user -> wiki`
  (`user-on`); `event` (`meta.id`) has none. The four other Wikipedia variants stay format 2:
  they vary identity readings, and a format-2 key reports "No relationships declared".
  `private-key-v1*.json` and `private-key-v2*.json` are above; `dev-key-v3.obf-r1.json` is under "Obfuscating a
  replicate".

### Scoring edges (contract §B3 "Relationships")

`score` grades edges for every key that declares `relationships`; a format 0–2 key prints one
line, "No relationships declared by this key (format 2 or earlier).", and its JSON holds
`edges: null`. The rule (`xtask/src/h_measure/edges.rs`):

- **Endpoints.** A predicted cluster stands for the key entity holding strictly more than half
  of its scored mentions (`2 · shared > size`, integers; the size counts spurious mentions, as
  precision does). Exactly half, or less, is no majority: an edge touching that cluster is false.
  An edge whose endpoint cluster has no scored mention at all (every mention unscored or
  excluded) is dropped and counted, never false; that is how an oracle edge to a `no_identity`
  value stays out of the ceiling. Known limit: if that excluded value is also a scored mention of
  the same type at another path, the cluster keeps that mention and the edge maps to its entity
  (an edge carries no record to tell the two apart).
- **Types.** A predicted edge's type is `(type of the from cluster, type of the to cluster,
  kind)`. Predicted types align one-to-one with key edge types by a maximum-weight assignment,
  the weight being how many distinct key edges of that key type the predicted type hits. The
  solver is an in-house Hungarian algorithm tested against exhaustive enumeration. **Tie rule:**
  among assignments of equal total, the one whose sorted `(key type, predicted type)` pair list
  is lexicographically smallest wins (types sorted by name, as the report prints them). A pair
  with weight 0 is never aligned.
- **Counting.** Under the alignment, a key edge hit by at least one predicted edge is one true
  positive; every further predicted edge on it is a false positive (an entity split across two
  clusters). Every other predicted edge is a false positive, including all edges of an unaligned
  predicted type; every key edge never hit is missed. Micro `P = TP / (TP + FP)`, `R = TP / (TP +
  FN)`, F1; a zero denominator is undefined.
- **Unobservable rows.** They place no gold edge. A predicted edge of an unaligned type whose
  mapped endpoints have an unobservable row's endpoint types is dropped and counted, not false.
  An edge of an aligned type pays its false positive whatever rows the key marks unobservable.
- **Report.** Per key: a row each for the mapping, the ceiling and the ceiling with links; a row
  per key edge type with its aligned predicted type and the ceiling's recall; then the unaligned
  predicted types, the no-majority, dropped and unobservable counts, and the declared types with
  no edge in the corpus. `--json` adds `grade.edges`, `grade.ceiling_edges` and
  `grade.ceiling_links_edges`.
- **Fixtures.** `cargo xtask h-measure selftest` prints the contract's eight frozen fixtures
  (`xtask/src/h_measure/fixtures.rs`), the three edge ones included, and checks that a mapping
  with relationship rules, graded against its own key, scores edge P and R 1.0 for the mapping
  and the ceiling.
