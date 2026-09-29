# H-measure data (s2w#56)

Data for `cargo xtask h-measure`, which grades a stream mapping against an answer key
(contract B3). The code is domain-free (decision 0018); everything that knows what
`mediawiki.recentchange` means is in this directory.

| File | What |
|---|---|
| `corpora.toml` | The four corpora (dev, heldout, heldout-2, reserved): role, window, event count, byte size, sha256. The corpora themselves are not committed. |
| `capture.sh` | The command that produced them, with `research/scripts/eventstreams_replay.py --all-wikis --raw-sse --max-events N`. |
| `keys.toml` | Every answer key's sha256, pinned before any score is run, and the reading of #17 it encodes. |
| `dev-key-v0.json` | dev-key v0: the base key (key-spec format v0, `xtask/src/h_measure/key.rs`). |
| `dev-key-v0.<variant>.json` | Sensitivity variants. Each differs from the base only as `keys.toml` says; `diff` the files to see the variant. |
| `dev-key-v1.json` | dev-key v1: the v0 base key in key-spec format 1, with `"no_identity": [0]` on the `log_id` mention (#225). The v0 files stay as they are. |
| `dev-key-v1.<variant>.json` | Each v0 sensitivity variant in format 1, with the same `no_identity` on `log_id`, pinned before any score. |

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

## Scoring a frozen mapping

```
cargo xtask h-measure score --mapping FILE --corpus NAME --key FILE [--key FILE ...] [--json FILE] [--dir DIR]
```

`score` refuses, before it reads the corpus: pins that changed since the freeze, a mapping not
frozen on the pinned development corpus, the `reserved` corpus, no `--key`, and a key that is
not pinned or does not match its pin. An abstaining mapping is graded as the empty prediction.
The markdown report opens with the alias limit, then gives per key the mapping and ceiling
rows (with and without singleton-only types), per type, per path, context collisions, and the
spurious, abstained and excluded counts; `--json` writes every number.

## Context collisions

The unfloored composite-key sub-metric (evaluation contract, cross-wiki composite-key
discovery; accepted as proposed on s2w#56, 2026-09-29). A row is one key type and one
**context path**: an identity path of a mention rule whose identity has two or more paths and
includes the rule's own path, other than that path. Drop the context part from each gold
entity's identity; gold entities of the type that are then equal form a **collision group**,
the entities a mapping keying the type without the context would merge. The row scores B-cubed
on every gold mention (alias paths included) of the entities in groups of two or more, with
the graded mapping's and the ceiling's predictions both restricted to those mentions. A row
with no group is listed with every metric undefined. A key's excluded (`no_identity`) mentions
are dropped from every prediction first (#225), so they never form a group.

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
