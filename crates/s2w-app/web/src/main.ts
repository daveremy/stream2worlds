import { ApiError, evidence, eventsUrl, kinds, snapshot, streamStatus, worlds } from './api';
import type { Message } from './api';
import { ViewState } from './state';
import { Force2D } from './renderers/force2d';
import { renderTable } from './table';
const status = document.querySelector<HTMLElement>('#status')!;
const graph = document.querySelector<HTMLElement>('#graph')!;
const table = document.querySelector<HTMLTableElement>('#evidence')!;
const form = document.querySelector<HTMLFormElement>('#controls')!;
const position = document.querySelector<HTMLElement>('#position')!;
let dispose = () => {};
let activeState: ViewState;
const keys = ['world', 'at', 'branch', 'lod', 'focus', 'hops'];
function describe(error: unknown): string {
  return error instanceof ApiError && error.code === 'offset_beyond_head' ? 'No data at this offset' :
    error instanceof Error ? error.message : String(error);
}
async function start(): Promise<void> {
  dispose(); // Abort outstanding fetches, close the stream and cancel every timer first.
  const controller = new AbortController();
  const { signal } = controller;
  const params = new URLSearchParams(location.search);
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
      } catch (error) { if (!signal.aborted) { status.textContent = describe(error); dirty = true; } }
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
        const list = await worlds(signal);
        if (signal.aborted) return;
        params.set('world', list.worlds[0]?.world ?? 'default');
        history.replaceState(null, '', `?${params}`);
        (form.elements.namedItem('world') as HTMLInputElement).value = params.get('world')!;
      }
      const view = await snapshot(params, signal); lastFetch = Date.now();
      const seed = await evidence(params, view.offset, signal);
      if (signal.aborted) return;
      state.snapshot(view); seed.forEach(message => state.apply(message));
      state.lastAppliedOffset = view.offset;
      renderer.mount(graph, state); paint();
      status.textContent = view.nodes.length ? '' : params.has('at') ? 'No data at this offset' : 'Waiting for events';
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
  history.replaceState(null, '', `?${params}`); void start();
});
document.querySelector('#pin')!.addEventListener('click', () => {
  if (!activeState) return;
  const params = new URLSearchParams(location.search);
  params.set('at', String(activeState.offset)); history.replaceState(null, '', `?${params}`); void start();
});
document.querySelector('#live')!.addEventListener('click', () => {
  const params = new URLSearchParams(location.search); params.delete('at');
  history.replaceState(null, '', `?${params}`); void start();
});
window.addEventListener('popstate', () => void start());
window.addEventListener('pagehide', () => dispose());
void start();
