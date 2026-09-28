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
