# Demo: local-routed-world

**What it shows:** a local `s2w serve` whose world is routed from its very first event. The
script proposes a stream mapping for the wikipedia source with `s2w proposals propose`, accepts
it as a human with `s2w proposals decide`, then serves the live feed on that log.

**Why it matters:** on a fresh log, `s2w serve wikipedia` routes nothing until discovery has
seen 10,000 events (about 9 minutes of live Wikipedia, [decision 0025](../../docs/decisions/0025-learned-mapping-auto-apply.md)),
so the world's head stays at 0 and the viewer has nothing to draw. A human-accepted mapping
routes the source immediately ([decision 0023](../../docs/decisions/0023-routes-from-stored-mappings.md)),
and discovery leaves a routed source alone. That makes viewer timing measurable on your own
machine before a deploy (s2w#309).

## Run it

```bash
cargo build --release   # once, or after a code change
./demos/local-routed-world/run.sh
```

Takes about 10 seconds. Requires `curl` and `jq` on `PATH` and network access to Wikipedia's
live feed. No configuration, no API keys. The data directory under `.demo-data/` is deleted on
exit.

## Measure the viewer against it

```bash
./demos/local-routed-world/run.sh --keep          # serves on 127.0.0.1:4310 until Ctrl-C
# in another shell:
S2W_DEMO_URL=http://127.0.0.1:4310 bash ~/lifeos/scripts/s2w-demo-check.sh --gates
```

The check is not part of this repo: it is `scripts/s2w-demo-check.sh` in the LifeOS checkout
(`~/lifeos` on the hub). Without it, `--keep` still gives you a routed world to point any
viewer or HTTP client at.

`--keep` serves on port 4310, or on `$S2W_PORT`; the script exits with an error if the port is
taken. The check's page check needs `node` and a headless
Chrome (`chrome-headless-shell` under `~/.cache/puppeteer` or `~/.cache/ms-playwright`, or
`$S2W_BROWSER`); without one, `--gates` reports UNKNOWN instead of a timing line. A real run
against this demo (2026-09-29):

```
ok   world default head > 0
timing world default: first_paint_ms=642 first_paint_adj_ms=636 graph_ms=642 graph_adj_ms=636 rtt_ms=3
ok   world default first_paint_adj_ms <= 1000 (max 636)
ok   world default graph_adj_ms <= 3000 (max 636)
ok   world default head advances (325 -> 2930 in 30s)
demo: PASS (1 world(s))
```

`S2W_MAPPING=<file>` runs a different mapping, for example
`crates/s2w-system1/testdata/sample-links.mapping.json` (the v2 mapping with links).

## What to look for

1. **`source 'wikipedia.page_change' now runs mapping …`** right after the accept: the route
   exists before `serve` starts.
2. **`routed: head N`** with N above 0 within seconds of the server starting.
3. **A per-type count** (`page`, `site`, `user`) from `/worlds/default/world?lod=type`.

## Expected output

See `expected-output.txt` for a real run. The port, proposal ids and counts vary with the live
feed; the proposal id and mapping identity stay the same for the same mapping file.
`mapping.json` is a symlink to the committed test fixture
`crates/s2w-system1/testdata/sample.mapping.json`, so a change to that fixture changes both.
