import { ApiError, evidence, eventsUrl, kinds, presentation as fetchPresentation, snapshot,
  sources as fetchSources, streamStatus, worlds } from './api';
import type { Message } from './api';
import { ViewState, unroutedStatus } from './state';
import { Force2D } from './renderers/force2d';
import { renderTable } from './table';
import { activeNow, linkColor, typeColor } from './profile';
import { applyPresentation } from './presentation';
import { buildUrl, parseWorldFromPath, worldPathFor } from './url';
const status = document.querySelector<HTMLElement>('#status')!;
const graph = document.querySelector<HTMLElement>('#graph')!;
const table = document.querySelector<HTMLTableElement>('#evidence')!;
const form = document.querySelector<HTMLFormElement>('#controls')!;
const position = document.querySelector<HTMLElement>('#position')!;
const legend = document.querySelector<HTMLElement>('#legend')!;
const active = document.querySelector<HTMLElement>('#active')!;
let dispose = () => {};
let activeState: ViewState;
const keys = ['world', 'at', 'branch', 'lod', 'focus', 'hops'];
function describe(error: unknown): string {
  return error instanceof ApiError && error.code === 'offset_beyond_head' ? 'No data at this offset' :
    error instanceof Error ? error.message : String(error);
}
// The path (`/w/<world>/`) takes precedence over the legacy `?world=` query param — the server
// redirects the query form to the path form (step 4), and every client-side URL writer below
// keeps `world` out of the query from here on (`buildUrl` strips it unconditionally).
function currentParams(): URLSearchParams {
  const params = new URLSearchParams(location.search);
  const pathWorld = parseWorldFromPath(location.pathname);
  if (pathWorld) params.set('world', pathWorld);
  return params;
}
// Every writer of the visible URL bar goes through this: the world (if known) always lives in
// the path, never the query (round-1 review finding).
function visibleUrl(params: URLSearchParams): string {
  const world = params.get('world');
  return buildUrl(world ? worldPathFor(world) : location.pathname, params);
}
async function start(): Promise<void> {
  dispose(); // Abort outstanding fetches, close the stream and cancel every timer first.
  const controller = new AbortController();
  const { signal } = controller;
  const params = currentParams();
  const state = new ViewState(params); activeState = state;
  const renderer = new Force2D();
  let source: EventSource | undefined;
  let retry: ReturnType<typeof setTimeout> | undefined;
  let refresh: ReturnType<typeof setTimeout> | undefined;
  let delay = 1000, lastFetch = 0, fetching = false, dirty = false;
  dispose = () => { controller.abort(); source?.close(); clearTimeout(retry); clearTimeout(refresh); renderer.destroy(); };
  status.textContent = 'Connecting'; position.textContent = ''; table.replaceChildren();
  for (const key of keys) (form.elements.namedItem(key) as HTMLInputElement).value =
    params.get(key) ?? ({ branch: 'actual', lod: 'entity', hops: '1' }[key] ?? '');
  function paint(): void {
    renderTable(table, state);
    renderLegend(legend, state);
    renderActive(active, state);
    position.textContent = `${params.has('at') ? 'Pinned' : 'Live'} · graph at ${state.offset} · evidence through ${state.lastAppliedOffset}`;
  }
  function scheduleRefresh(): void {
    dirty = true;
    if (refresh !== undefined || fetching || signal.aborted) return;
    refresh = setTimeout(async () => {
      refresh = undefined; dirty = false; fetching = true; lastFetch = Date.now();
      try {
        const view = await snapshot(params, signal);
        if (!signal.aborted) { state.snapshot(view); renderer.update(state); paint(); }
      } catch (error) {
        if (!signal.aborted) {
          status.textContent = describe(error);
          // Only retry on something that can plausibly resolve itself (503/network); a
          // non-503 ApiError (400/404/...) will fail identically forever, so stop looping
          // instead of refetching once a second with no backoff (round-1 review finding).
          dirty = !(error instanceof ApiError && error.status !== 503);
        }
      }
      finally { fetching = false; if (dirty && !signal.aborted) scheduleRefresh(); }
    }, Math.max(0, 1000 - (Date.now() - lastFetch)));
  }
  function reconnect(): void {
    if (signal.aborted) return;
    status.textContent = 'Reconnecting';
    retry = setTimeout(open, delay); delay = Math.min(delay * 2, 30_000);
  }
  function open(): void {
    if (signal.aborted) return;
    source?.close();
    const current = new EventSource(eventsUrl(params, state.lastAppliedOffset)); source = current;
    for (const kind of kinds) current.addEventListener(kind, event => {
      if (signal.aborted || source !== current) return;
      try {
        const message = JSON.parse((event as MessageEvent<string>).data) as Message;
        if (!state.apply(message)) return;
        delay = 1000; status.textContent = ''; paint();
        if (message.type !== 'noop') scheduleRefresh();
      } catch (error) { current.close(); status.textContent = describe(error); }
    });
    current.onerror = async () => {
      if (signal.aborted || source !== current) return;
      // CONNECTING means the browser wants to retry; close it and own retry timing instead.
      const readyState = current.readyState;
      current.close(); source = undefined;
      status.textContent = readyState === EventSource.CLOSED ? 'Disconnected' : 'Reconnecting';
      try { await streamStatus(params, state.lastAppliedOffset, AbortSignal.any([signal, AbortSignal.timeout(10_000)])); }
      catch (error) {
        if (signal.aborted) return;
        if (error instanceof ApiError && error.status === 403) {
          status.textContent = `Connection rejected: ${error.message}`; return;
        }
      }
      reconnect(); // Includes 503: a connection slot may become available later.
    };
  }
  async function initialize(): Promise<void> {
    try {
      if (!params.get('world')) {
        // Bare `/`: self-discover via `/worlds`, then navigate to the canonical `/w/<world>/`
        // URL — a real navigation (`location.replace`), not `history.replaceState`, since this
        // is the one case where the pathname itself must change from `/` to `/w/<world>/`.
        const list = await worlds(signal);
        if (signal.aborted) return;
        params.set('world', list.worlds[0]?.world ?? 'default');
        location.replace(visibleUrl(params));
        return;
      }
      // Best-effort: presentation absence/failure must never block the graph itself (same
      // posture as the sources fetch below).
      try {
        const p = await fetchPresentation(params.get('world')!, signal);
        if (!signal.aborted) applyPresentation(p, renderer);
      } catch { /* non-essential */ }
      if (signal.aborted) return;
      const view = await snapshot(params, signal); lastFetch = Date.now();
      const seed = await evidence(params, view.offset, signal);
      if (signal.aborted) return;
      state.snapshot(view); seed.forEach(message => state.apply(message));
      state.lastAppliedOffset = view.offset;
      // Empty live view only: learn whether the log has unrouted traffic so the page can name
      // that state instead of reading as broken (#143). Best-effort — a failed fetch here
      // must not block the graph itself; it just leaves the idle copy in place.
      if (view.nodes.length === 0 && !params.has('at')) {
        try { state.sources = await fetchSources(params, signal); } catch { /* non-essential */ }
        if (signal.aborted) return;
      }
      renderer.mount(graph, state); paint();
      status.textContent = view.nodes.length ? '' : params.has('at') ? 'No data at this offset' :
        (unroutedStatus(state.sources) ?? 'Waiting for events');
      if (!params.has('at')) open();
    } catch (error) {
      if (signal.aborted) return;
      if (error instanceof ApiError && error.status !== 503) { status.textContent = describe(error); return; }
      status.textContent = `Reconnecting: ${describe(error)}`;
      retry = setTimeout(() => void initialize(), delay); delay = Math.min(delay * 2, 30_000);
    }
  }
  await initialize();
}
form.addEventListener('submit', event => {
  event.preventDefault(); const params = new URLSearchParams();
  for (const key of keys) {
    const value = (form.elements.namedItem(key) as HTMLInputElement).value.trim();
    if (value) params.set(key, value);
  }
  history.replaceState(null, '', visibleUrl(params)); void start();
});
document.querySelector('#pin')!.addEventListener('click', () => {
  if (!activeState) return;
  const params = currentParams();
  params.set('at', String(activeState.offset)); history.replaceState(null, '', visibleUrl(params)); void start();
});
document.querySelector('#live')!.addEventListener('click', () => {
  const params = currentParams(); params.delete('at');
  history.replaceState(null, '', visibleUrl(params)); void start();
});
window.addEventListener('popstate', () => void start());
window.addEventListener('pagehide', () => dispose());
void start();

