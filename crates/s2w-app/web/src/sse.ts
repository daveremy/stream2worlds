import type { Message } from './api';

/// The SSE stream's final `event: error` frame, read from a finite `/events` body. `data` holds
/// the server's `{"error": ...}` JSON, so `isStaleEpoch`/`isBeforeBase` (state.ts) read it the
/// same way they read an EventSource error event.
export class SseErrorFrame extends Error {
  data: string;
  constructor(data: string) { super(`/events error frame: ${data}`); this.data = data; }
}

/// One frame's messages: its `data:` lines joined, or nothing for a comment-only frame. Pure.
function frameMessage(block: string): Message[] {
  let event = 'message';
  const data: string[] = [];
  for (const line of block.split(/\r?\n/)) {
    if (line.startsWith('data:')) data.push(line.slice(5).trimStart());
    else if (line.startsWith('event:')) event = line.slice(6).trim();
  }
  if (data.length === 0) return [];
  if (event === 'error') throw new SseErrorFrame(data.join('\n'));
  return [JSON.parse(data.join('\n')) as Message];
}

/// Parses a finite SSE body as it arrives (s2w#295): `push` takes each network chunk and returns
/// the messages of every frame it completed; a frame split across chunks, or a UTF-8 character
/// split across chunks, waits for the rest. `end` flushes a last frame with no trailing blank
/// line. Pure: no I/O, so tests feed it chunk boundaries directly.
export class SseParser {
  private decoder = new TextDecoder();
  private buffer = '';
  push(chunk: Uint8Array): Message[] {
    this.buffer += this.decoder.decode(chunk, { stream: true });
    const blocks = this.buffer.split(/\r?\n\r?\n/);
    this.buffer = blocks.pop()!;
    return blocks.flatMap(frameMessage);
  }
  end(): Message[] {
    this.buffer += this.decoder.decode();
    const rest = this.buffer; this.buffer = '';
    return rest.split(/\r?\n\r?\n/).flatMap(frameMessage);
  }
}
