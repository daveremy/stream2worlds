import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isStaleEpoch } from '../src/state.ts';

test('a stale_epoch ApiError or SSE error frame is recognised; nothing else is', () => {
  // ApiError shape (HTTP 410 body `{"error":"stale_epoch"}`).
  assert.equal(isStaleEpoch({ status: 410, code: 'stale_epoch', message: 'x' }), true);
  // The SSE stream's final `event: error` frame, as the browser hands it to `onerror`.
  assert.equal(isStaleEpoch({ type: 'error', data: '{"error":"stale_epoch","message":"x"}' }), true);
  for (const other of [
    undefined, null, 'stale_epoch', new Error('stale_epoch'),
    { status: 410, code: 'offset_before_base' },
    { type: 'error' }, // a plain connection error carries no data
    { data: '{"error":"stream_limit"}' }, { data: 'not json' }, { data: 'null' },
  ]) assert.equal(isStaleEpoch(other), false, JSON.stringify(other));
});

test('eventsUrl pins the epoch when given one and leaves bare offsets bare', async () => {
  globalThis.location = new URL('http://127.0.0.1:8080/w/default/');
  const { eventsUrl } = await import('../src/api.ts');
  const params = new URLSearchParams('world=default&at=9');
  const pinned = eventsUrl(params, 3, 7, '1111111111111111');
  assert.equal(pinned.pathname, '/worlds/default/events');
  assert.equal(pinned.searchParams.get('epoch'), '1111111111111111');
  assert.equal(pinned.searchParams.get('from'), '3');
  assert.equal(pinned.searchParams.get('at'), '7');
  const bare = eventsUrl(params, 3, undefined, '');
  assert.equal(bare.searchParams.has('epoch'), false);
  assert.equal(bare.searchParams.has('at'), false);
});