function renderLegend(element: HTMLElement, state: ViewState): void {
  const entityList = document.createElement('ul');
  for (const entityType of new Set([...state.nodes.values()].map(node => node.entity_type))) {
    const key = state.keyByType.get(entityType);
    entityList.append(legendItem(typeColor(entityType), `${entityType} · ${key === undefined ? 'keys' : `labeled by ${key}`}`));
  }
  const linkList = document.createElement('ul');
  for (const kind of new Set([...state.links.values()].map(link => link.kind))) linkList.append(legendItem(linkColor(kind), kind));
  const entityHeading = document.createElement('h3'); entityHeading.textContent = 'Entity types';
  const linkHeading = document.createElement('h3'); linkHeading.textContent = 'Link kinds';
  element.replaceChildren(entityHeading, entityList, linkHeading, linkList);
}

function legendItem(color: string, text: string): HTMLLIElement {
  const item = document.createElement('li');
  const swatch = document.createElement('span'); swatch.className = 'swatch'; swatch.style.backgroundColor = color;
  item.append(swatch, document.createTextNode(text));
  return item;
}

function renderActive(element: HTMLElement, state: ViewState): void {
  const recent = activeNow(state.evidence, state.nodesById, state.keyByType, state.labels);
  const recentHeading = document.createElement('h3'); recentHeading.textContent = 'Active now';
  const hubHeading = document.createElement('h3'); hubHeading.textContent = 'Hubs';
  const recentList = metricList(recent.map(item => `${item.label} (${item.count})`));
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
