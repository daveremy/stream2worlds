import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ENTITY_VIEW_LIMIT, bounded, entityCount, loadWorld, outgrown, servedLod, submittedLod } from '../src/lod.ts';

// #262: the demo world's default entity view was 370 MB / 310,669 nodes and hung the browser.
const typeView = count => ({ offset: 9, lod: 'type', focus: null, links: [], nodes: [
  { kind: 'type', id: 'type:page', entity_type: 'page', count: count - 1 },
  { kind: 'hub', id: 'e:1', entity: 1, entity_type: 'wiki', keys: [], attrs: {}, members: [1], hub_refs: [],
    in_degree: 9, by_kind: {}, last_seen_offset: 9 },
] });
const entityView = n => ({ offset: 9, lod: 'entity', focus: null, links: [],
  nodes: Array.from({ length: n }, (_, i) => ({ kind: 'entity', id: `e:${i}` })) });
// A fake `/world`: records every request, answers a world of `size` entities.
function server(size) {
  const sent = [];
  const fetchView = async request => {
    sent.push(new URLSearchParams(request));
    return request.get('lod') === 'type' ? typeView(size) : entityView(request.has('focus') ? 3 : size);
  };
  return { sent, fetchView };
}
const LARGE = 310_669, SMALL = 40;

test('the default page request on a large world is bounded and shows types with a note', async () => {
  const { sent, fetchView } = server(LARGE);
  const loaded = await loadWorld(new URLSearchParams('world=default'), fetchView);
  assert.ok(sent.length > 0);
  for (const request of sent) assert.ok(bounded(request), `unbounded request: ${request}`);
  assert.equal(loaded.view.lod, 'type');
  assert.equal(loaded.request.get('lod'), 'type');
  assert.match(loaded.note, /310,669 entities/);
});

test('no page parameters ever send an unbounded request to a large world', async () => {
  for (const lod of [undefined, 'entity', 'type']) for (const focus of [undefined, '7']) {
    const params = new URLSearchParams('world=default&hops=2');
    if (lod) params.set('lod', lod);
    if (focus) params.set('focus', focus);
    const { sent, fetchView } = server(LARGE);
    const loaded = await loadWorld(params, fetchView);
    for (const request of sent) assert.ok(bounded(request), `${params} sent ${request}`);
    assert.ok(bounded(loaded.request), `${params} refreshes with ${loaded.request}`);
  }
});

test('a small world keeps the entity view by default, after a type probe', async () => {
  const { sent, fetchView } = server(SMALL);
  const loaded = await loadWorld(new URLSearchParams('world=default'), fetchView);
  assert.deepEqual(sent.map(r => r.get('lod')), ['type', 'entity']);
  assert.equal(loaded.request.has('links'), false);
  assert.equal(loaded.view.lod, 'entity');
  assert.equal(loaded.note, undefined);
});

test('the limit itself is allowed; one more entity is not', async () => {
  assert.equal((await loadWorld(new URLSearchParams(), server(ENTITY_VIEW_LIMIT).fetchView)).view.lod, 'entity');
  assert.equal((await loadWorld(new URLSearchParams(), server(ENTITY_VIEW_LIMIT + 1).fetchView)).view.lod, 'type');
});

test('a focus and an unknown level go as asked, in one request', async () => {
  for (const query of ['lod=entity&focus=7&hops=2', 'lod=cluster']) {
    const { sent, fetchView } = server(LARGE);
    const loaded = await loadWorld(new URLSearchParams(query), fetchView);
    assert.deepEqual(sent.map(String), [query]);
    assert.equal(String(loaded.request), query);
  }
});

test('a world that grows past the limit between the probe and the entity fetch shows types', async () => {
  const sent = [];
  const fetchView = async request => { sent.push(String(request));
    return request.get('lod') === 'type' ? typeView(SMALL) : entityView(ENTITY_VIEW_LIMIT + 1); };
  const loaded = await loadWorld(new URLSearchParams(), fetchView);
  assert.deepEqual(sent, ['lod=type&links=none', 'lod=entity', 'lod=type']);
  assert.equal(String(loaded.request), 'lod=type');
  assert.equal(loaded.view.lod, 'type');
  assert.equal(loaded.request.get('lod'), 'type');
  assert.match(loaded.note, /5,001 entities/);
});

test('entityCount counts a type by its count and a hub as one', () => {
  assert.equal(entityCount(typeView(12)), 12);
  assert.equal(entityCount(entityView(4)), 4);
});

