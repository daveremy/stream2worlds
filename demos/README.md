# Demos

Every demo here runs from a clean clone with one command, needs no configuration, and finishes
in well under a minute. Each demo's own `README.md` says what to look for; `expected-output.txt`
is a captured real run — exact numbers vary (these read Wikipedia's live edit stream), the shape
does not.

| Demo | Command | Shows |
|---|---|---|
| [watch-wikipedia](watch-wikipedia/) | `./demos/watch-wikipedia/run.sh` | Live stream → durable log; a progress line while healthy; resume across a restart |
| [serve-wikipedia](serve-wikipedia/) | `./demos/serve-wikipedia/run.sh` | One process ingests and serves a live, queryable world over HTTP |

Build once first: `cargo build --release`. Every `run.sh` fails fast with a build instruction
if the binary is missing — it never builds anything implicitly.

These are offline-shaped but not CI-recorded: they read Wikipedia's real live feed, so CI only
lints the scripts (`bash -n`, shellcheck). Run them yourself to see live output.
