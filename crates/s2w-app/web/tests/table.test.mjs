import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ViewState } from '../src/state.ts';
import { renderTable } from '../src/table.ts';

class Element {
  constructor(tagName) { this.tagName = tagName; this.children = []; this.textContent = ''; }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = [...children]; }
  insertRow(index = this.children.length) {
    const row = new Element('tr'); this.children.splice(index, 0, row); return row;
  }
  insertCell() { const cell = new Element('td'); this.children.push(cell); return cell; }
  get rows() { return this.children; }
  deleteRow(index) { this.children.splice(index, 1); }
}

globalThis.document = { createElement: tagName => new Element(tagName) };

function entity(entity, key, value) {
  return { kind: 'entity', id: `entity:${entity}`, entity, entity_type: 'item', keys: [`key-${entity}`],
    attrs: { [key]: { Str: value } }, members: [], hub_refs: [] };
}

test('table prepends one row incrementally and rebuilds existing rows after a snapshot', () => {
  const state = new ViewState(new URLSearchParams());
  state.snapshot({ offset: 1, nodes: [entity(1, 'name', 'Shared'), entity(2, 'name', 'Shared')], links: [] });
  state.apply({ offset: 1, type: 'link', source: 1, target: 2, kind: 'related', weight: 1 });
  const table = new Element('table');
  renderTable(table, state);

  const originalBody = table.children[1];
  const originalRow = originalBody.children[0];
  assert.equal(originalRow.children[2].textContent, 'Shared #1, Shared #2');
  assert.match(originalRow.children[3].textContent, /Shared #1/);

  state.apply({ offset: 2, type: 'entity', entity: 1, resolved: 1, minted: false });
  renderTable(table, state);
  assert.equal(table.children[1], originalBody);
  assert.equal(originalBody.children[1], originalRow);

  state.snapshot({ offset: 2, nodes: [entity(1, 'title', 'First'), entity(2, 'title', 'Second')], links: [] });
  renderTable(table, state);
  assert.notEqual(table.children[1], originalBody);
  assert.notEqual(table.children[1].children[1], originalRow);
  assert.equal(table.children[1].children[1].children[2].textContent, 'First, Second');
  assert.match(table.children[1].children[1].children[3].textContent, /"source":"First"/);
});

test('table updates incrementally at the 500-row cap by evicting the oldest row, not rebuilding', () => {
  // Codex round-2 finding: once state.evidence hits its 500-row cap, ViewState.apply() evicts
  // the oldest row on every subsequent push, so evidence.length stays constant at 500 forever.
  // The table must keep updating one row at a time in that steady state, not fall back to a
  // full rebuild on every message.
  const state = new ViewState(new URLSearchParams());
  state.snapshot({ offset: 0, nodes: [entity(1, 'name', 'Solo')], links: [] });
  for (let offset = 1; offset <= 500; offset += 1) {
    state.apply({ offset, type: 'entity', entity: 1, resolved: 1, minted: false });
  }
  assert.equal(state.evidence.length, 500);

  const table = new Element('table');
  renderTable(table, state);
  const body = table.children[1];
  assert.equal(body.children.length, 500);
  const survivingRow = body.children[0]; // most recent (offset 500) — must survive the next push
  const evictedRow = body.children[499]; // oldest currently rendered (offset 1) — must be dropped

  state.apply({ offset: 501, type: 'entity', entity: 1, resolved: 1, minted: false });
  assert.equal(state.evidence.length, 500); // still capped — the length-based signal alone is flat
  renderTable(table, state);

  assert.equal(table.children[1], body); // same tbody reused — no full rebuild
  assert.equal(body.children.length, 500);
  assert.equal(body.children[1], survivingRow); // prior newest row shifted down by one, reused
  assert.ok(!body.children.includes(evictedRow)); // oldest row is gone
  assert.equal(body.children[0].children[0].textContent, '501'); // new newest row is at the top
});
