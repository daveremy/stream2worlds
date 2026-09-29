import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ViewState, isStaleEpoch } from '../src/state.ts';

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

test('a fresh view state has no epoch until the page sets one', () => {
  const state = new ViewState(new URLSearchParams());
  assert.equal(state.epoch, '');
});
