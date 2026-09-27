#!/usr/bin/env python3
"""Replay an EventStreams window with reconnect-by-Last-Event-ID. Filters enwiki.
usage: replay.py <stream> <since_iso> <until_iso> <out.ndjson>"""
import sys, json, time, urllib.request, datetime as dt

stream, since, until, out = sys.argv[1:5]
UA = 's2w-research/0.1 (davidlremy@gmail.com)'
until_t = dt.datetime.fromisoformat(until.replace('Z', '+00:00'))
last_id = None
n = 0
reconnects = 0
with open(out, 'w') as f:
    while True:
        url = f'https://stream.wikimedia.org/v2/stream/{stream}'
        if last_id is None:
            url += f'?since={since}'
        req = urllib.request.Request(url, headers={'User-Agent': UA, 'Accept': 'text/event-stream'})
        if last_id:
            req.add_header('Last-Event-ID', last_id)
        done = False
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                data = None
                pending_id = None  # committed to last_id only once its frame is fully processed
                for raw in r:
                    line = raw.decode('utf-8', 'replace').rstrip('\n')
                    if line.startswith('id: '):
                        pending_id = line[4:]
                    elif line.startswith('data: '):
                        data = line[6:]
                    elif line == '':
                        frame, data = data, None
                        frame_id, pending_id = pending_id, None
                        if frame is None:
                            continue
                        try:
                            ev = json.loads(frame)
                        except Exception:
                            if frame_id:
                                last_id = frame_id
                            continue
                        wiki = ev.get('wiki_id') or ev.get('database')
                        if wiki != 'enwiki':
                            if frame_id:
                                last_id = frame_id
                            continue
                        edt = ev.get('meta', {}).get('dt')
                        if edt and dt.datetime.fromisoformat(edt.replace('Z', '+00:00')) > until_t:
                            done = True
                            break
                        f.write(json.dumps(ev) + '\n')
                        n += 1
                        if frame_id:
                            last_id = frame_id
        except Exception as e:
            print('reconnect', reconnects, repr(e)[:120], flush=True)
        if done:
            break
        reconnects += 1
        time.sleep(2)
print('done', n, 'events', reconnects, 'reconnects', flush=True)
