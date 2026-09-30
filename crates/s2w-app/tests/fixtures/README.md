# Test fixtures (human-owned)

Never regenerate or edit a fixture here to make a test pass. A deliberate re-recording is a
reviewed change that re-pins its hash and counts in the same PR.

`wikipedia-page-change.jsonl` (pre-existing) is replayed by `../bridge_replay.rs`; this README
documents the recorded stream below.

## `recorded-10min.raw.sse` (s2w#174)

Ten minutes of the Wikimedia EventStreams `mediawiki.page_change.v1` stream, raw SSE,
byte-for-byte, IDs included, never re-synthesized. The scale gates replay it alongside the
seeded synthetic generator (`../support/scale_generator.rs`): the generator's distributions are
chosen, these are observed. Loader: `../support/recorded.rs`. Pins: `../recorded_fixture.rs`.

| | |
|---|---|
| Source | `https://stream.wikimedia.org/v2/stream/mediawiki.page_change.v1` |
| Stream | `mediawiki.page_change.v1`. Research 0006 §12 named `recentchange`; the preset and every recorded fixture in this repo use `page_change` instead. |
| Captured | 2026-09-29T11:48:41Z (the header comment), 600 s |
| Payload `dt` | first event 2026-09-29T11:48:39Z, last 2026-09-29T11:58:40Z (10 min 01 s). 4 events carry an older `dt` (the oldest is 2024-04-17T21:18:07Z); file order is replay order. |
| Events | 11,667 frames, each with `data` and `id` (about 19.4 per second) |
| Size | 34,164,103 bytes raw; 4.9 MB `gzip -9`. Git stores blobs compressed. |
| sha256 | `303529e889ed2de2531985b567ef549d6ff607306c9dd484afea5dabd295f9cf` |
| FNV-1a 64 | `0x070e_ba9f_7d43_edcd` (`FIXTURE_HASH`, checked by `cargo test`) |
| Canary frames | 0 |

Capture command (the first three lines of the file record it):

```bash
UA='s2w-fixture-capture/0.2 (email@daveremy.com)'
URL='https://stream.wikimedia.org/v2/stream/mediawiki.page_change.v1'
{ printf ': captured %s via\n: curl -s -N -H '\''User-Agent: %s'\'' --max-time 600 '\''%s'\''\n: real live traffic, byte-for-byte, IDs included; never re-synthesized\n' "$(date -u +%FT%TZ)" "$UA" "$URL"
  curl -s -N -H "User-Agent: $UA" --max-time 600 "$URL"; } > recorded-10min.raw.sse
```

The timeout cuts the stream mid-frame, so everything after the last blank line was dropped
(here nothing: the capture happened to end on a frame boundary). Wikimedia's robot policy asks
for a contact User-Agent; the command sends one.

Through the committed mapping (`recorded.mapping.json`, a symlink to
`s2w-system1/testdata/sample.mapping.json`, the mapping check 11 replays, so the two cannot
drift) the fixture yields 0 abstentions, 58,335 claims (35,001 entity-observed, 23,334
relationship-observed), and a world of 11,462 entities and 19,512 relationships.

The parse benchmark (s2w#166) maps it with `recorded-links.mapping.json` instead, a symlink to
`s2w-system1/testdata/sample-links.mapping.json` (the same mapping plus a second site entity and
a link merging it into the first): 81,669 claims, 11,667 of them link merges, 0 abstentions,
pinned in `../support/recorded_links.rs`.

### Licence and privacy

Edit metadata, page titles and edit comments are Wikimedia contributor content: **CC BY-SA
4.0** for Wikipedia-family wikis and **CC0 1.0** for Wikidata (the most frequent wiki in this
sample is `wikidatawiki`, then `commonswiki`). Attribution: Wikimedia Foundation and
contributors, via EventStreams. Usernames are public under Wikimedia's privacy policy.

19 `user_text` fields in the file are IP-address shaped (`grep -cE
'"user_text":"([0-9]{1,3}\.){3}[0-9]{1,3}"'`): edits made without an account, which the
stream publishes. They are left as recorded; a fixture is never edited.

`deny.toml` and `npm run licenses` cover crates and npm packages, not data, so neither lists
this file.
