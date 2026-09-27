#!/usr/bin/env python3
"""Revert pilot (research note 0004). Reads the output of eventstreams_replay.py for
mediawiki.page_change.v1 (pc.ndjson) and mediawiki.page_revert_risk_prediction_change.v1
(rr.ndjson), both filtered to enwiki, and prints the pilot measurements.

Pilot approximations (the contract's real matcher replaces them):
- receipt time is approximated by meta.dt (the replay cannot observe live receipt);
- revert ranges are compared by rev_id, not by (rev_timestamp, rev_id);
- a revert whose oldest reverted revision predates the replay window is still matched by id.

usage: revert_pilot.py pc.ndjson rr.ndjson COHORT_START_ISO COHORT_END_ISO
"""
import datetime as d
import json
import statistics as st
import sys
from collections import Counter

pc_path, rr_path, start, end = sys.argv[1:5]


def ts(s):
    return d.datetime.fromisoformat(s.replace('Z', '+00:00')).timestamp()


def load(path):
    out = []
    for line in open(path):
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    return out


def q(a, f):
    return round(a[min(len(a) - 1, int(f * len(a)))], 1)


ev = load(pc_path)
revs = [e for e in ev if e.get('page_change_kind') in ('edit', 'create') and 'revision' in e]
reverts = [e for e in revs if e['revision'].get('revert')]
print('revisions', len(revs), 'reverting', len(reverts),
      dict(Counter(r['revision']['revert'].get('method') for r in reverts)))

t0, t1 = ts(start), ts(end)
cohort = [e for e in revs if e['page_change_kind'] == 'edit' and e['page']['namespace_id'] == 0
          and not e['revision']['editor'].get('is_bot') and t0 <= ts(e['revision']['rev_dt']) < t1]
arr = sorted(ts(e['meta']['dt']) - ts(e['revision']['rev_dt']) for e in cohort)
print('cohort', len(cohort), 'arrival s p50', q(arr, .5), 'p90', q(arr, .9), 'p99', q(arr, .99),
      'max', round(arr[-1], 1), '>60s', sum(x > 60 for x in arr))

by_page = {}
for r in reverts:
    by_page.setdefault(r['page']['page_id'], []).append(r)

labels, delays, pre, bot, self_ = {}, [], 0, 0, 0
for e in cohort:
    rid = e['revision']['rev_id']
    t = ts(e['revision']['rev_dt'])
    cutoff = ts(e['meta']['dt']) + 15
    rv = lambda r: r['revision']['revert']
    hits = sorted((r for r in by_page.get(e['page']['page_id'], [])
                   if rv(r).get('rev_reverted_oldest_id', 1e18) <= rid <= rv(r).get('rev_reverted_newest_id', -1)),
                  key=lambda r: (r['revision']['rev_dt'], r['revision']['rev_id']))
    if hits:
        r = hits[0]
        if ts(r['revision']['rev_dt']) <= cutoff:
            pre += 1
            continue
        if ts(r['revision']['rev_dt']) - t <= 1800:
            labels[rid] = 1
            delays.append((ts(r['revision']['rev_dt']) - t) / 60)
            bot += bool(r['revision']['editor'].get('is_bot'))
            self_ += r['revision']['editor'].get('user_text') == e['revision']['editor'].get('user_text')
            continue
    labels[rid] = 0

pos = sum(labels.values())
print('ineligible (reverted by cutoff)', pre, 'eligible', len(labels), 'positives', pos,
      'base rate', round(pos / len(labels), 4))
delays.sort()
print('edit->revert min p25', q(delays, .25), 'p50', q(delays, .5), 'p90', q(delays, .9),
      'bot reverts', bot, 'self reverts', self_)
anon = [e for e in cohort if e['revision']['rev_id'] in labels
        and (e['revision']['editor'].get('is_temp') or not e['revision']['editor'].get('user_id'))]
print('temp/anon eligible', len(anon), 'positives', sum(labels[e['revision']['rev_id']] for e in anon))

rr = {x['revision']['rev_id']: x for x in load(rr_path)}
cov = [e for e in cohort if e['revision']['rev_id'] in labels and e['revision']['rev_id'] in rr]
lag = sorted(ts(rr[e['revision']['rev_id']]['meta']['dt']) - ts(e['revision']['rev_dt']) for e in cov)
in_time = sum(ts(rr[e['revision']['rev_id']]['meta']['dt']) <= ts(e['meta']['dt']) + 15 for e in cov)
y = [labels[e['revision']['rev_id']] for e in cov]
p = [rr[e['revision']['rev_id']]['predicted_classification']['probabilities']['true'] for e in cov]
base = sum(y) / len(y)
b0 = sum((base - v) ** 2 for v in y)
b2 = sum((a - v) ** 2 for a, v in zip(p, y))
pos_p = [a for a, v in zip(p, y) if v]
neg_p = [a for a, v in zip(p, y) if not v]
auc = sum((a > b) + 0.5 * (a == b) for a in pos_p for b in neg_p) / max(1, len(pos_p) * len(neg_p))
print('B2 coverage', len(cov), '/', len(labels), 'by cutoff', in_time, 'lag s p50', q(lag, .5), 'p90', q(lag, .9))
print('B2 raw mean p', round(st.mean(p), 3), 'Brier', round(b2 / len(y), 4),
      'base-rate Brier', round(b0 / len(y), 4), 'ROC AUC', round(auc, 3))
