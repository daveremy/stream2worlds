# H-measure data (s2w#56)

Data for `cargo xtask h-measure`, which grades a stream mapping against an answer key
(contract B3). The code is domain-free (decision 0018); everything that knows what
`mediawiki.recentchange` means is in this directory.

| File | What |
|---|---|
| `corpora.toml` | The four corpora (dev, heldout, heldout-2, reserved): window, event count, byte size, sha256. The corpora themselves are not committed. |
| `capture.sh` | The command that produced them, with `research/scripts/eventstreams_replay.py --all-wikis --raw-sse --max-events N`. |
| `keys.toml` | Every answer key's sha256, pinned before any score is run, and the reading of #17 it encodes. |
| `dev-key-v0.json` | dev-key v0: the base key (key-spec format v0, `xtask/src/h_measure/key.rs`). |
| `dev-key-v0.<variant>.json` | Sensitivity variants. Each differs from the base only as `keys.toml` says; `diff` the files to see the variant. |

## Rules

- A key file never changes once scored. A change is a new file (`dev-key-v1.json`) and a new row
  in `keys.toml`, with the reason; the report lists every key it scored.
- The held-out and reserved corpora are opened only by a score run, never while building,
  tuning or writing a key. Their sha256 was committed before any of those (git log).
- Rebuild the corpora with `capture.sh 2026-09-28T00:00:00Z <dir>` only while EventStreams still
  retains that window (~7 days). The files carry a capture-time header, so a re-capture has new
  hashes; check them against `corpora.toml` and record any change as a new manifest entry.

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

**Open question for PR 2b (not settled here):** 1,348 of the dev corpus's 19,684 log events are
AbuseFilter hits with `log_id` 0 (no log row is written). Base key v0 reads them as one log
entity per wiki. Format v0 cannot exclude a value, so fixing this needs either a format change
or a key that drops `log_id`; either is a new key file, decided before the first score.
Tracked in #225; PR 2b blocks its first score on it, and `dev-key-v0` stays as it is here.
