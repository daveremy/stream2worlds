# H-measure data (s2w#56)

Data for `cargo xtask h-measure`, which grades a stream mapping against an answer key
(contract B3). The code is domain-free (decision 0018); everything that knows what
`mediawiki.recentchange` means is in this directory.

| File | What |
|---|---|
| `corpora.toml` | The corpora (dev, heldout, heldout-2, reserved, reserved-2 for s2w#250 PR 1, and reserved-3 for s2w#250 PR 2; `reserved` opened as held-out by s2w#244, the others by the PRs named): role, window, event count, byte size, sha256. The corpora themselves are not committed. |
| `capture.sh` | The command that produced them, with `research/scripts/eventstreams_replay.py --all-wikis --raw-sse --max-events N`. |
| `keys.toml` | Every answer key's sha256, pinned before any score is run, and the reading of #17 it encodes. |
| `dev-key-v0.json` | dev-key v0: the base key (key-spec format v0, `xtask/src/h_measure/key.rs`). |
| `dev-key-v0.<variant>.json` | Sensitivity variants. Each differs from the base only as `keys.toml` says; `diff` the files to see the variant. |
| `dev-key-v1.json` | dev-key v1: the v0 base key in key-spec format 1, with `"no_identity": [0]` on the `log_id` mention (#225). The v0 files stay as they are. |
| `dev-key-v1.<variant>.json` | Each v0 sensitivity variant in format 1, with the same `no_identity` on `log_id`, pinned before any score. |
| `frozen/h-lite-v2.dev-N.json` | H-lite (`PROFILER_VERSION` 2) frozen on `dev` at N = 10^4 and 2×10^5 (commit 5e74d8f, built at eeeced7), before any held-out corpus was scored. Score them with a build whose profiler and `Config` match, or `score` refuses. |
| `results/h-lite-v2.dev-N.<corpus>.md` | The pre-registered held-out reports, one per frozen file and held-out corpus, read in research [0009](../0009-h-min-plain-wikipedia.md). |
| `frozen/h-lite-v3.dev-N.json`, `results/h-lite-v3.*` | H-lite `PROFILER_VERSION` 3 (s2w#250 PR 1): frozen on `dev` at the same windows, its `dev` profile table, and its `reserved-2` reports next to v2's on the same span. |
| `frozen/h-lite-v4.dev-N.json`, `results/h-lite-v4.*` | H-lite `PROFILER_VERSION` 4 (s2w#250 PR 2, the second entity test): frozen on `dev` at the same windows, its `dev` profile table, and its `reserved-3` reports, with v3's reports on the same span beside them. |
| `frozen/h-min-v5.dev-N.json`, `results/h-min-v5.*` | H-min `PROFILER_VERSION` 5 (s2w#244, stage 5b containment): frozen on `dev` after `reserved` was opened, its `dev` profile table, and its `reserved` reports. |
| `frozen/h-lite-v4.dev-N.pins-244.json`, `results/h-lite-v4.dev-N.reserved.md` | v4 re-frozen on `dev` under the pins after `reserved` was opened (only that pin line differs from `h-lite-v4.dev-N.json`, which `score` now refuses for `reserved`), and its `reserved` reports beside v5's. |

## Rules

- A key file never changes once scored. A change is a new file (`dev-key-v1.json`) and a new row
  in `keys.toml`, with the reason; the report lists every key it scored.
- The held-out and reserved corpora are opened only by a score run, never while building,
  tuning or writing a key. Their sha256 was committed before any of those (git log).
- Rebuild the corpora with `capture.sh 2026-09-28T00:00:00Z <dir>` only while EventStreams still
  retains that window (~7 days). The files carry a capture-time header, so a re-capture has new
  hashes; check them against `corpora.toml` and record any change as a new manifest entry.

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
  in a `"version": 1` spec. An alias rule (path not in its identity) cannot carry it, and an
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
