import { test } from 'node:test';
import assert from 'node:assert/strict';
import { KIND_ICON, changesText, displayText, feedSentences, fromDashboard, manifestLabel, pollRetries,
  sentenceActive, typeNodeLabel, withIcon } from '../src/manifest.ts';
import { ViewState } from '../src/state.ts';
import { renderLive } from '../src/live.ts';

class Element {
  constructor(tagName) { this.tagName = tagName; this.children = []; this.textContent = ''; this.attrs = {}; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = [...children]; }
  insertRow(index = this.children.length) { const row = new Element('tr'); this.children.splice(index, 0, row); return row; }
  insertCell() { const cell = new Element('td'); this.children.push(cell); return cell; }
  get rows() { return this.children; }
  deleteRow(index) { this.children.splice(index, 1); }
}
globalThis.document = { createElement: tagName => new Element(tagName) };

const dashboard = { manifest: {
  domain: { name: 'A world', summary: 's' },
  types: [
    { type: 'page', primary: true, noun: 'page', label: { attr: 'title' }, kind: 'document' },
    { type: 'user', primary: true, noun: 'editor', label: { key: 0 }, kind: 'person' },
    { type: 'aside', primary: false, noun: 'aside', kind: 'other' },
  ],
  events: [{ source: 's', sentence: { text: '{0}', fields: [['a']] } }],
} };
const manifest = fromDashboard(dashboard);

function entity(entity, entity_type, keys, attrs = {}) {
  return { kind: 'entity', id: `entity:${entity}`, entity, entity_type, keys, attrs, members: [], hub_refs: [] };
}

test('display text follows the Rust vectors', () => {
  const vectors = [
    ['/* Early life */ fixed a typo', 'fixed a typo'], ['a/*x*/b /* y */  c', 'a b c'], ['/* only a span */', ''],
    ['  two\t\nlines  ', 'two lines'], ['open /* never closed', 'open /* never closed'], ['', ''],
    ['Tucson,_Arizona', 'Tucson, Arizona'], ['Draft:Battle_of_the_Wall', 'Draft:Battle of the Wall'],
    ['_lead__trail_', 'lead trail'], ['___', ''], ['keep snake_case here', 'keep snake_case here'],
    ['/* s */ Some_Title', 'Some Title'], ['plain', 'plain'],
  ];
  for (const [raw, shown] of vectors) assert.equal(displayText(raw), shown, raw);
});

test('every kind has an icon and no manifest means no icon', () => {
  for (const kind of ['person', 'document', 'category', 'place', 'organisation', 'event', 'other']) {
    assert.ok(KIND_ICON[kind], kind);
  }
  assert.equal(Object.keys(KIND_ICON).length, 7);
  assert.equal(withIcon(undefined, 'page', 'x'), 'x');
  assert.equal(withIcon(manifest, 'page', 'x'), `${KIND_ICON.document} x`);
  assert.equal(withIcon(manifest, 'unknown', 'x'), 'x');
});

test('a dashboard with no manifest keeps today\'s view', () => {
  assert.equal(fromDashboard({ manifest: null }), undefined);
  assert.equal(fromDashboard(undefined), undefined);
  assert.equal(fromDashboard({ manifest: { ...dashboard.manifest, events: null } }).sentences, false);
  assert.equal(manifest.sentences, true);
});

test('manifest labels read an attribute or a key part, as display text', () => {
  const page = entity(1, 'page', ['page\u001f7'], { title: { Str: 'Tucson,_Arizona' } });
  const user = entity(2, 'user', ['user\u001f"Some_One"\u001f3']);
  assert.equal(manifestLabel(page, manifest), 'Tucson, Arizona');
  assert.equal(manifestLabel(user, manifest), 'Some One');
  assert.equal(manifestLabel(entity(3, 'page', ['page\u001f8'], { title: { Str: ' ' } }), manifest), undefined);
  assert.equal(manifestLabel(entity(4, 'page', ['page\u001f9'], { title: { Int: 5 } }), manifest), '5');
  assert.equal(manifestLabel(entity(5, 'aside', ['aside\u001f1']), manifest), undefined);
  assert.equal(manifestLabel(page, undefined), undefined);
  assert.equal(typeNodeLabel({ kind: 'type', id: 't', entity_type: 'user', count: 3 }, manifest),
    `${KIND_ICON.person} editor (3)`);
  assert.equal(typeNodeLabel({ kind: 'type', id: 't', entity_type: 'nope', count: 3 }, manifest), undefined);
});

