import type { ViewState } from './state';
// @ts-expect-error tsconfig's Bundler resolution forbids the extension; Node's native
// TypeScript stripping (tests/*.test.mjs importing this file directly) requires it.
import { renderTable } from './table.ts';
// @ts-expect-error see above
import { renderActive, metricList } from './active.ts';
// @ts-expect-error see above
import { changesText, feedSentences, sentenceActive, withIcon } from './manifest.ts';
import type { SentenceRow } from './manifest';

// The feed each table last showed: a repaint with the same rows keeps the DOM.
const renderedFeeds = new WeakMap<HTMLTableElement, SentenceRow[]>();

/// The live list and Active now, one renderer per mode (s2w#289): with a sentence feed, the
/// manifest's sentences and Active now counted from them; without one, today's table and
/// Active now.
export function renderLive(table: HTMLTableElement, active: HTMLElement, state: ViewState): void {
  if (state.feed === undefined) {
    renderedFeeds.delete(table);
    renderTable(table, state); renderActive(active, state);
    return;
  }
  renderFeed(table, state.feed);
  renderSentenceActive(active, state, state.feed);
}

/// One row per sentence, newest first, under an explicit `tbody`.
export function renderFeed(table: HTMLTableElement, rows: SentenceRow[]): void {
  if (renderedFeeds.get(table) === rows) return;
  const head = document.createElement('thead');
  const cell = document.createElement('th'); cell.textContent = 'Change'; head.insertRow().append(cell);
  const body = document.createElement('tbody');
  for (const sentence of feedSentences(rows)) body.insertRow().insertCell().textContent = sentence;
  table.replaceChildren(head, body);
  renderedFeeds.set(table, rows);
}

/// Active now from the feed (`<icon> <label> (<n> changes)`), then Hubs as today.
export function renderSentenceActive(element: HTMLElement, state: ViewState, rows: SentenceRow[]): void {
  const recent = sentenceActive(rows, state.manifest, entity => {
    const node = state.nodesById.get(entity);
    return node === undefined ? undefined : state.labels.get(node.id);
  });
  const recentHeading = document.createElement('h3'); recentHeading.textContent = 'Active now';
  const hubHeading = document.createElement('h3'); hubHeading.textContent = 'Hubs';
  const recentList = metricList(recent.map(item =>
    `${withIcon(state.manifest, item.type, item.label)} (${changesText(item.count)})`));
  const hubList = metricList(state.hubs.map(item => `${item.label} (${item.degree})`));
  element.replaceChildren(recentHeading, recentList, hubHeading, hubList);
}