test('outgrown flags an unfocused entity view past the limit, and nothing else', () => {
  const unfocused = new URLSearchParams('lod=entity');
  assert.equal(outgrown(unfocused, entityView(ENTITY_VIEW_LIMIT)), false);
  assert.equal(outgrown(unfocused, entityView(ENTITY_VIEW_LIMIT + 1)), true);
  assert.equal(outgrown(new URLSearchParams('lod=entity&focus=1'), entityView(ENTITY_VIEW_LIMIT + 1)), false);
  assert.equal(outgrown(new URLSearchParams('lod=type'), entityView(ENTITY_VIEW_LIMIT + 1)), false);
});

// #269: the Detail selector read "Entities" while the page drew the type view of a huge world.
test('the Detail selector shows Types when a large world falls back, and Entities stays bounded', async () => {
  for (const lod of [undefined, 'entity']) {
    const params = new URLSearchParams('world=default');
    if (lod) params.set('lod', lod);
    const { sent, fetchView } = server(LARGE);
    const loaded = await loadWorld(params, fetchView);
    assert.equal(servedLod(loaded), 'type');
    assert.match(loaded.note, /too many to draw/);
    for (const request of sent) assert.ok(bounded(request), `unbounded request: ${request}`);
  }
});

test('the Detail selector shows what was served on small worlds and focused views', async () => {
  assert.equal(servedLod(await loadWorld(new URLSearchParams('world=default'), server(SMALL).fetchView)), 'entity');
  assert.equal(servedLod(await loadWorld(new URLSearchParams('world=default&lod=type'), server(SMALL).fetchView)), 'type');
  assert.equal(servedLod(await loadWorld(new URLSearchParams('world=default&focus=7'), server(LARGE).fetchView)), 'entity');
});

test('a submit keeps the asked level while the selector shows an untouched fallback', () => {
  // Fallback left as shown: adding a Focus must still ask for the entity neighbourhood.
  assert.equal(submittedLod('type', 'type', 'entity'), 'entity');
  // The user re-picks Entities on the huge world: sent as entity, and loadWorld bounds it again.
  assert.equal(submittedLod('entity', 'type', 'entity'), 'entity');
  // No fallback: the selection goes as picked.
  assert.equal(submittedLod('type', 'entity', 'entity'), 'type');
  assert.equal(submittedLod('entity', 'type', 'type'), 'entity');
});

// #303: the type summary (`lod=type&links=none`, #296) is the size probe and is drawn first.
const summarized = () => { const drawn = []; return { drawn, onSummary: view => drawn.push(view) }; };

test('every request is bounded and the first one is the summary', async () => {
  for (const size of [SMALL, LARGE]) for (const lod of [undefined, 'entity', 'type']) {
    const params = new URLSearchParams('world=default');
    if (lod) params.set('lod', lod);
    const { sent, fetchView } = server(size);
    await loadWorld(params, fetchView, summarized().onSummary);
    assert.equal(String(sent[0]), String(new URLSearchParams({ world: 'default', lod: 'type', links: 'none' })), `${params}`);
    // A small world's entity view is unbounded by shape but gated by the summary's count.
    if (size === LARGE) for (const request of sent) assert.ok(bounded(request), `${params} sent ${request}`);
  }
});

test('a small world sends summary then entity, and draws no summary', async () => {
  const { sent, fetchView } = server(SMALL);
  const { drawn, onSummary } = summarized();
  const loaded = await loadWorld(new URLSearchParams('world=default'), fetchView, onSummary);
  assert.deepEqual(sent.map(String), ['world=default&lod=type&links=none', 'world=default&lod=entity']);
  assert.equal(drawn.length, 0);
  assert.equal(loaded.view.lod, 'entity');
});

test('a large world sends summary then the full type view, and draws the summary once', async () => {
  const { sent, fetchView } = server(LARGE);
  const { drawn, onSummary } = summarized();
  const loaded = await loadWorld(new URLSearchParams('world=default'), fetchView, onSummary);
  assert.deepEqual(sent.map(String), ['world=default&lod=type&links=none', 'world=default&lod=type']);
  assert.equal(drawn.length, 1);
  assert.equal(entityCount(drawn[0]), LARGE);
  assert.match(loaded.note, /310,669 entities/);
});

test('the served request after load is the full type view, never the summary', async () => {
  for (const [size, query] of [[LARGE, 'world=default'], [LARGE, 'world=default&lod=type'], [SMALL, 'world=default&lod=type'],
    [LARGE, 'world=default&lod=type&links=none']]) {
    const loaded = await loadWorld(new URLSearchParams(query), server(size).fetchView, summarized().onSummary);
    assert.equal(String(loaded.request), 'world=default&lod=type', `${query}`);
  }
});

test('onSummary is called before the full type view is requested', async () => {
  const order = [];
  const fetchView = async request => { order.push(`fetch ${request}`); return typeView(LARGE); };
  await loadWorld(new URLSearchParams('lod=type'), fetchView, () => order.push('summary'));
  assert.deepEqual(order, ['fetch lod=type&links=none', 'summary', 'fetch lod=type']);
});
