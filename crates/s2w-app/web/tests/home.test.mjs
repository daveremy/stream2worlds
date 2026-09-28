import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildCard, displayTitle, renderWorlds } from '../src/home.ts';

class Element {
  constructor(tagName) { this.tagName = tagName; this.children = []; this.textContent = ''; this.attrs = {};
    this.styles = {}; this.className = ''; this.style = { setProperty: (k, v) => { this.styles[k] = v; } }; }
  append(...c) { this.children.push(...c); }
  replaceChildren(...c) { this.children = [...c]; }
  setAttribute(k, v) { this.attrs[k] = v; }
}
globalThis.document = { createElement: t => new Element(t) };

test('displayTitle prefers title, then name, then world id', () => {
  assert.equal(displayTitle({ world: 'w', name: 'n', title: 't' }), 't');
  assert.equal(displayTitle({ world: 'w', name: 'n', title: null }), 'n');
  assert.equal(displayTitle({ world: 'w', name: '' }), 'w');
});

test('buildCard merges presentation over summary and keeps both accents', () => {
  const card = buildCard({ world: 'w', name: 'n', head: 3, tagline: 'sum' },
    { description: 'd', palette_dark: { accent: '#111' }, palette_light: { accent: '#eee' } });
  assert.deepEqual(card, { world: 'w', head: 3, title: 'n', tagline: 'sum', description: 'd',
    accentLight: '#eee', accentDark: '#111' });
});

test('renderWorlds shows an empty state', () => {
  const c = new Element('div'); renderWorlds(c, []);
  assert.equal(c.children[0].className, 'empty');
});

test('renderWorlds builds an escaped-by-textContent link card', () => {
  const c = new Element('div');
  renderWorlds(c, [{ world: 'a b', head: 1, title: '<b>x</b>', tagline: 'tg', accentDark: '#111' }]);
  const card = c.children[0];
  assert.equal(card.attrs.href, '/w/a%20b/');
  assert.equal(card.children[0].textContent, '<b>x</b>');
  assert.equal(card.children[1].className, 'tagline');
  assert.equal(card.children.at(-1).textContent, '1 event');
  assert.equal(card.styles['--card-accent-dark'], '#111');
});