test('the snapshot labels nodes from the manifest, falling back to the heuristic', () => {
  const state = new ViewState(new URLSearchParams());
  const nodes = [entity(1, 'page', ['page\u001f7'], { title: { Str: 'A_Page' } }),
    entity(2, 'other', ['other\u001f"x"'], { name: { Str: 'Named' } })];
  state.snapshot({ offset: 1, nodes, links: [] });
  assert.equal(state.labels.get('entity:1'), 'A_Page', 'the heuristic shows the raw value');
  state.manifest = manifest; state.relabel();
  assert.equal(state.labels.get('entity:1'), 'A Page');
  state.snapshot({ offset: 2, nodes, links: [] });
  assert.equal(state.labels.get('entity:1'), 'A Page', 'kept across snapshots');
});

const rows = [
  { position: 1, source: 's', sentence: 'first', entities: [{ type: 'page', key: 'k1', entity: 1, label: 'One' }] },
  { position: 2, source: 's', sentence: null, entities: [{ type: 'aside', key: 'a', entity: 9 }] },
  { position: 3, source: 's', sentence: 'third', entities: [
    { type: 'page', key: 'k1', entity: 1, label: 'One' }, { type: 'page', key: 'k1', entity: 1, label: 'One' },
    { type: 'user', key: 'u', entity: 2 }, { type: 'user', key: 'v' }] },
];

test('the feed is newest first and skips rows with no sentence', () => {
  assert.deepEqual(feedSentences(rows), ['third', 'first']);
});

test('active now counts primary entities once per event', () => {
  const items = sentenceActive(rows, manifest, id => (id === 2 ? 'Two' : undefined));
  assert.deepEqual(items, [
    { type: 'page', label: 'One', count: 2 },
    { type: 'user', label: 'Two', count: 1 },
    { type: 'user', label: 'v', count: 1 },
  ]);
  assert.deepEqual(sentenceActive([{ position: 1, source: 's', sentence: 'x', entities: [{ type: 'user', key: 'u', entity: 4 }] }],
    manifest, () => undefined), [{ type: 'user', label: '#4', count: 1 }]);
  assert.equal(changesText(1), '1 change');
  assert.equal(changesText(22), '22 changes');
});

test('the feed renders under an explicit tbody; no feed renders today\'s table', () => {
  const state = new ViewState(new URLSearchParams());
  state.manifest = manifest;
  const table = new Element('table'), active = new Element('div');
  renderLive(table, active, state);
  assert.notEqual(table.children[1]?.children[0]?.children.length, 1, 'today\'s table (or empty) without a feed');
  state.feed = rows;
  renderLive(table, active, state);
  const [head, body] = table.children;
  assert.equal(head.tagName, 'thead');
  assert.equal(body.tagName, 'tbody');
  assert.deepEqual(body.children.map(tr => tr.children[0].textContent), ['third', 'first']);
  const recent = active.children[1].children.map(li => li.textContent);
  assert.deepEqual(recent, [`${KIND_ICON.document} One (2 changes)`, `${KIND_ICON.person} #2 (1 change)`,
    `${KIND_ICON.person} v (1 change)`]);
});

test('only a retryable failure keeps the sentence poll alive', () => {
  assert.equal(pollRetries({ status: 503 }), true);
  assert.equal(pollRetries(new TypeError('network')), true);
  assert.equal(pollRetries({ status: 404 }), false);
  assert.equal(pollRetries({ status: 400 }), false);
});
