import type { ViewState } from './state';
// @ts-expect-error tsconfig's Bundler resolution forbids the extension; Node's native
// TypeScript stripping (tests/*.test.mjs importing this file directly) requires it.
import { renderTable } from './table.ts';
// @ts-expect-error see above
import { renderActive } from './active.ts';
// @ts-expect-error see above
import { changesText, feedSentences, sentenceActive, withIcon } from './manifest.ts';
import type { SentenceRow } from './manifest';

// The feed each table last showed, as its row positions: a repaint or a poll that returns the
// same events keeps the DOM. A position's sentence changes only when the server accepts a new
// manifest; on a quiet stream the old text stays until the next event shifts the positions.
const renderedFeeds = new WeakMap<HTMLTableElement, string>();

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
  renderActive(active, state, sentenceRecent(state, state.feed));
}

/// One row per sentence, newest first, under an explicit `tbody`.
export function renderFeed(table: HTMLTableElement, rows: SentenceRow[]): void {
  const shown = `${rows.length}:${rows[0]?.position}:${rows[rows.length - 1]?.position}`;
  if (renderedFeeds.get(table) === shown) return;
  const head = document.createElement('thead');
  const cell = document.createElement('th'); cell.textContent = 'Change'; head.insertRow().append(cell);
  const body = document.createElement('tbody');
  for (const sentence of feedSentences(rows)) body.insertRow().insertCell().textContent = sentence;
  table.replaceChildren(head, body);
  renderedFeeds.set(table, shown);
}

// Active now from the feed, per feed array: recounted only when the feed or the labels change
// (`relabel` replaces the label map whenever nodes or the manifest change), not every frame.
const activeCache = new WeakMap<SentenceRow[], { labels: ViewState['labels']; rows: string[] }>();

/// Active now from the feed: `<icon> <label> (<n> changes)`.
function sentenceRecent(state: ViewState, rows: SentenceRow[]): string[] {
  const cached = activeCache.get(rows);
  if (cached !== undefined && cached.labels === state.labels) return cached.rows;
  const recent = sentenceActive(rows, state.manifest, entity => {
    const node = state.nodesById.get(entity);
    return node === undefined ? undefined : state.labels.get(node.id);
  }).map(item => `${withIcon(state.manifest, item.type, item.label)} (${changesText(item.count)})`);
  activeCache.set(rows, { labels: state.labels, rows: recent });
  return recent;
}
