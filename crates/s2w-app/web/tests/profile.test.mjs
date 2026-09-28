import { test } from 'node:test';
import assert from 'node:assert/strict';
import { activeNow, labeledSummary, labelFor, linkColor, nodeById, pickLabelKeys, topHubs, typeColor } from '../src/profile.ts';

function entity(entity, attrs, kind = 'entity', inDegree = 0) {
  const base = { kind, id: `entity:${entity}`, entity, entity_type: 'record', keys: [`key-${entity}`],
    attrs, members: [], hub_refs: [] };
  return kind === 'hub' ? { ...base, in_degree: inDegree, by_kind: {}, last_seen_offset: 0 } : base;
}

test('pickLabelKeys chooses the higher-cardinality string attribute', () => {
  const nodes = [
    entity(1, { alpha: { Str: 'Alpha One' }, beta: { Str: 'Group One' } }),
    entity(2, { alpha: { Str: 'Alpha Two' }, beta: { Str: 'Group One' } }),
    entity(3, { alpha: { Str: 'Alpha Three' }, beta: { Str: 'Group Two' } }),
    entity(4, { alpha: { Str: 'Alpha Four' }, beta: { Str: 'Group Two' } }),
  ];
  assert.equal(pickLabelKeys(nodes).get('record'), 'alpha');
});

test('pickLabelKeys returns undefined when a type has no string attributes', () => {
  assert.equal(pickLabelKeys([entity(1, { count: { Int: 3 }, ready: { Bool: true } })]).get('record'), undefined);
});

test('pickLabelKeys keeps a human label with one numeric-looking value', () => {
  const nodes = [entity(1, { candidate: { Str: '1984' } }), entity(2, { candidate: { Str: 'Alpha Two' } }),
    entity(3, { candidate: { Str: 'Alpha Three' } })];
  assert.equal(pickLabelKeys(nodes).get('record'), 'candidate');
});

test('pickLabelKeys prefers whitespace-bearing values over tied machine tokens', () => {
  const nodes = [
    entity(1, { alpha: { Str: 'https://example.test/1' }, zeta: { Str: 'Alpha One' } }),
    entity(2, { alpha: { Str: 'https://example.test/2' }, zeta: { Str: 'Alpha Two' } }),
  ];
  assert.equal(pickLabelKeys(nodes).get('record'), 'zeta');
});

test('labelFor falls back to entity keys', () => {
  assert.equal(labelFor(entity(1, {}), new Map()), 'key-1');
});

test('labeledSummary resolves protocol ids and preserves other fields', () => {
  const nodes = [entity(1, { candidate: { Str: 'Alpha One' } })];
  assert.equal(labeledSummary(
    { offset: 4, type: 'link', source: 1, target: 9, kind: 'connected', weight: 2 },
    nodeById(nodes), pickLabelKeys(nodes),
  ), JSON.stringify({ offset: 4, type: 'link', source: 'Alpha One', target: '#9', kind: 'connected', weight: 2 }));
});

test('profile colors are deterministic', () => {
  assert.equal(typeColor('record'), typeColor('record'));
  assert.equal(linkColor('connected'), linkColor('connected'));
});

test('activeNow and topHubs sort, truncate, and retain missing ids', () => {
  const nodes = [
    entity(1, { candidate: { Str: 'Alpha One' } }),
    entity(2, { candidate: { Str: 'Beta Two' } }, 'hub', 7),
    entity(3, { candidate: { Str: 'Gamma Three' } }),
    entity(4, { candidate: { Str: 'Delta Four' } }),
    entity(5, { candidate: { Str: 'Epsilon Five' } }),
  ];
  const keys = pickLabelKeys(nodes);
  const evidence = [
    { offset: 1, kind: 'link', entityIds: [1, 99, 2, 3, 4, 5, 6], summary: '{}' },
    { offset: 2, kind: 'link', entityIds: [1, 99, 2, 3, 4, 5], summary: '{}' },
    { offset: 3, kind: 'link', entityIds: [1, 99, 2, 3, 4], summary: '{}' },
    { offset: 4, kind: 'link', entityIds: [1, 99, 2, 3], summary: '{}' },
    { offset: 5, kind: 'link', entityIds: [1, 99, 2], summary: '{}' },
    { offset: 6, kind: 'link', entityIds: [1, 99], summary: '{}' },
  ];
  assert.deepEqual(activeNow(evidence, nodeById(nodes), keys), [
    { label: 'Alpha One', count: 6 }, { label: '#99', count: 6 }, { label: 'Beta Two', count: 5 },
    { label: 'Gamma Three', count: 4 }, { label: 'Delta Four', count: 3 },
  ]);

  const links = [
    { source: 'entity:1', target: 'entity:3', kind: 'connected', weight: 1 },
    { source: 'entity:1', target: 'entity:4', kind: 'connected', weight: 1 },
    { source: 'entity:1', target: 'entity:5', kind: 'connected', weight: 1 },
  ];
  assert.deepEqual(topHubs(nodes, links, keys, 2), [
    { label: 'Beta Two', degree: 7 }, { label: 'Alpha One', degree: 3 },
  ]);
});
