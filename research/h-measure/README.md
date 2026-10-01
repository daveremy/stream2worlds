# H-measure data (s2w#56)

Data for `cargo xtask h-measure`, which grades a stream mapping against an answer key
(contract B3). The code is domain-free (decision 0018); everything that knows what
`mediawiki.recentchange` means is in this directory.

| File | What |
|---|---|
| `corpora.toml` | The corpora (dev, heldout, heldout-2, reserved, reserved-2 for s2w#250 PR 1, reserved-3 for s2w#250 PR 2, and reserved-4 for s2w#291 PR 2; `reserved` opened as held-out by s2w#244, the others by the PRs named): role, window, event count, byte size, sha256. The corpora themselves are not committed. |
| `capture.sh` | The command that produced them, with `research/scripts/eventstreams_replay.py --all-wikis --raw-sse --max-events N`. |
| `private/` | The private-stream capture (s2w#371): `capture.ts` (sources to SSE), `scrub.ts` (the fail-closed gate), `extract.ts` (the published `detail` regex table), `events.ts` (pure builders), `fixture.ts` and `fixture/synthetic-20.sse` (the only committed capture-format file: fake numbers and shas), `capture.test.ts`. See "Private stream" below. |
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
  issue labelled before the span starts has no `pts` inside it. Rows from other projects are dropped; every drop is counted in the header.
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

The capture run, its `corpora.toml` pins (`private-dev` development, `private-test` reserved) and
the freeze are s2w#371 PR 2.

## Private answer key (s2w#372)

`private-key-v0.json` is the private stream's answer key, in key format 2, so `score` reads it as
it reads `dev-key-v2.json`. Identity is the source system's own identifier, never a judgment.
Plan and rulings: the s2w#372 issue comments of 2026-10-01.

| Type | Identity | Mention paths (alias paths join on equal identity values) |
|---|---|---|
| `item` | `(repo, number)`: issues and PRs share one number space per repository | `number`, `issue`, `key` (`"s2w#372"` on a leg), `pr`, `ref_number` (identity `(ref_repo, ref_number)`) |
| `commit` | the 40-hex sha | `sha`, `head_sha`, `merge_commit_sha`, `commit_sha`, `parents.0`, `parents.1` |
| `branch` | `(repo, name)` | `branch`, `head_ref` |
| `sprint` | the sprint number | `sprint`, `slot`, `file_id` |
| `seat` | `seat_id` | `seat_id` (singleton-only type) |
| `comment` | `comment_id` | `comment_id` (singleton-only type) |

- **Unscored:** plumbing and values (`kind`, `ts`, `step`, `verdict`, `engine`, `model`, `pts`,
  `base_ref`, `row_id` and the rest of the key file's list), and `refs`: the capture keeps `#n`
  and drops a `lifeos#`/`s2w#` prefix, so a ref's repository is unknown by construction.
- **Two pinned readings:** the base key leaves `repo` and `actor` unscored (two repo values and
  one observable actor would carry a large share of the micro score for a trivially keyed
  field). `private-key-v0.context-scored.json` (variant `context-scored`) scores `repo` (alias
  `ref_repo`) and `actor` (alias `author`, `other` excluded). A score report names the variant
  each number came from.
- **Relationships are declared, not scored:** `EDGES` in `private/key.ts` lists the typed
  directed edges (§B3) between mentions of one frame, with `key.ts --edges` printing them as
  JSON. The edge grader and a key format that carries them are s2w#388.
- **Generated, never hand-edited:** `node --experimental-strip-types
  research/h-measure/private/key.ts --write` writes both key files and
  `private/fixture/synthetic-20.key-shape.json`. A changed key is a new file and a new
  `keys.toml` row (`private-key-v1.json`), never an edit. The key must be pinned before the H
  freeze that `private-test` is scored against: `score` refuses a key the freeze did not record.
- **Verifier:** `key.ts --check --corpus <name>.raw.sse --provenance <name>.provenance.jsonl`
  executes the key with the Rust executor's semantics and checks each mention against the
  capture's sidecar: `sidecar-aligned` (one sidecar line per frame, same id), `sha-shape` (every
  commit mention is 40 lowercase hex), `sha-resolved` (a leg's `sha` or a seat's `head_sha` is
  the sidecar's `sha_resolved`), `ref-repo`, `seat-issue` (a seat without an issue is marked
  `issue_unobservable` exactly when no PR has its branch) and `leg-key`. It prints counts only
  (mentions per path, entity sizes per type, abstentions, rule failures, observed edges) and
  exits 1 on any failure; that report is the publishable summary of the key.
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
must be outside the repository; one key is one replicate. Each `--corpus` (a pinned corpus,
checked against its pin) is written to `DIR/<corpus>.obf-<replicate>.raw.sse`, and each `--key`
(a pinned answer key) is renamed to the obfuscated paths and written to
`research/h-measure/<key stem>.obf-<replicate>.json`. The metadata file (default
`research/h-measure/obfuscation/<replicate>.meta.json`) records the field table, how each path
was treated, undeclared numeric paths, URL values that fell back to a whole-text hash, rules
that matched no path, the unobservable rows, and the sha256 of the key, the rules, every input
and every output. It never records the key itself.

A replicate's windows share one key and one field table, but they need not run together: the
test window is obfuscated later than the development window, after the mappings are committed.
With `--meta` naming an existing metadata file, the run reuses its field table and refuses a
field path the table does not hold, a different key, a different rules file, a different
replicate, or a corpus or answer key the metadata already records. It then appends its inputs
and outputs (window name, file, sha256) to the metadata, unites the per-path treatments and
undeclared numbers, sums the fallback counts, and keeps a rule listed as unused only if no
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

[[unobservable]]           # recorded in the metadata only
from = "comment"
to = "title"
kind = "names"
reason = "a title named inside a comment"
```

A rule sets exactly one of `domain`, `url_query` and `unix_seconds`; `fold`, `from` and
`url_path` need a `domain`. A `url_path` value of any other shape is hashed whole as text and
counted in the metadata.

A fold changes what the context-collision rows can measure: one name under two contexts gets
two hashes, so a folded type has no collision groups on the obfuscated stream. Every other
number is the same as on the plain stream up to renaming; `h_measure/obfuscate/tests.rs` checks
this on a synthetic stream.

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
