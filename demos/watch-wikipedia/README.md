# Demo: watch-wikipedia

**What it shows:** `s2w watch wikipedia` streams Wikipedia's live edit feed into a durable
SQLite log, prints a progress line while healthy, and resumes from its stored cursor after a
restart instead of starting over.

**Why it matters:** the log, not the process, is the source of truth. Stopping and restarting
`s2w` never loses or replays already-seen events — the property every other `s2w` command
(`serve`, `mcp`, a future dashboard) depends on.

## Run it

```bash
cargo build --release   # once, or after a code change
./demos/watch-wikipedia/run.sh
```

Takes about 35 seconds. No configuration, no API keys — Wikipedia's EventStreams feed is
public. Requires `sqlite3` on `PATH` to print the log's row count and stored cursor (skipped,
not fatal, if absent).

## What to look for

1. **A progress line every 5 seconds** on stderr: events/s, this run's own total, and time
   since the last event. Before this issue (s2w#87), a healthy run printed nothing and read as
   a hang.
2. **`s2w: wikipedia: resuming from stored cursor ...` on stderr, right when the second run
   starts.** This is the direct proof of resume: the source itself reports which cursor it is
   reconnecting from, before it has processed a single new event — not an inference from
   counts that would also grow if the restart reconnected at the live stream's current
   position instead. The first run instead logs `no stored cursor; starting fresh`.
3. **The log's row count keeps growing across the restart** (`log now holds N events total`) —
   the second run's own per-process total resets near zero, but the persisted log does not.
4. **The stored cursor advances, it does not reset** (`stored cursor advanced from ... to
   ...`) — corroborating evidence alongside point 2.

## Expected output

See `expected-output.txt` for a full real run. The exact counts vary every time (it is reading
Wikipedia's live edit stream), but the shape — a progress line every ~5s, a growing log total,
a clean restart — is stable.
