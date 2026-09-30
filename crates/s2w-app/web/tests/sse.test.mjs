import { test } from 'node:test';
import assert from 'node:assert/strict';
import { SseErrorFrame, SseParser } from '../src/sse.ts';
import { isStaleEpoch } from '../src/state.ts';

// s2w#295: the tail is parsed as it arrives, so frames and characters split across chunks.
const bytes = text => new TextEncoder().encode(text);
const frame = (offset, event = 'noop') =>
  `id: 00000000000000aa:${offset}\nevent: ${event}\ndata: {"offset":${offset},"type":"noop","event":"é—${offset}"}\n\n`;

// Feeds `body` to a parser split at every byte index in `cuts`, returning each push's offsets.
function feed(body, cuts) {
  const parser = new SseParser(), all = bytes(body), pushes = [];
  let from = 0;
  for (const cut of [...cuts, all.length]) { pushes.push(parser.push(all.slice(from, cut)).map(m => m.offset)); from = cut; }
  pushes.push(parser.end().map(m => m.offset));
  return pushes;
}

test('every split point of a two-frame body yields both messages exactly once, in order', () => {
  const body = frame(1) + frame(2);
  const size = bytes(body).length;
  for (let cut = 0; cut <= size; cut++) assert.deepEqual(feed(body, [cut]).flat(), [1, 2], `cut at ${cut}`);
});

test('a frame split mid-way waits for its second half', () => {
  const body = frame(1) + frame(2);
  const cut = bytes(frame(1)).length + 10;
  assert.deepEqual(feed(body, [cut]), [[1], [2], []]);
});

test('a UTF-8 character split across chunks decodes intact', () => {
  const body = frame(7);
  const all = bytes(body), at = all.indexOf(0xc3) + 1; // inside the two bytes of "é"
  const parser = new SseParser();
  assert.deepEqual(parser.push(all.slice(0, at)), []);
  const [message] = parser.push(all.slice(at));
  assert.equal(message.event, 'é—7');
});

test('CRLF line endings, split between CR and LF, still end a frame', () => {
  const body = frame(3).replaceAll('\n', '\r\n');
  const all = bytes(body);
  for (let cut = all.length - 4; cut < all.length; cut++) assert.deepEqual(feed(body, [cut]).flat(), [3], `cut at ${cut}`);
});

test('end() flushes a final frame with no trailing blank line; comment-only frames yield nothing', () => {
  const parser = new SseParser();
  assert.deepEqual(parser.push(bytes(': keep-alive\n\n' + frame(4).trimEnd())), []);
  assert.deepEqual(parser.end().map(m => m.offset), [4]);
});

test('an `event: error` frame throws, and a stale_epoch one reads as stale', () => {
  const parser = new SseParser();
  assert.throws(() => parser.push(bytes('event: error\ndata: {"error":"stale_epoch","message":"replaced"}\n\n')),
    error => error instanceof SseErrorFrame && isStaleEpoch(error));
});
