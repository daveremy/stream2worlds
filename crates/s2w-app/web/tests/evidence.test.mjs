import { test } from 'node:test';
import assert from 'node:assert/strict';

const frame = offset => `id: 00000000000000aa:${offset}\nevent: noop\ndata: {"offset":${offset},"type":"noop","event":"x"}\n\n`;

// s2w#294: the live seed is one `last=500` request, and both seeds return the resolved head and epoch.
test('evidenceTail asks for the last 500 and reads S2W-Head and S2W-Epoch', async () => {
  globalThis.location = { origin: 'http://s2w.test' };
  const { evidenceTail } = await import('../src/api.ts');
  const sent = [];
  globalThis.fetch = async url => {
    sent.push(new URL(String(url)));
    return new Response(frame(41) + frame(42),
      { status: 200, headers: { 'S2W-Head': '42', 'S2W-Epoch': '00000000000000aa' } });
  };
  const got = await evidenceTail(new URLSearchParams('world=w&from=3&at=9&epoch=ff'), new AbortController().signal);
  assert.deepEqual(got.messages.map(m => m.offset), [41, 42]);
  assert.equal(got.head, 42);
  assert.equal(got.epoch, '00000000000000aa');
  const q = sent[0].searchParams;
  assert.equal(q.get('last'), '500');
  assert.ok(!q.has('from') && !q.has('at') && !q.has('epoch'), sent[0].href);
});

test('an empty tail still names the head and epoch', async () => {
  globalThis.location = { origin: 'http://s2w.test' };
  const { evidenceTail } = await import('../src/api.ts');
  globalThis.fetch = async () =>
    new Response('', { status: 200, headers: { 'S2W-Head': '0', 'S2W-Epoch': '00000000000000bb' } });
  const got = await evidenceTail(new URLSearchParams('world=w'), new AbortController().signal);
  assert.deepEqual(got, { messages: [], head: 0, epoch: '00000000000000bb' });
});

test('pinned evidence reads the headers, and a 410 below the base is an empty seed', async () => {
  globalThis.location = { origin: 'http://s2w.test' };
  const { evidence } = await import('../src/api.ts');
  globalThis.fetch = async () =>
    new Response(frame(7), { status: 200, headers: { 'S2W-Head': '90', 'S2W-Epoch': '00000000000000aa' } });
  const pinned = await evidence(new URLSearchParams('world=w&at=7'), 7, '00000000000000aa', new AbortController().signal);
  assert.deepEqual(pinned, { messages: [pinned.messages[0]], head: 90, epoch: '00000000000000aa' });
  assert.equal(pinned.messages[0].offset, 7);
  globalThis.fetch = async () => new Response(JSON.stringify({ error: 'offset_before_base', message: 'gone' }),
    { status: 410, headers: { 'Content-Type': 'application/json' } });
  const gone = await evidence(new URLSearchParams('world=w'), 7, undefined, new AbortController().signal);
  assert.deepEqual(gone, { messages: [], head: 7, epoch: '' });
});
