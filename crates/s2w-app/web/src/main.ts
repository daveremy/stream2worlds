import { ApiError, evidence, evidenceTail, eventsUrl, kinds, presentation as fetchPresentation, refreshSnapshot, snapshot,
  sources as fetchSources, streamStatus, proposals as fetchProposals, dashboard as fetchDashboard,
  sentences as fetchSentences } from './api';
import type { Message } from './api';
import { ViewState, isBeforeBase, isStaleEpoch, rebuildingStatus, staleEpochDelay, unroutedStatus } from './state';
import { Force2D } from './renderers/force2d';
import { renderLive } from './live';
import { fromDashboard, nounFor, pollRetries, withIcon } from './manifest';
import type { Manifest } from './manifest';
import { renderProposals } from './proposals';
import { linkColor, typeColor } from './profile';
import { RefreshGuard, bootstrap, frameThrottle, liveRow } from './bootstrap';
import { applyPresentation } from './presentation';
import { buildUrl, parseWorldFromPath, worldPathFor } from './url';
import { loadWorld, outgrown, servedLod, submittedLod } from './lod';
import type { Loaded } from './lod';
const status = document.querySelector<HTMLElement>('#status')!;
const graph = document.querySelector<HTMLElement>('#graph')!;
const table = document.querySelector<HTMLTableElement>('#evidence')!;
const form = document.querySelector<HTMLFormElement>('#controls')!;
const lodSelect = form.elements.namedItem('lod') as HTMLSelectElement;
const position = document.querySelector<HTMLElement>('#position')!;
const legend = document.querySelector<HTMLElement>('#legend')!;
const active = document.querySelector<HTMLElement>('#active')!;
const proposalsPanel = document.querySelector<HTMLElement>('#proposals')!;
const eventsHeading = document.querySelector<HTMLElement>('#events-heading')!;
// Today's heading and label, restored on every start(): a feed replaces them only once it shows.
const EVENTS_HEADING = eventsHeading.innerHTML;
const EVENTS_LABEL = table.getAttribute('aria-label') ?? '';
let dispose = () => {};
let activeState: ViewState;
// Consecutive `stale_epoch` restarts: reset once a restart has lasted STALE_WINDOW_MS.
const STALE_WINDOW_MS = 30_000;
let staleRestarts = 0, lastStaleRestart = 0;
let staleTimer: ReturnType<typeof setTimeout> | undefined;
// The served history was replaced: start over from a fresh snapshot, backing off when it keeps
// happening (#184, deferred from 2b-i).
function restartStale(reason = 'The world was rebuilt; reloading'): void {
  dispose(); clearTimeout(staleTimer);
  const now = Date.now();
  staleRestarts = now - lastStaleRestart < STALE_WINDOW_MS ? staleRestarts + 1 : 0;
  lastStaleRestart = now;
  status.textContent = reason;
  staleTimer = setTimeout(() => void start(), staleEpochDelay(staleRestarts));
}
const BEHIND = 'Fell behind the live stream; reloading';
const keys = ['world', 'at', 'branch', 'lod', 'focus', 'hops'];
// The proposal ledger changes on System 2's cadence, not per event: poll it on its own slow timer.
const PROPOSALS_POLL_MS = 5000;
// The sentence feed (s2w#289): the newest SENTENCES_LAST events, refetched on this timer.
const SENTENCES_POLL_MS = 5000;
const SENTENCES_LAST = 200;
// At most one full `/world` refetch per this many ms while deltas keep arriving. Each one is
// projected under the server's read lock, which blocks the fold (#216), so a busy stream must
// not trigger one per second; deltas still apply locally between refetches.
const WORLD_REFRESH_MS = 5000;
// The Detail level the page asked for and the one the selector shows (#269); they differ when a
// large world fell back to the type view.
let detail = { asked: 'entity', shown: 'entity' };
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
  clearTimeout(staleTimer);
  const controller = new AbortController();
  const { signal } = controller;
  const params = currentParams();
  const pinned = params.has('at');
  const state = new ViewState(params); activeState = state;
  const renderer = new Force2D();
  let source: EventSource | undefined;
  let retry: ReturnType<typeof setTimeout> | undefined; // the next load attempt
  let reconnectTimer: ReturnType<typeof setTimeout> | undefined; // the next live-stream open
  // Bumped by every open() and closeStream(): a stream's late error handler sees it moved on.
  let streamGeneration = 0;
  let refresh: ReturnType<typeof setTimeout> | undefined;
  let proposalsTimer: ReturnType<typeof setTimeout> | undefined;
  let frame: number | undefined;
  let proposalsGeneration = 0;
  let sentencesTimer: ReturnType<typeof setTimeout> | undefined;
  let sentencesGeneration = 0;
  let delay = 1000, lastFetch = 0, fetching = false, dirty = false, mounted = false, drawn = false;
  // The `/world` parameters actually served, which may differ from the page's (#262): every
  // refresh reuses them, so a refresh can never widen the view to the whole entity graph. Until
  // the first world view is applied they are unknown, so `guard` holds every refresh (#295).
  let request = params;
  const guard = new RefreshGuard(scheduleRefresh);
  // Why the page shows less than it asked for, if it does; kept while live deltas arrive.
  let note: string | undefined;
  dispose = () => {
    controller.abort(); source?.close(); clearTimeout(retry); clearTimeout(reconnectTimer); clearTimeout(refresh); clearTimeout(proposalsTimer);
    clearTimeout(sentencesTimer);
    if (frame !== undefined) cancelAnimationFrame(frame);
    renderer.destroy();
  };
  status.textContent = 'Connecting'; position.textContent = ''; table.replaceChildren(); proposalsPanel.replaceChildren();
  eventsHeading.innerHTML = EVENTS_HEADING; table.setAttribute('aria-label', EVENTS_LABEL);
  for (const key of keys) (form.elements.namedItem(key) as HTMLInputElement).value =
    params.get(key) ?? ({ branch: 'actual', lod: 'entity', hops: '1' }[key] ?? '');
  detail = { asked: lodSelect.value, shown: lodSelect.value };
  function placeText(): string {
    return `${pinned ? 'Pinned' : 'Live'} · ` + (mounted ? `graph at ${state.offset} · evidence through ${state.lastAppliedOffset}` :
      `evidence through ${state.lastAppliedOffset} · graph loading`);
  }
  // The tail's chunks and bursts of live rows: at most one table render per animation frame.
  const paintEvidence = frameThrottle(() => {
    frame = undefined;
    if (signal.aborted) return;
    renderLive(table, active, state); position.textContent = placeText();
  }, run => { frame = requestAnimationFrame(run); });
  function paint(): void {
    renderLive(table, active, state);
    renderLegend(legend, state);
    position.textContent = placeText();
  }
  // Best-effort, like proposals: no manifest (or no route) leaves today's view as it is. With
  // one, names, nouns and icons apply (pinned pages too); the feed needs a live page.
  async function loadManifest(world: string, load: AbortSignal): Promise<void> {
    let manifest: Manifest | undefined;
    try { manifest = fromDashboard(await fetchDashboard(world, load)); } catch { return; }
    if (load.aborted || manifest === undefined) return;
    state.manifest = manifest; state.relabel();
    if (drawn) renderer.update(state);
    paint();
    if (!pinned && manifest.sentences) void pollSentences(world, manifest);
  }
  // The sentence feed: the next poll is scheduled only after this one settles; a stale
  // response is dropped; an answer that cannot change (non-503) stops the poll.
  async function pollSentences(world: string, manifest: Manifest): Promise<void> {
    clearTimeout(sentencesTimer);
    const generation = ++sentencesGeneration;
    let retry = true;
    try {
      const rows = await fetchSentences(world, SENTENCES_LAST, signal);
      if (!signal.aborted && generation === sentencesGeneration) {
        if (state.feed === undefined) {
          const small = document.createElement('small'); small.textContent = manifest.domain;
          eventsHeading.replaceChildren('Live changes ', small);
          table.setAttribute('aria-label', 'Live changes');
        }
        state.feed = rows; renderLive(table, active, state);
      }
    } catch (error) { retry = pollRetries(error); }
    if (retry && !signal.aborted && generation === sentencesGeneration) {
      sentencesTimer = setTimeout(() => void pollSentences(world, manifest), SENTENCES_POLL_MS);
    }
  }
  // Best-effort: a missing or failing proposals route must never block the graph. The next poll
  // is scheduled only after this one settles, and the generation check drops any stale response.
  async function pollProposals(): Promise<void> {
    clearTimeout(proposalsTimer);
    const generation = ++proposalsGeneration;
    try {
      const data = await fetchProposals(params, signal);
      if (!signal.aborted && generation === proposalsGeneration) renderProposals(proposalsPanel, data);
    } catch { /* non-essential */ }
    if (!signal.aborted && generation === proposalsGeneration) proposalsTimer = setTimeout(() => void pollProposals(), PROPOSALS_POLL_MS);
  }
  // Only `guard` calls this before the first world view; after it, `guard.request()` passes through.
  function scheduleRefresh(): void {
    dirty = true;
    if (refresh !== undefined || fetching || signal.aborted) return;
    refresh = setTimeout(async () => {
      refresh = undefined; dirty = false; fetching = true; lastFetch = Date.now();
      try {
        // `null`: 304, the view already shown is current (#216).
        const view = await refreshSnapshot(request, signal);
        if (signal.aborted) return;
        // A small live world grew past the entity limit: reload, and the probe picks types.
        if (view && outgrown(request, view)) { restartStale('The world outgrew the entity view; showing types'); return; }
        // Another history is served: this page's offsets name another world, so rebuild.
        if (view && view.epoch !== state.epoch) { restartStale(); return; }
        // A rebuild in progress: keep its count current, and keep refreshing on a quiet log,
        // until the server says it is done (it clears the field on its next idle poll).
        if (rebuildingStatus(state.sources) !== undefined) {
          try { state.sources = await fetchSources(params, signal); } catch { /* non-essential */ }
          if (signal.aborted) return;
          const rebuilding = rebuildingStatus(state.sources);
          if (rebuilding !== undefined) dirty = true;
          status.textContent = rebuilding ?? note ?? ((view ? view.nodes.length : state.nodes.size) ? '' : 'Waiting for events');
        }
        if (view) { state.snapshot(view); renderer.update(state); paint(); }
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
    }, Math.max(0, WORLD_REFRESH_MS - (Date.now() - lastFetch)));
  }
  function reconnect(): void {
    if (signal.aborted) return;
    status.textContent = 'Reconnecting';
    reconnectTimer = setTimeout(open, delay); delay = Math.min(delay * 2, 30_000);
  }
  function closeStream(): void {
    streamGeneration++; source?.close(); source = undefined; clearTimeout(reconnectTimer);
  }
  function open(): void {
    if (signal.aborted) return;
    closeStream();
    const generation = ++streamGeneration;
    const current = new EventSource(eventsUrl(params, state.lastAppliedOffset, undefined, state.epoch)); source = current;
    for (const kind of kinds) current.addEventListener(kind, event => {
      if (signal.aborted || source !== current) return;
      try {
        const message = JSON.parse((event as MessageEvent<string>).data) as Message;
        if (!liveRow(state, guard, message, paintEvidence)) return;
        delay = 1000;
        status.textContent = mounted ? rebuildingStatus(state.sources) ?? note ?? '' : 'Loading world…';
      } catch (error) { current.close(); status.textContent = describe(error); }
    });
    current.onerror = async event => {
      if (signal.aborted || source !== current) return;
      // The stream's final `event: error` frame names a replaced history, or an offset the
      // server no longer keeps (decision 0026): rebuild, skip the probe.
      if (isStaleEpoch(event)) { current.close(); restartStale(); return; }
      if (isBeforeBase(event)) { current.close(); restartStale(BEHIND); return; }
      // CONNECTING means the browser wants to retry; close it and own retry timing instead.
      const readyState = current.readyState;
      current.close(); source = undefined;
      status.textContent = readyState === EventSource.CLOSED ? 'Disconnected' : 'Reconnecting';
      try { await streamStatus(params, state.lastAppliedOffset, state.epoch, AbortSignal.any([signal, AbortSignal.timeout(10_000)])); }
      catch (error) {
        if (signal.aborted || generation !== streamGeneration) return;
        if (isStaleEpoch(error)) { restartStale(); return; }
        if (isBeforeBase(error)) { restartStale(BEHIND); return; }
        if (error instanceof ApiError && error.status === 403) {
          status.textContent = `Connection rejected: ${error.message}`; return;
        }
      }
      // A failed load attempt closed the stream meanwhile: its retry reopens it.
      if (signal.aborted || generation !== streamGeneration) return;
      reconnect(); // Includes 503: a connection slot may become available later.
    };
  }
  function draw(): void {
    if (drawn) renderer.update(state); else renderer.mount(graph, state);
    drawn = true;
  }
  // Draws the type summary of a large world while its full type view loads (#303). The status
  // stays "Loading world…" and `mounted` stays false until `mount` applies the full view.
  function summary(): void { draw(); paint(); }
  // Shows the first applied world view; `bootstrap` calls it, then lets held refreshes through.
  function mount(loaded: Loaded): void {
    const { view } = loaded;
    lastFetch = Date.now(); request = loaded.request; note = loaded.note;
    // Show the level actually served: `Types` when a large world fell back to the type view.
    lodSelect.value = detail.shown = servedLod(loaded);
    // A retried load (the seed failed after the world was shown), or the full view after the
    // summary, updates the drawn renderer; nodes keep their positions by id and links appear.
    draw();
    mounted = true; paint();
    status.textContent = rebuildingStatus(state.sources) ?? note ??
      (view.nodes.length ? '' : pinned ? 'No data at this offset' :
        (unroutedStatus(state.sources) ?? 'Waiting for events'));
  }
  async function initialize(): Promise<void> {
    // A world-less page load (a stray `/index.html`) has nothing to show: send it to the home page.
    const world = params.get('world');
    if (!world) { location.replace('/'); return; }
    // One load attempt: a failed one is aborted before the retry, so its late responses land nowhere.
    const attempt = new AbortController();
    const load = AbortSignal.any([signal, attempt.signal]);
    // A large world takes seconds to build: name the wait instead of an empty canvas.
    status.textContent = 'Loading world…';
    void loadManifest(world, load);
    try {
      await bootstrap({
        at: pinned ? Number(params.get('at')) : undefined, state, guard,
        aborted: () => load.aborted,
        presentation: async () => {
          const p = await fetchPresentation(world, load);
          if (!load.aborted) applyPresentation(p, renderer);
        },
        proposals: () => void pollProposals(),
        sources: () => fetchSources(params, load),
        tail: stream => evidenceTail(params, load, stream),
        evidence: at => evidence(params, at, undefined, load),
        loadWorld: onSummary => loadWorld(params, served => snapshot(served, load), onSummary),
        open, paintEvidence, summary, mount, restartStale,
      });
    } catch (error) {
      attempt.abort();
      if (signal.aborted) return;
      closeStream(); clearTimeout(retry);
      // The history was replaced between two reads: start over.
      if (isStaleEpoch(error)) { restartStale(); return; }
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
  const lod = submittedLod(lodSelect.value, detail.shown, detail.asked);
  if (lod) params.set('lod', lod); else params.delete('lod');
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
window.addEventListener('pagehide', () => { dispose(); clearTimeout(staleTimer); });
void start();

function renderLegend(element: HTMLElement, state: ViewState): void {
  const entityList = document.createElement('ul');
  for (const entityType of new Set([...state.nodes.values()].map(node => node.entity_type))) {
    const key = state.keyByType.get(entityType);
    // A manifest row with a noun reads as that noun; any other type as today.
    const text = state.manifest?.rows.get(entityType)?.noun !== undefined ?
      withIcon(state.manifest, entityType, nounFor(state.manifest, entityType)) :
      `${entityType} · ${key === undefined ? 'keys' : `labeled by ${key}`}`;
    entityList.append(legendItem(typeColor(entityType), text));
  }
  const linkList = document.createElement('ul');
  for (const kind of new Set([...state.links.values()].map(link => link.kind))) linkList.append(legendItem(linkColor(kind), kind));
  const entityHeading = document.createElement('h3'); entityHeading.textContent = 'Types';
  const linkHeading = document.createElement('h3'); linkHeading.textContent = 'Link kinds';
  element.replaceChildren(entityHeading, entityList, linkHeading, linkList);
}

function legendItem(color: string, text: string): HTMLLIElement {
  const item = document.createElement('li');
  const swatch = document.createElement('span'); swatch.className = 'swatch'; swatch.style.backgroundColor = color;
  item.append(swatch, document.createTextNode(text));
  return item;
}
