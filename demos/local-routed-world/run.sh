#!/usr/bin/env bash
# Demo: local-routed-world (s2w#309). Seeds a fresh log with a human-accepted stream mapping for
# the wikipedia source, then runs `s2w serve wikipedia` on it, so the world is routed from the
# first event instead of after discovery's 10,000-event window (~9 minutes live).
#
#   ./demos/local-routed-world/run.sh          bounded: show the routed world, stop, clean up
#   ./demos/local-routed-world/run.sh --keep   keep serving on port 4310 (or $S2W_PORT) until
#                                              Ctrl-C, for s2w-demo-check.sh --gates
#
# S2W_MAPPING overrides the mapping file (default: mapping.json here, the committed sample).
# See README.md in this directory.
set -euo pipefail
cd "$(dirname "$0")/../.."

KEEP=0
case "${1:-}" in
  "") ;;
  --keep) KEEP=1 ;;
  *) echo "usage: $0 [--keep]" >&2; exit 2 ;;
esac

BIN="target/release/s2w"
if [[ ! -x "$BIN" ]]; then
  echo "error: $BIN not found. Build it first: cargo build --release" >&2
  exit 1
fi
for tool in curl jq; do
  if ! command -v "$tool" >/dev/null; then
    echo "error: $tool is required" >&2
    exit 1
  fi
done

MAPPING="${S2W_MAPPING:-demos/local-routed-world/mapping.json}"
if [[ "$KEEP" == 1 ]]; then PORT="${S2W_PORT:-4310}"; else PORT="${S2W_PORT:-0}"; fi
SOURCE="wikipedia.page_change"

# Under the repo, not /tmp: an s2w SQLite log dir must not live where a sandboxed run's /tmp
# could be shared or cleared mid-demo.
DATA_DIR=".demo-data/local-routed-world-$$"
mkdir -p "$DATA_DIR"
LOG_FILE="$DATA_DIR/serve.log"

# Bounded shutdown, as in serve-wikipedia: signal, poll, SIGKILL after 5s.
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
  [[ -n "${PID:-}" ]] && stop_pid "$PID"
  rm -rf "$DATA_DIR"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

echo "== stream2worlds: local-routed-world demo =="
echo "-- 1. propose $MAPPING for source '$SOURCE' --"
ID=$("$BIN" proposals propose --log-dir "$DATA_DIR" --source "$SOURCE" \
  --mapping "$MAPPING" --author demo --json | jq -r '.proposal.id')
echo "proposal: $ID"

echo "-- 2. accept it as a human --"
"$BIN" proposals decide --log-dir "$DATA_DIR" --proposal "$ID" --outcome accept \
  --basis "local demo world (s2w#309)" --reviewer demo

echo "-- 3. serve --"
"$BIN" serve wikipedia --log-dir "$DATA_DIR" --port "$PORT" >"$LOG_FILE" 2>&1 &
PID=$!

URL=""
for _ in $(seq 1 50); do
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "error: the server exited before it started listening (port $PORT taken?)" >&2
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

# The world is routed from the start, so head should move within seconds of the first event.
HEAD=0
for _ in $(seq 1 60); do
  HEAD=$(curl -sf --max-time 5 "$URL/worlds" | jq -r '[.worlds[].head] | max // 0' || echo 0)
  [[ "$HEAD" =~ ^[0-9]+$ && "$HEAD" -gt 0 ]] && break
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "error: the server exited" >&2
    cat "$LOG_FILE" >&2
    exit 1
  fi
  sleep 1
done
if ! [[ "$HEAD" =~ ^[0-9]+$ && "$HEAD" -gt 0 ]]; then
  echo "error: head is still 0 after 60s; is the live feed reachable? server log:" >&2
  tail -n 20 "$LOG_FILE" >&2
  exit 1
fi
echo "routed: head $HEAD"

echo
echo "== world by type (curl $URL/worlds/default/world?lod=type | jq) =="
curl -sf --max-time 10 "$URL/worlds/default/world?lod=type" \
  | jq '{offset, types: [.nodes[] | {type: .entity_type, count}]}'

if [[ "$KEEP" == 1 ]]; then
  echo
  echo "serving until Ctrl-C (log: $LOG_FILE). In another shell, with a LifeOS checkout:"
  echo "  S2W_DEMO_URL=$URL bash ~/lifeos/scripts/s2w-demo-check.sh --gates"
  wait "$PID" || true
  PID=""
fi

echo
echo "== done =="
