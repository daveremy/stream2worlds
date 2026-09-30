import { test } from 'node:test';
import assert from 'node:assert/strict';
import { RefreshGuard, bootstrap, frameThrottle, liveRow } from '../src/bootstrap.ts';
import { loadWorld } from '../src/lod.ts';
import { ViewState } from '../src/state.ts';
import { renderTable } from '../src/table.ts';
import { renderActive } from '../src/active.ts';

// s2w#295: the viewer's load order, driven with recording fakes (lod.test.mjs style).
const EPOCH = '00000000000000aa', OTHER = '00000000000000bb';
const LARGE = 310_669;
const tick = () => new Promise(resolve => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const row = offset => ({ offset, type: 'entity', entity: offset, resolved: offset, minted: false });
const rows = (from, to) => Array.from({ length: to - from + 1 }, (_, i) => row(from + i));
const typeView = (offset, epoch, count = LARGE) => ({ offset, epoch, lod: 'type', focus: null, links: [],
  nodes: [{ kind: 'type', id: 'type:page', entity_type: 'page', count }] });

// One page load against fakes. `log` records every request and effect in order; `/world`
// requests stay pending in `worlds` until the test resolves them. `world(view)` answers every
// `/world` request, pending and later (the summary and the full view), with `view`. `status`
// follows main.ts's rule (#303): "Loading world…" until `mount`, which sets the note or ''.
function page({ at, paintEvidence } = {}) {
  const log = [], worlds = [];
  let answer, status = 'Loading world…';
  const params = new URLSearchParams(at === undefined ? 'world=w' : `world=w&at=${at}`);
  const state = new ViewState(params);
  const tail = deferred(), seed = deferred();
  let stream, loaded, aborted = false;
  const guard = new RefreshGuard(() => log.push(`refresh ${loaded.request}`));
  const deps = {
    at, state, guard, aborted: () => aborted,
    presentation: async () => { log.push('presentation'); },
    proposals: () => log.push('proposals'),
    sources: async () => { log.push('sources'); return []; },
    tail: s => { log.push('tail'); stream = s; return tail.promise; },
    evidence: offset => { log.push(`evidence at=${offset}`); return seed.promise; },
    loadWorld: onSummary => loadWorld(params, request => {
      const d = deferred(); worlds.push({ request: new URLSearchParams(request), d });
      log.push(`world ${request}`);
      if (answer) d.resolve(answer);
      return d.promise;
    }, onSummary),
    open: () => log.push(`open from=${state.lastAppliedOffset} epoch=${state.epoch}`),
    paintEvidence: paintEvidence ? () => { paintEvidence(state); log.push('paint'); } : () => log.push(`paint ${state.evidence.length}`),
    summary: view => log.push(`summary ${view.lod} status=${status}`),
    mount: l => { loaded = l; status = l.note ?? ''; log.push(`mount ${l.request}`); },
    restartStale: () => { aborted = true; log.push('restart'); },
  };
  const run = bootstrap(deps);
  run.catch(() => {});
  return {
    log, worlds, state, guard, run,
    head: (head, epoch = EPOCH) => stream.onHead(head, epoch),
    chunk: messages => stream.onRows(messages),
    close: (head, epoch = EPOCH, messages = []) => tail.resolve({ messages, head, epoch }),
    seed: (messages, head, epoch = EPOCH) => seed.resolve({ messages, head, epoch }),
    world: view => { answer = view; for (const { d } of worlds) d.resolve(view); },
    // Answers only the `/world` request `i`, leaving later ones pending.
    worldAt: (i, view) => worlds[i].d.resolve(view),
    status: () => status,
    live: message => liveRow(state, guard, message, deps.paintEvidence),
  };
}
// Streams a live tail of `from..to` in one chunk and closes it at `to`.
async function tailed(p, from, to, epoch = EPOCH) {
  p.head(to, epoch); p.chunk(rows(from, to)); p.close(to, epoch, rows(from, to)); await tick();
}

test('1: every request goes out at t=0, and the tail and live stream never wait on /world', async () => {
  const p = page();
  await tick();
  for (const request of ['presentation', 'proposals', 'sources', 'tail']) assert.ok(p.log.includes(request), request);
  assert.equal(p.worlds.length, 1, 'the world probe is in flight');
  await tailed(p, 1, 500);
  assert.ok(p.log.some(line => line.startsWith('open ')), 'the live stream opened before /world resolved');
  assert.equal(p.worlds.length, 1);
  assert.ok(!p.log.some(line => line.startsWith('mount')));
});

test('2: a live row before the probe resolves sends no /world request; after it, exactly one refresh', async () => {
  const p = page();
  await tailed(p, 1, 500);
  assert.equal(p.guard.loaded, false);
  assert.equal(p.live(row(501)), true);
  await tick();
  assert.equal(p.worlds.length, 1, 'only the probe went out');
  assert.ok(!p.log.some(line => line.startsWith('refresh')), p.log.join('\n'));
  for (const { request } of p.worlds) assert.ok(!(request.get('lod') === 'entity' && !request.has('focus')), `${request}`);
  p.world(typeView(480, EPOCH));
  await p.run;
  assert.equal(p.guard.loaded, true);
  assert.deepEqual(p.log.filter(line => line.startsWith('refresh')), ['refresh world=w&lod=type']);
  assert.ok(p.log.indexOf('mount world=w&lod=type') < p.log.indexOf('refresh world=w&lod=type'));
  p.live(row(502));
  assert.equal(p.log.filter(line => line.startsWith('refresh')).length, 2, 'after load, rows refresh directly');
});

test('2b: with no live row before the world, loading sends no refresh at all', async () => {
  const p = page();
  await tailed(p, 1, 500);
  p.world(typeView(500, EPOCH));
  await p.run;
  assert.ok(!p.log.some(line => line.startsWith('refresh')));
});

test('3: a pinned page reads evidence(at), never a tail, and opens no stream', async () => {
  const p = page({ at: 40 });
  await tick();
  assert.ok(p.log.includes('evidence at=40'));
  p.seed(rows(1, 40), 90);
  p.world(typeView(40, EPOCH));
  await p.run;
  assert.ok(!p.log.includes('tail') && !p.log.includes('sources'), p.log.join('\n'));
  assert.ok(!p.log.some(line => line.startsWith('open')));
  assert.equal(p.state.lastAppliedOffset, 40);
  assert.equal(p.state.evidence.length, 40);
});

test('3b: a pinned seed older than the server keeps (unknown epoch) adopts the world\'s', async () => {
  const p = page({ at: 40 });
  p.seed([], 40, '');
  p.world(typeView(40, EPOCH));
  await p.run;
  assert.ok(!p.log.includes('restart'));
  assert.equal(p.state.epoch, EPOCH);
});

test('4: a tail epoch other than the world\'s restarts the page, live and pinned, in either order', async () => {
  const tailFirst = page();
  await tailed(tailFirst, 1, 500, EPOCH);
  tailFirst.world(typeView(500, OTHER));
  await tailFirst.run;
  assert.ok(tailFirst.log.includes('restart'));
  assert.ok(!tailFirst.log.some(line => line.startsWith('mount')));

  const worldFirst = page();
  worldFirst.world(typeView(500, OTHER));
  await tick();
  worldFirst.head(500, EPOCH);
  assert.ok(worldFirst.log.includes('restart'), 'the tail headers alone name the mismatch');
  worldFirst.close(500, EPOCH);
  await worldFirst.run;
  assert.ok(!worldFirst.log.some(line => line.startsWith('open')));

  const pinned = page({ at: 40 });
  pinned.seed(rows(1, 40), 90, EPOCH);
  pinned.world(typeView(40, OTHER));
  await pinned.run;
  assert.ok(pinned.log.includes('restart'));
  assert.equal(pinned.log.filter(line => line === 'restart').length, 1);
});

test('5: a mismatch after live rows landed restarts, and the restarted page starts from an empty table', async () => {
  const p = page();
  await tailed(p, 1, 500);
  p.live(row(501)); p.live(row(502));
  p.world(typeView(480, OTHER));
  await p.run;
  assert.ok(p.log.includes('restart'));
  const after = p.log.length;
  p.chunk(rows(503, 510));
  assert.equal(p.log.length, after, 'the stale load paints nothing more');
  // The restart is a fresh start(): a new ViewState, so a new table.
  const next = page();
  assert.equal(next.state.evidence.length, 0);
  next.head(700, EPOCH); next.chunk([row(700)]);
  assert.deepEqual(next.log.filter(line => line.startsWith('paint')), ['paint 1']);
});

// A DOM just big enough for renderTable and renderActive (as in table.test.mjs).
class Element {
  constructor(tagName) { this.tagName = tagName; this.children = []; this.textContent = ''; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = [...children]; }
  insertRow(index = this.children.length) { const r = new Element('tr'); this.children.splice(index, 0, r); return r; }
  insertCell() { const cell = new Element('td'); this.children.push(cell); return cell; }
  get rows() { return this.children; }
  deleteRow(index) { this.children.splice(index, 1); }
}

test('6: the table and Active now render from tail rows before the renderer mounts', async () => {
  globalThis.document = { createElement: tagName => new Element(tagName), createTextNode: text => ({ text }) };
  const table = new Element('table'), active = new Element('div');
  const p = page({ paintEvidence: state => { renderTable(table, state); renderActive(active, state); } });
  p.head(3); p.chunk(rows(1, 3));
  const body = table.children[1];
  assert.equal(body.children.length, 3);
  const recent = active.children[1].children.map(item => item.textContent);
  assert.deepEqual(recent, ['#1 (1)', '#2 (1)', '#3 (1)']);
  assert.ok(!p.log.some(line => line.startsWith('mount')));
  p.close(3); p.world(typeView(3, EPOCH));
  await p.run;
  assert.ok(p.log.indexOf('paint') < p.log.findIndex(line => line.startsWith('mount')));
});

test('7: lastAppliedOffset never moves backward when the world lands below the tail head', async () => {
  const p = page();
  await tailed(p, 1, 500);
  assert.equal(p.state.lastAppliedOffset, 500);
  assert.ok(p.log.includes(`open from=500 epoch=${EPOCH}`));
  p.world(typeView(420, EPOCH));
  await p.run;
  assert.equal(p.state.offset, 420);
  assert.equal(p.state.lastAppliedOffset, 500);
  assert.equal(p.live(row(450)), false, 'a row the tail already applied is not applied again');
});

test('7b: an empty tail still opens the stream from its head', async () => {
  const p = page();
  p.head(1200); p.close(1200);
  await tick();
  assert.ok(p.log.includes(`open from=1200 epoch=${EPOCH}`));
});

test('8: the first paint happens on the first parsed chunk, before the tail body closes', async () => {
  const p = page();
  p.head(500);
  p.chunk(rows(1, 60));
  assert.deepEqual(p.log.filter(line => line.startsWith('paint')), ['paint 60']);
  assert.ok(!p.log.some(line => line.startsWith('open')), 'the tail has not closed');
  p.chunk(rows(61, 500));
  p.close(500);
  await tick();
  assert.equal(p.state.evidence.length, 500);
});

test('RefreshGuard holds requests until ready, then lets exactly one through', () => {
  let sent = 0;
  const guard = new RefreshGuard(() => sent++);
  guard.request(); guard.request();
  assert.equal(sent, 0);
  guard.ready();
  assert.equal(sent, 1);
  guard.ready();
  assert.equal(sent, 1);
  guard.request();
  assert.equal(sent, 2);
  const quiet = new RefreshGuard(() => sent++);
  quiet.ready();
  assert.equal(sent, 2);
});

test('frameThrottle runs at most once per scheduled frame', () => {
  const frames = [];
  let runs = 0;
  const paint = frameThrottle(() => runs++, run => frames.push(run));
  paint(); paint(); paint();
  assert.equal(frames.length, 1);
  assert.equal(runs, 0);
  frames.shift()();
  assert.equal(runs, 1);
  paint();
  assert.equal(frames.length, 1);
});

test('a world view below the tail head gets one refresh even on a quiet log, in either order', async () => {
  const tailFirst = page();
  await tailed(tailFirst, 1, 500);
  tailFirst.world(typeView(420, EPOCH));
  await tailFirst.run;
  assert.deepEqual(tailFirst.log.filter(line => line.startsWith('refresh')), ['refresh world=w&lod=type']);

  const worldFirst = page();
  worldFirst.world(typeView(420, EPOCH));
  await tick();
  assert.ok(worldFirst.log.some(line => line.startsWith('mount')));
  assert.ok(!worldFirst.log.some(line => line.startsWith('refresh')));
  await tailed(worldFirst, 1, 500);
  await worldFirst.run;
  assert.deepEqual(worldFirst.log.filter(line => line.startsWith('refresh')), ['refresh world=w&lod=type']);
});

test('a pinned world view never refreshes', async () => {
  const p = page({ at: 40 });
  p.seed(rows(1, 40), 90);
  p.world(typeView(30, EPOCH));
  await p.run;
  assert.ok(!p.log.some(line => line.startsWith('refresh')));
});

// #303: the summary is drawn first, but the page is not loaded, and the status not cleared,
// until the full type view lands.
const summaryView = (offset, epoch, count = LARGE) => ({ ...typeView(offset, epoch, count), links: [] });

test('a large world draws the summary under "Loading world…", then mounts the full type view', async () => {
  const p = page();
  await tailed(p, 1, 500);
  p.live(row(501));
  assert.equal(String(p.worlds[0].request), 'world=w&lod=type&links=none');
  p.worldAt(0, summaryView(480, EPOCH));
  await tick();
  assert.deepEqual(p.log.filter(line => line.startsWith('summary')), ['summary type status=Loading world…']);
  assert.equal(p.status(), 'Loading world…');
  assert.equal(p.guard.loaded, false, 'the summary does not make the page loaded');
  assert.ok(!p.log.some(line => line.startsWith('mount') || line.startsWith('refresh')));
  assert.equal(p.worlds.length, 2);
  assert.equal(String(p.worlds[1].request), 'world=w&lod=type');
  p.worldAt(1, typeView(480, EPOCH));
  await p.run;
  assert.equal(p.guard.loaded, true);
  assert.match(p.status(), /too many to draw/);
  assert.deepEqual(p.log.filter(line => line.startsWith('summary')).length, 1);
  assert.ok(p.log.findIndex(line => line.startsWith('summary')) < p.log.indexOf('mount world=w&lod=type'));
  // Refreshes use the full type view, never the summary.
  assert.deepEqual(p.log.filter(line => line.startsWith('refresh')), ['refresh world=w&lod=type']);
});

test('a small world goes summary then entity, draws no summary, and clears the status', async () => {
  const p = page();
  await tailed(p, 1, 500);
  p.worldAt(0, summaryView(500, EPOCH, 40));
  await tick();
  assert.deepEqual(p.worlds.map(w => String(w.request)), ['world=w&lod=type&links=none', 'world=w&lod=entity']);
  p.worldAt(1, { offset: 500, epoch: EPOCH, lod: 'entity', focus: null, links: [], nodes: [] });
  await p.run;
  assert.ok(!p.log.some(line => line.startsWith('summary')));
  assert.equal(p.status(), '');
  assert.ok(p.log.includes('mount world=w&lod=entity'));
});

test('a summary from another history restarts the page and draws nothing', async () => {
  const p = page();
  await tailed(p, 1, 500, EPOCH);
  p.worldAt(0, summaryView(480, OTHER));
  await tick();
  assert.ok(p.log.includes('restart'));
  assert.ok(!p.log.some(line => line.startsWith('summary')));
});
