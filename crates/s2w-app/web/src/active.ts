import type { ViewState } from './state';
// @ts-expect-error tsconfig's Bundler resolution forbids the extension; Node's native
// TypeScript stripping (tests/*.test.mjs importing this file directly) requires it.
import { activeNow } from './profile.ts';

/// Active now and Hubs. Safe before the first world view (s2w#295): with no nodes yet, rows show
/// their entity ids (`#42`) and Hubs shows None. `recent` replaces the evidence-counted Active
/// now rows (the sentence feed's, s2w#289).
export function renderActive(element: HTMLElement, state: ViewState, recent?: string[]): void {
  const rows = recent ?? activeNow(state.evidence, state.nodesById, state.keyByType, state.labels)
    .map(item => `${item.label} (${item.count})`);
  const recentHeading = document.createElement('h3'); recentHeading.textContent = 'Active now';
  const hubHeading = document.createElement('h3'); hubHeading.textContent = 'Hubs';
  const recentList = metricList(rows);
  const hubList = metricList(state.hubs.map(item => `${item.label} (${item.degree})`));
  element.replaceChildren(recentHeading, recentList, hubHeading, hubList);
}

function metricList(values: string[]): HTMLUListElement {
  const list = document.createElement('ul');
  for (const value of values.length ? values : ['None']) {
    const item = document.createElement('li'); item.textContent = value; list.append(item);
  }
  return list;
}
