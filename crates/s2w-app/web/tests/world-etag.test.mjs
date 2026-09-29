import { test } from 'node:test';
import assert from 'node:assert/strict';

// `/world` answers 304 to its own tag (#216): a refresh sends the tag back and gets `null`.
test('a refresh sends the last tag of the same URL and a 304 resolves to null', async () => {
  globalThis.location = { origin: 'http://s2w.test' };
  const { snapshot, refreshSnapshot } = await import('../src/api.ts');
  const sent = [];
  const replies = [
    { status: 200, tag: '"a"', body: { offset: 1, nodes: [] } },
    { status: 304, tag: '"a"' },
    { status: 200, tag: '"b"', body: { offset: 2, nodes: [] } },
  ];
  globalThis.fetch = async (url, init) => {
    sent.push({ url: String(url), inm: init.headers?.['If-None-Match'], cache: init.cache });
    const reply = replies.shift();
    return new Response(reply.body ? JSON.stringify(reply.body) : null,
      { status: reply.status, headers: { ETag: reply.tag } });
  };
  const params = new URLSearchParams('world=w');
  assert.equal((await snapshot(params, new AbortController().signal)).offset, 1);
  assert.equal(await refreshSnapshot(params, new AbortController().signal), null);
  assert.equal((await refreshSnapshot(params, new AbortController().signal)).offset, 2);
  assert.deepEqual(sent.map(s => s.inm), [undefined, '"a"', '"a"']);
  assert.ok(sent.every(s => s.cache === 'no-store'));
  // Another URL never borrows this one's tag.
  replies.push({ status: 200, tag: '"c"', body: { offset: 2, nodes: [] } });
  await refreshSnapshot(new URLSearchParams('world=w&lod=type'), new AbortController().signal);
  assert.equal(sent.at(-1).inm, undefined);
});
