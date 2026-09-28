#!/usr/bin/env bash
# Demo: serve-wikipedia. Runs `s2w serve wikipedia` for 15s, then asks its live query API for
# a world summary with curl + jq. See README.md in this directory for what to look for.
set -euo pipefail
cd "$(dirname "$0")/../.."

BIN="target/release/s2w"
if [[ ! -x "$BIN" ]]; then
  echo "error: $BIN not found. Build it first: cargo build --release" >&2
  exit 1
fi
if ! command -v jq >/dev/null; then
  echo "error: jq is required (https://jqlang.org)" >&2
  exit 1
fi

# Under the repo, not /tmp: an s2w SQLite log dir must not live where a sandboxed run's /tmp
# could be shared or cleared mid-demo.
DATA_DIR=".demo-data/serve-wikipedia-$$"
mkdir -p "$DATA_DIR"
LOG_FILE="$DATA_DIR/serve.log"

# Bounded shutdown: send the signal, then poll for exit instead of `wait`ing indefinitely --
# a process that ignores the signal must not hang the demo forever. Escalates to SIGKILL after
# a 5s grace period.
stop_pid() {
  local pid=$1 waited=0
  kill "$pid" 2>/dev/null || return 0
  while kill -0 "$pid" 2>/dev/null; do
    if [[ "$waited" -ge 50 ]]; then
      kill -9 "$pid" 2>/dev/null || true
      break
    fi
    sleep 0.1
    waited=$((waited + 1))
  done
  wait "$pid" 2>/dev/null || true
}
cleanup() {
  [[ -n "${TAIL_PID:-}" ]] && stop_pid "$TAIL_PID"
  [[ -n "${PID:-}" ]] && stop_pid "$PID"
  rm -rf "$DATA_DIR"
}
trap cleanup EXIT

echo "== stream2worlds: serve-wikipedia demo =="
"$BIN" serve wikipedia --log-dir "$DATA_DIR" --port 0 >"$LOG_FILE" 2>&1 &
PID=$!

URL=""
for _ in $(seq 1 50); do
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "error: the server process exited before it started listening" >&2
    cat "$LOG_FILE" >&2
    exit 1
  fi
  URL=$(grep -m1 -oE 'http://[0-9.]+:[0-9]+' "$LOG_FILE" 2>/dev/null || true)
  [[ -n "$URL" ]] && break
  sleep 0.2
done
if [[ -z "$URL" ]]; then
  echo "error: server did not report a listening address in time" >&2
  cat "$LOG_FILE" >&2
  exit 1
fi
echo "server: $URL"

# Stream the server's own stderr (its progress line included) live, not just on failure.
tail -n +1 -f "$LOG_FILE" &
TAIL_PID=$!

echo "ingesting for 15s..."
sleep 15

stop_pid "$TAIL_PID"
TAIL_PID=""

echo
echo "== world summary (curl $URL/worlds/default/world | jq) =="
curl -sf --max-time 10 "$URL/worlds/default/world" | jq '{nodes: (.nodes | length), links: (.links | length)}'

echo
echo "== done =="
