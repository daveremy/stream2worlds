#!/usr/bin/env bash
# Demo: watch-wikipedia. Streams Wikipedia's live edit feed into a durable log for 20s, stops
# it, then restarts against the same log for 15s to show the cursor resumes instead of
# replaying from zero. See README.md in this directory for what to look for.
set -euo pipefail
cd "$(dirname "$0")/../.."

BIN="target/release/s2w"
if [[ ! -x "$BIN" ]]; then
  echo "error: $BIN not found. Build it first: cargo build --release" >&2
  exit 1
fi

# Under the repo, not /tmp: an s2w SQLite log dir must not live where a sandboxed run's /tmp
# could be shared or cleared mid-demo.
DATA_DIR=".demo-data/watch-wikipedia-$$"
mkdir -p "$DATA_DIR"
trap 'rm -rf "$DATA_DIR"' EXIT

# Best-effort: the log's own row count and stored cursor, read directly since s2w exposes no
# "how many events are stored" or "what cursor is stored" flag. Skipped if sqlite3 is absent;
# the demo still runs.
log_rows() {
  command -v sqlite3 >/dev/null && [[ -f "$DATA_DIR/events.sqlite3" ]] \
    && sqlite3 "$DATA_DIR/events.sqlite3" "select count(*) from events;" 2>/dev/null
}
log_cursor() {
  command -v sqlite3 >/dev/null && [[ -f "$DATA_DIR/events.sqlite3" ]] \
    && sqlite3 "$DATA_DIR/events.sqlite3" "select cursor from cursors limit 1;" 2>/dev/null
}

# Bounded run: the process reads a `live` stream forever (Ending::Never), so `timeout` is what
# stops it, not a natural exit. `-k 5` forces SIGKILL 5s after SIGTERM if the process ignores
# it, so shutdown itself is bounded too. GNU `timeout` exits 124 exactly when IT had to
# terminate the command because the duration elapsed. A clean 0 is accepted too (defensive:
# `watch` is documented to never end on its own for this source, but this demo must not
# mistake that for a hang if it ever does); any OTHER exit code -- a crash, a bad argument, a
# missing binary -- is a real failure, not the expected "ran for the demo's duration".
run_bounded() {
  local seconds=$1 rc=0
  timeout -k 5 "$seconds" "$BIN" watch wikipedia --log-dir "$DATA_DIR" || rc=$?
  if [[ "$rc" -ne 0 && "$rc" -ne 124 ]]; then
    echo "error: s2w watch exited $rc (expected 0 or 124)" >&2
    exit "$rc"
  fi
}

echo "== stream2worlds: watch-wikipedia demo =="
echo "log dir: $DATA_DIR (temporary, removed on exit)"
echo
echo "-- first run: 20s --"
run_bounded 20
FIRST_RUN_ROWS="$(log_rows || true)"
FIRST_RUN_CURSOR="$(log_cursor || true)"
[[ -n "$FIRST_RUN_ROWS" ]] && echo "log now holds $FIRST_RUN_ROWS events total"
[[ -n "$FIRST_RUN_CURSOR" ]] && echo "stored cursor: $FIRST_RUN_CURSOR"

echo
echo "-- restart against the same log dir: 15s --"
echo "watch the per-run total below start near 0 again (it is not cumulative across restarts) while the log's own row count keeps growing from where it left off"
echo "watch stderr above (once this run starts) for 's2w: wikipedia: resuming from stored cursor ...' -- that line, not just growing counts, is the direct evidence the restart used the saved cursor rather than reconnecting fresh"
run_bounded 15
SECOND_RUN_ROWS="$(log_rows || true)"
SECOND_RUN_CURSOR="$(log_cursor || true)"
# Report what actually happened, not what we hoped for: a claim of "advanced"/"grew" requires
# both readings to be present AND actually different -- an unchanged value (e.g. the live feed
# was momentarily idle, or sqlite3 is present but returned the same row twice) must say so
# honestly rather than silently reusing the growth-claiming sentence either way.
if [[ -n "$FIRST_RUN_ROWS" && -n "$SECOND_RUN_ROWS" ]]; then
  if [[ "$SECOND_RUN_ROWS" -gt "$FIRST_RUN_ROWS" ]]; then
    echo "log now holds $SECOND_RUN_ROWS events total (was $FIRST_RUN_ROWS before the restart) -- nothing was replayed from scratch"
  else
    echo "log now holds $SECOND_RUN_ROWS events total (was $FIRST_RUN_ROWS before the restart) -- unchanged or fewer; no new events landed in this window, so growth is not evidence here"
  fi
fi
if [[ -n "$FIRST_RUN_CURSOR" && -n "$SECOND_RUN_CURSOR" ]]; then
  if [[ "$SECOND_RUN_CURSOR" != "$FIRST_RUN_CURSOR" ]]; then
    echo "stored cursor advanced from $FIRST_RUN_CURSOR to $SECOND_RUN_CURSOR -- the restart resumed from it, it did not start over"
  else
    echo "stored cursor unchanged ($SECOND_RUN_CURSOR) -- no new events landed in this window; the 'resuming from stored cursor' line above is the resume evidence, not this comparison"
  fi
fi

echo
echo "== done =="
