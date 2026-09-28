// Instance home page (stream2worlds#144 PR 2): a dashboard of the worlds this instance serves.
// Side-effect free and free of runtime imports of api.ts (Node's test loader needs extensions),
// so tests can load it; the fetching bootstrap lives in home-main.ts.
import type { WorldPresentation, WorldSummary } from './api';
// @ts-expect-error tsconfig's Bundler resolution forbids the extension; Node's native
// TypeScript stripping (home.test.mjs importing this file directly) requires it.
import { worldPathFor } from './url.ts';

export type WorldCard = {
  world: string; head: number; title: string; tagline?: string; description?: string;
  accentLight?: string; accentDark?: string;
};

export function displayTitle(summary: { world: string; name: string; title?: string | null }): string {
  return summary.title || summary.name || summary.world;
}

export function buildCard(summary: WorldSummary, pres: WorldPresentation): WorldCard {
  return {
    world: summary.world, head: summary.head, title: pres.title || displayTitle(summary),
    tagline: pres.tagline || summary.tagline || undefined, description: pres.description || undefined,
    accentLight: pres.palette_light?.accent, accentDark: pres.palette_dark?.accent,
  };
}

function el(tag: string, className: string | undefined, text: string): HTMLElement {
  const node = document.createElement(tag);
  if (className) node.className = className;
  node.textContent = text;
  return node;
}

export function renderWorlds(container: HTMLElement, cards: WorldCard[]): void {
  if (cards.length === 0) { container.replaceChildren(el('p', 'empty', 'No worlds yet.')); return; }
  container.replaceChildren(...cards.map(card => {
    const link = document.createElement('a');
    link.className = 'world-card';
    link.setAttribute('href', worldPathFor(card.world));
    if (card.accentLight) link.style.setProperty('--card-accent-light', card.accentLight);
    if (card.accentDark) link.style.setProperty('--card-accent-dark', card.accentDark);
    link.append(el('h2', undefined, card.title));
    if (card.tagline) link.append(el('p', 'tagline', card.tagline));
    if (card.description) link.append(el('p', 'description', card.description));
    link.append(el('p', 'meta', `${card.head} event${card.head === 1 ? '' : 's'}`));
    return link;
  }));
}
