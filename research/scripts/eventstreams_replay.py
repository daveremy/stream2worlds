#!/usr/bin/env python3
"""Replay an EventStreams window with reconnect-by-Last-Event-ID.

usage: eventstreams_replay.py [--all-wikis] [--raw-sse] [--max-events N]
                              <stream> <since_iso> <until_iso> <out>

Default (the revert pilot, research 0004): keeps enwiki only and writes parsed NDJSON.
--all-wikis   keep every wiki (no filter).
--raw-sse     write each kept frame's lines as received (UTF-8, LF line ends) plus the blank
              line that ends it, under a `:` comment header naming the capture, so
              `s2w_sources::replay_frames` reads the file as the live SSE adapter reads the
              stream (s2w#56).
--max-events  stop after N kept events (0 = no limit); `until` still ends the window.

An event past `until` is skipped; the replay stops once every topic in the stream's id has
passed `until` (or at --max-events).

Prints one summary line on stdout when done:
  done <events> events <reconnects> reconnects first_dt=<min dt> last_dt=<max dt>
"""
import argparse, datetime as dt, http.client, json, sys, time, urllib.error, urllib.request

UA = 's2w-research/0.1 (davidlremy@gmail.com)'


def parse_args(argv):
    p = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    p.add_argument('--all-wikis', action='store_true')
    p.add_argument('--raw-sse', action='store_true')
    p.add_argument('--max-events', type=int, default=0)
    p.add_argument('stream')
    p.add_argument('since')
    p.add_argument('until')
    p.add_argument('out')
    return p.parse_args(argv)


def event_wiki(ev):
    return ev.get('wiki_id') or ev.get('database')


def topics(frame_id):
    """The topics an EventStreams id assigns (one cursor per datacenter topic), or None when the
    id does not read, so an unreadable id never counts as every topic being done."""
    try:
        found = {cursor['topic'] for cursor in json.loads(frame_id)}
    except Exception:
        return None
    return found or None


def after(edt, until_t):
    return bool(edt) and dt.datetime.fromisoformat(edt.replace('Z', '+00:00')) > until_t


# Only a dropped or failed connection is retried; anything else (a bad timestamp, a full disk)
# stops the capture instead of reconnecting forever at the same Last-Event-ID.
NETWORK_ERRORS = (urllib.error.URLError, http.client.HTTPException, ConnectionError, TimeoutError)


def main(argv):
    args = parse_args(argv)
    until_t = dt.datetime.fromisoformat(args.until.replace('Z', '+00:00'))
    last_id = None
    n = 0
    reconnects = 0
    first_dt = last_dt = None
    past_until = set()
    with open(args.out, 'w', encoding='utf-8', newline='') as f:
        if args.raw_sse:
            started = dt.datetime.now(dt.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
            f.write(f': captured {started} via research/scripts/eventstreams_replay.py\n')
            f.write(f': {" ".join(argv)}\n')
            f.write(': real traffic from EventStreams history; each kept frame\'s lines as received, IDs included (UTF-8, LF line ends)\n\n')
        while True:
            url = f'https://stream.wikimedia.org/v2/stream/{args.stream}'
            if last_id is None:
                url += f'?since={args.since}'
            req = urllib.request.Request(url, headers={'User-Agent': UA, 'Accept': 'text/event-stream'})
            if last_id:
                req.add_header('Last-Event-ID', last_id)
            done = False
            try:
                with urllib.request.urlopen(req, timeout=60) as r:
                    data = []  # the frame's data lines, joined with \n as replay_frames does
                    pending_id = None  # committed to last_id only once its frame is fully processed
                    lines = []  # the frame's lines, for --raw-sse
                    for raw in r:
                        line = raw.decode('utf-8', 'replace').rstrip('\n').rstrip('\r')
                        if line.startswith(':'):
                            continue
                        if line != '':
                            lines.append(line)
                            field, _, value = line.partition(':')
                            value = value[1:] if value.startswith(' ') else value
                            if field == 'id':
                                pending_id = value
                            elif field == 'data':
                                data.append(value)
                            continue
                        frame = '\n'.join(data) if data else None
                        frame_id, pending_id, data = pending_id, None, []
                        frame_lines, lines = lines, []
                        if frame is None:
                            continue
                        try:
                            ev = json.loads(frame)
                        except Exception:
                            ev = None
                        if frame_id:
                            last_id = frame_id
                        if not isinstance(ev, dict):
                            continue
                        meta = ev.get('meta') or {}
                        edt = meta.get('dt')
                        if after(edt, until_t):
                            # A history replay delivers each datacenter topic in turn, not
                            # interleaved by time (measured 2026-09-29): one topic passing
                            # `until` ends only that topic. Stop once every topic has.
                            if meta.get('topic'):
                                past_until.add(meta['topic'])
                            assigned = topics(frame_id)
                            if assigned is not None and assigned <= past_until:
                                done = True
                                break
                            continue
                        if not args.all_wikis and event_wiki(ev) != 'enwiki':
                            continue
                        if args.raw_sse:
                            f.write('\n'.join(frame_lines) + '\n\n')
                        else:
                            f.write(json.dumps(ev) + '\n')
                        n += 1
                        if edt:
                            first_dt = min(first_dt or edt, edt)
                            last_dt = max(last_dt or edt, edt)
                        if args.max_events and n >= args.max_events:
                            done = True
                            break
            except NETWORK_ERRORS as e:
                print('reconnect', reconnects, repr(e)[:120], flush=True)
            if done:
                break
            reconnects += 1
            time.sleep(2)
    print(f'done {n} events {reconnects} reconnects first_dt={first_dt} last_dt={last_dt}', flush=True)


if __name__ == '__main__':
    main(sys.argv[1:])
