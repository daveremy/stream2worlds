import { test } from 'node:test';
import assert from 'node:assert/strict';
import { activeNow, labeledSummary, labelFor, labelMap, linkColor, nodeById, pickLabelKeys, topHubs, typeColor } from '../src/profile.ts';

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

// A 32-bit FNV-1a hash, kept local to the test (profile.ts's own `hash()` is unexported) so the
// fixture's ids look like real hashed identifiers, not small sequential integers a production
// stream would never produce.
function hashId(seed) {
  let result = 0x811c9dc5;
  for (const char of seed) { result ^= char.codePointAt(0); result = Math.imul(result, 0x01000193); }
  return result >>> 0;
}

test('pickLabelKeys is invariant under key-renaming AND id-hashing (decision 0018 obfuscation replay)', () => {
  // Mirrors the obfuscation replay decision 0018 requires at the Rust level: every field
  // renamed AND every id hashed. Entity ids below are hashed (not sequential 1/2/3/4), and no
  // two candidate keys tie on score/whitespace/distinct, so the assertion below proves the
  // SCORING (not a tie-break rule) survives both transformations.
  const nodes = [
    entity(hashId('alpha'), { title: { Str: 'Amber Stone' }, category: { Str: 'Group North' } }),
    entity(hashId('beta'), { title: { Str: 'Birch Field' }, category: { Str: 'Group North' } }),
    entity(hashId('gamma'), { title: { Str: 'Cobalt Lake' }, category: { Str: 'Group South' } }),
    entity(hashId('delta'), { title: { Str: 'Dune Ridge' }, category: { Str: 'Group South' } }),
  ];
  // Confirm the fixture actually has hash-shaped ids, not an accident of small seed strings.
  for (const node of nodes) assert.ok(node.entity > 0xffff, `expected a hash-sized id, got ${node.entity}`);
  const rename = key => `renamed_${[...key].reverse().join('')}`;
  const renamed = nodes.map((node, index) => ({
    ...node,
    entity: hashId(`renamed-${index}`), id: `entity:${hashId(`renamed-${index}`)}`,
    keys: [`key-${hashId(`renamed-${index}`)}`],
    attrs: Object.fromEntries(Object.entries(node.attrs).map(([key, value]) => [rename(key), value])),
  }));
  const expected = new Map([...pickLabelKeys(nodes)].map(([entityType, key]) =>
    [entityType, key === undefined ? undefined : rename(key)]));
  assert.deepEqual(pickLabelKeys(renamed), expected);
});

test('pickLabelKeys ranks score before whitespace', () => {
  const nodes = [
    entity(10, { phrase: { Str: 'Shared Value' }, token: { Str: 'Amber' } }),
    entity(20, { phrase: { Str: 'Shared Value' }, token: { Str: 'Birch' } }),
    entity(30, { phrase: { Str: 'Shared Value' }, token: { Str: 'Cobalt' } }),
    entity(40, { phrase: { Str: 'Shared Value' }, token: { Str: 'Dune' } }),
  ];
  assert.equal(pickLabelKeys(nodes).get('record'), 'token');
});

test('pickLabelKeys excludes a long prose attribute even with spaces and no digits/dashes (s2w#126)', () => {
  // isIdLike's length>80 early return must win regardless of whitespace or digit/dash content,
  // so a long free-text description never beats a genuinely unique short title on the tie-break.
  const nodes = [
    entity(10, { title: { Str: 'Adamant' },
      description: { Str: 'A long free text description field with plenty of prose words and no numbers here Amber' } }),
    entity(20, { title: { Str: 'Birchwood' },
      description: { Str: 'A long free text description field with plenty of prose words and no numbers here Birch' } }),
    entity(30, { title: { Str: 'Cobalton' },
      description: { Str: 'A long free text description field with plenty of prose words and no numbers here Cobalt' } }),
    entity(40, { title: { Str: 'Dunewood' },
      description: { Str: 'A long free text description field with plenty of prose words and no numbers here Dune' } }),
  ];
  for (const node of nodes) {
    assert.ok(node.attrs.description.Str.length > 80, 'fixture description must be >80 chars');
  }
  assert.equal(pickLabelKeys(nodes).get('record'), 'title');
});

test('pickLabelKeys keeps hex-letter-only words', () => {
  const nodes = [entity(10, { candidate: { Str: 'Ada' } }), entity(20, { candidate: { Str: 'cafe' } }),
    entity(30, { candidate: { Str: 'Beef' } })];
  assert.equal(pickLabelKeys(nodes).get('record'), 'candidate');
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

test('labelMap disambiguates duplicate labels only', () => {
  const nodes = [
    entity(482910, { candidate: { Str: 'Shared Name' } }),
    entity(9123055, { candidate: { Str: 'Shared Name' } }),
    entity(7301842, { candidate: { Str: 'Distinct Name' } }),
  ];
  assert.deepEqual(labelMap(nodes, pickLabelKeys(nodes)), new Map([
    ['entity:482910', 'Shared Name #482910'],
    ['entity:9123055', 'Shared Name #9123055'],
    ['entity:7301842', 'Distinct Name'],
  ]));
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
    { id: 1, label: 'Alpha One', count: 6 }, { id: 99, label: '#99', count: 6 },
    { id: 2, label: 'Beta Two', count: 5 }, { id: 3, label: 'Gamma Three', count: 4 },
    { id: 4, label: 'Delta Four', count: 3 },
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

test('topHubs uses visible degree when it exceeds recorded degree', () => {
  const nodes = [
    entity(10, { candidate: { Str: 'Amber One' } }, 'hub', 1),
    entity(20, { candidate: { Str: 'Birch Two' } }),
    entity(30, { candidate: { Str: 'Cobalt Three' } }),
  ];
  const links = [
    { source: 'entity:10', target: 'entity:20', kind: 'connected', weight: 1 },
    { source: 'entity:10', target: 'entity:30', kind: 'connected', weight: 1 },
  ];
  assert.deepEqual(topHubs(nodes, links, pickLabelKeys(nodes), 1), [{ label: 'Amber One', degree: 2 }]);
});
