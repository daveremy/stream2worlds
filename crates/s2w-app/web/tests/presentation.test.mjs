import { test } from 'node:test';
import assert from 'node:assert/strict';

// presentation.ts touches `document`/`matchMedia`/`getComputedStyle` at import time, so this
// shims a minimal DOM before importing it — same pattern table.test.mjs uses for `table.ts`.
class FakeElement {
  constructor() { this.hidden = false; this.textContent = ''; this.dataset = {}; }
}
const store = new Map();
const root = new FakeElement();
root.style = {
  setProperty: (k, v) => store.set(k, v), removeProperty: k => store.delete(k), get: k => store.get(k),
  get colorScheme() { return store.get('color-scheme'); },
  set colorScheme(v) { store.set('color-scheme', v); },
};
let mediaMatches = false;
const mediaListeners = [];
globalThis.matchMedia = () => ({
  get matches() { return mediaMatches; },
  addEventListener: (_type, listener) => mediaListeners.push(listener),
});
const elements = { '#world-title': new FakeElement(), '#world-tagline': new FakeElement(),
  '#world-description': new FakeElement() };
elements['#world-tagline'].textContent = 'default tagline';
globalThis.document = {
  documentElement: root,
  title: 'default title',
  querySelector: selector => elements[selector] ?? null,
};
globalThis.getComputedStyle = element => ({
  getPropertyValue: name => String(element.style.get(name) ?? ''),
});

const { applyPresentation } = await import('../src/presentation.ts');

test('applyPresentation sets every palette token as a CSS custom property', () => {
  const palette = { ground: '#111', ink: '#222', accent: '#333', success: '#444', warning: '#555', danger: '#666' };
  applyPresentation({ palette_dark: palette });
  for (const [token, value] of Object.entries(palette)) assert.equal(root.style.get(`--s2w-${token}`), value);
});

test('applyPresentation falls back to un-set custom properties when no palette exists', () => {
  applyPresentation({ palette_dark: { ground: '#111', ink: '#222', accent: '#3', success: '#4', warning: '#5', danger: '#6' } });
  applyPresentation({});
  assert.equal(root.style.get('--s2w-ground'), undefined);
  assert.equal(root.style.get('--s2w-ink'), undefined);
});

test('applyPresentation uses whichever palette exists when only one of light/dark is set', () => {
  mediaMatches = true; // system prefers light
  applyPresentation({ palette_dark: { ground: '#0a0', ink: '#0a1', accent: '#0a2', success: '#0a3', warning: '#0a4', danger: '#0a5' } });
  assert.equal(root.style.get('--s2w-ground'), '#0a0');
  mediaMatches = false;
});

test('applyPresentation sets colorScheme light only when a light-resolved palette applies, dark otherwise', () => {
  mediaMatches = true;
  applyPresentation({ palette_light: { ground: '#1', ink: '#2', accent: '#3', success: '#4', warning: '#5', danger: '#6' } });
  assert.equal(root.style.colorScheme, 'light');
  applyPresentation({});
  assert.equal(root.style.colorScheme, 'dark');
  mediaMatches = false;
});

test('applyPresentation falls back to the default title/tagline when absent', () => {
  applyPresentation({ title: 'My World', tagline: 'A tagline' });
  assert.equal(elements['#world-title'].textContent, 'My World');
  assert.equal(elements['#world-tagline'].textContent, 'A tagline');
  applyPresentation({});
  assert.equal(elements['#world-tagline'].textContent, 'default tagline');
});

test('applyPresentation hides the description element when absent, shows it as plain text when present', () => {
  applyPresentation({ description: 'line one\n\nline two' });
  assert.equal(elements['#world-description'].hidden, false);
  assert.equal(elements['#world-description'].textContent, 'line one\n\nline two');
  applyPresentation({});
  assert.equal(elements['#world-description'].hidden, true);
});
