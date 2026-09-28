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

echo "== stream2worlds: watch-wikipedia demo =="
echo "log dir: $DATA_DIR (temporary, removed on exit)"
echo
echo "-- first run: 20s --"
timeout 20 "$BIN" watch wikipedia --log-dir "$DATA_DIR" || true
FIRST_RUN_ROWS="$(log_rows || true)"
FIRST_RUN_CURSOR="$(log_cursor || true)"
[[ -n "$FIRST_RUN_ROWS" ]] && echo "log now holds $FIRST_RUN_ROWS events total"
[[ -n "$FIRST_RUN_CURSOR" ]] && echo "stored cursor: $FIRST_RUN_CURSOR"

echo
echo "-- restart against the same log dir: 15s --"
echo "watch the per-run total below start near 0 again (it is not cumulative across restarts) while the log's own row count keeps growing from where it left off"
timeout 15 "$BIN" watch wikipedia --log-dir "$DATA_DIR" || true
SECOND_RUN_ROWS="$(log_rows || true)"
SECOND_RUN_CURSOR="$(log_cursor || true)"
if [[ -n "$FIRST_RUN_ROWS" && -n "$SECOND_RUN_ROWS" ]]; then
  echo "log now holds $SECOND_RUN_ROWS events total (was $FIRST_RUN_ROWS before the restart) -- nothing was replayed from scratch"
fi
if [[ -n "$FIRST_RUN_CURSOR" && -n "$SECOND_RUN_CURSOR" ]]; then
  echo "stored cursor advanced from $FIRST_RUN_CURSOR to $SECOND_RUN_CURSOR -- the restart resumed from it, it did not start over"
fi

echo
echo "== done =="
