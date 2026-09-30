# Demos

Every demo here runs from a clean clone with one command, needs no configuration, and finishes
in well under a minute. Each demo's own `README.md` says what to look for; `expected-output.txt`
is a captured real run — exact numbers vary (these read Wikipedia's live edit stream), the shape
does not.

| Demo | Type | Command | Shows |
|---|---|---|---|
| [watch-wikipedia](watch-wikipedia/) | `live` | `./demos/watch-wikipedia/run.sh` | Live stream → durable log; a progress line while healthy; resume across a restart |
| [local-routed-world](local-routed-world/) | `live` | `./demos/local-routed-world/run.sh [--keep]` | A world routed from the first event: propose + human-accept a mapping, then serve; `--keep` holds it up for `s2w-demo-check.sh --gates` |
| [serve-wikipedia](serve-wikipedia/) | `live` | `./demos/serve-wikipedia/run.sh` | One process ingests and serves a live, queryable world over HTTP |

Build once first: `cargo build --release`. Every `run.sh` fails fast with a build instruction
if the binary is missing — it never builds anything implicitly.

These read Wikipedia's real live feed, marked `live`: CI only lints the scripts (`bash -n`,
shellcheck), and never runs a demo itself. Run them yourself to see live output.

## Adding a demo

Every sprint that ships user-visible behaviour adds or extends a demo (karpathy def rule,
2026-09-27). A demo that runs on recorded/offline fixtures gets an `offline` `Type` and CI
actually runs it, not just lints it; a demo that reads a live external feed gets `live` and is
lint-only here (run it yourself). Follow the existing folder shape: `run.sh` (one command,
bounded runtime, no config), `README.md` (what it shows, why it matters, expected output), and
`expected-output.txt`. Add the new row to both this table and the top-level README's Demos
section.
