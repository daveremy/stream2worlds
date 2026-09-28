# Demo: serve-wikipedia

**What it shows:** `s2w serve wikipedia` ingests Wikipedia's live edit feed, runs it through the
System 1 bridge, and serves the resulting world over a loopback HTTP query API — one process,
one command.

**Why it matters:** this is the shape every agent-facing use of `s2w` builds on: a live,
queryable world, not a batch export.

## Run it

```bash
cargo build --release   # once, or after a code change
./demos/serve-wikipedia/run.sh
```

Takes about 20 seconds. Requires `curl` and `jq` on `PATH`. No configuration, no API keys.

## What to look for

1. **A progress line every 5 seconds** on stderr while the server runs (same mechanism as the
   `watch-wikipedia` demo — s2w#87).
2. **`curl $URL/world | jq`** returns a live node/link count for whatever Wikipedia has edited
   in the last few seconds.

## Known gap (out of scope here, follow-up filed)

The nodes today are Wikidata identifiers (`Q62072440`, `Lexeme:L…`), not readable page titles,
and cover every wiki mixed together — so "what's being edited most on English Wikipedia right
now?" is not yet an answerable question from this output alone. A wiki filter and
human-readable titles are tracked in
[#91](https://github.com/daveremy/stream2worlds/issues/91).

## Expected output

See `expected-output.txt` for a full real run. The exact node/link counts vary every time (it
is reading Wikipedia's live edit stream); the shape — a progress line every ~5s, then a
`{nodes, links}` summary — is stable.
