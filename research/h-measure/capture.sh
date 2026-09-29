#!/usr/bin/env bash
# Captures the s2w#56 H-measure corpora (host: the hub, GNU date + sha256sum) from
# EventStreams history with research/scripts/eventstreams_replay.py, one after another:
# dev (2x10^5 events), then heldout, heldout-2 and reserved (10^5 each), each starting 6 h of
# stream time after the previous one's last event. Prints the corpora.toml stanza per corpus.
#
# usage: capture.sh <dev_since_iso> <out_dir>
# Corpora are not committed; research/h-measure/corpora.toml pins their sha256.
set -euo pipefail

since=$1
out=$2
here=$(cd "$(dirname "$0")" && pwd)
replay="$here/../scripts/eventstreams_replay.py"
stream=mediawiki.recentchange
mkdir -p "$out"

capture() { # name events since
  local name=$1 events=$2 from=$3 to summary first last file
  to=$(date -u -d "$from + 3 hours" +%Y-%m-%dT%H:%M:%SZ)
  file="$out/$name.raw.sse"
  summary=$(python3 "$replay" --all-wikis --raw-sse --max-events "$events" \
    "$stream" "$from" "$to" "$file" | grep '^done ')
  read -r _ n _ <<<"$summary"
  first=${summary#*first_dt=}; first=${first%% *}
  last=${summary##*last_dt=}
  if [ "$n" != "$events" ]; then
    echo "capture.sh: $name kept $n events, wanted $events" >&2
    exit 1
  fi
  printf '\n[corpus.%s]\nfile = "%s.raw.sse"\nstream = "%s"\nsince = "%s"\nuntil = "%s"\nfirst_dt = "%s"\nlast_dt = "%s"\nevents = %s\nbytes = %s\nsha256 = "%s"\n' \
    "$name" "$name" "$stream" "$from" "$to" "$first" "$last" "$n" \
    "$(stat -c %s "$file")" "$(sha256sum "$file" | cut -d' ' -f1)"
  next=$(date -u -d "$last + 6 hours" +%Y-%m-%dT%H:%M:%SZ)
}

capture dev 200000 "$since"
capture heldout 100000 "$next"
capture heldout-2 100000 "$next"
capture reserved 100000 "$next"
