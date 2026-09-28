import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildUrl, parseWorldFromPath, worldPathFor } from '../src/url.ts';

test('parseWorldFromPath extracts and percent-decodes the world segment', () => {
  assert.equal(parseWorldFromPath('/w/foo/'), 'foo');
  assert.equal(parseWorldFromPath('/w/my%20world/'), 'my world');
  assert.equal(parseWorldFromPath('/w/foo/bar'), 'foo');
  assert.equal(parseWorldFromPath('/'), undefined);
  assert.equal(parseWorldFromPath('/w/foo'), undefined); // no trailing slash: not yet canonical
  assert.equal(parseWorldFromPath('/worlds'), undefined);
});

test('worldPathFor percent-encodes the world into the canonical path', () => {
  assert.equal(worldPathFor('foo'), '/w/foo/');
  assert.equal(worldPathFor('my world'), '/w/my%20world/');
});

test('buildUrl strips world from the query and preserves every other param', () => {
  const params = new URLSearchParams('world=foo&at=5&branch=actual');
  assert.equal(buildUrl('/w/foo/', params), '/w/foo/?at=5&branch=actual');
});

test('buildUrl never reintroduces world even if a caller forgets to remove it first', () => {
  const params = new URLSearchParams('world=foo&hops=2');
  const url = buildUrl(worldPathFor('foo'), params);
  assert.ok(!url.includes('world='), `expected no world= in ${url}`);
  assert.equal(url, '/w/foo/?hops=2');
});

test('buildUrl on an empty params object still produces a query-string suffix', () => {
  assert.equal(buildUrl('/w/foo/', new URLSearchParams()), '/w/foo/?');
});

test('buildUrl does not mutate the params object passed in', () => {
  const params = new URLSearchParams('world=foo&at=5');
  buildUrl('/w/foo/', params);
  assert.equal(params.get('world'), 'foo');
});
