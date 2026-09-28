import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ViewState, unroutedStatus } from '../src/state.ts';

test('evidence retains 500 offsets and reconnect replay is idempotent', () => {
  const state = new ViewState(new URLSearchParams());
  for (let offset = 1; offset <= 700; offset++) {
    assert.equal(state.apply({ offset, type: 'entity', entity: offset, resolved: offset, minted: true }), true);
  }
  assert.equal(state.evidence.length, 500);
  assert.equal(state.evidence[0].offset, 201);
  assert.deepEqual(state.evidence[0].entityIds, [201]);
  assert.equal(state.apply({ offset: 700, type: 'noop', event: 'EntitiesMerged' }), false);
  assert.equal(state.evidence.length, 500);
  assert.equal(state.lastAppliedOffset, 700);
});

test('deltas leave the complete graph snapshot intact even for merge and hub transitions', () => {
  const state = new ViewState(new URLSearchParams('at=1&lod=type&focus=1'));
  state.snapshot({ offset: 1, nodes: [{ kind: 'type', id: 'type:page', entity_type: 'page', count: 1 }], links: [] });
  const nodes = state.nodes;
  state.apply({ offset: 2, type: 'hub_ref', source: 2, hub: 1, kind: 'edited', tripped: true, in_degree: 4 });
  state.apply({ offset: 3, type: 'merge', survivor: 1, absorbed: 2 });
  state.apply({ offset: 4, type: 'split', survivor: 1, absorbed: 2 });
  assert.equal(state.nodes, nodes);
  assert.equal(state.offset, 1);
  assert.deepEqual(state.evidence.map(row => row.kind), ['hub_ref', 'merge', 'split']);
});

test('unroutedStatus names the busiest unrouted source, ignores routed and idle ones', () => {
  assert.equal(unroutedStatus([]), undefined);
  assert.equal(
    unroutedStatus([{ source: 'stdin', consumed: 4, unrouted: 0, recent_unrouted: [] }]),
    undefined,
    'consumed but fully routed is not the unrouted state',
  );
  assert.equal(
    unroutedStatus([{ source: 'kafka.orders', consumed: 0, unrouted: 0, recent_unrouted: [] }]),
    undefined,
    'no traffic at all stays the genuine idle case',
  );
  assert.equal(
    unroutedStatus([
      { source: 'a', consumed: 10, unrouted: 10, recent_unrouted: [] },
      { source: 'b', consumed: 4000, unrouted: 4000, recent_unrouted: [] },
    ]),
    '4000 events logged from b, no engine routed yet (#71)',
    'the busiest unrouted source wins when several are unrouted',
  );
});
