import type { Evidence, EvidenceStream, Message, SourceInfo, WorldView } from './api';
import type { Loaded } from './lod';

/// The viewer's load order (s2w#295), pure so tests drive it with recording fakes (lod.test.mjs
/// style). At t=0, in parallel: presentation, the evidence seed, proposals, sources (live only)
/// and the world. The evidence table and Active now paint from the seed's first parsed rows,
/// without waiting on `/world`; the only request that waits on the world is the graph refresh,
/// held by `RefreshGuard` until the first world view is applied.

/// The part of `ViewState` the load order touches (the real one on the page).
export interface BootState {
  epoch: string;
  lastAppliedOffset: number;
  sources: SourceInfo[];
  apply(message: Message): boolean;
  snapshot(view: WorldView): void;
}

/// Holds `/world` refreshes until the first world view is applied. Before then the served
/// request is unknown, and the page's own parameters (`lod=entity`, no focus) are the unbounded
/// request #262 forbids, so a live row only marks the view `dirty`. `ready()` sets `loaded` and,
/// if a row arrived meanwhile, lets exactly one refresh through.
export class RefreshGuard {
  loaded = false;
  dirty = false;
  private refresh: () => void;
  constructor(refresh: () => void) { this.refresh = refresh; }
  request(): void {
    if (!this.loaded) { this.dirty = true; return; }
    this.refresh();
  }
  ready(): void {
    if (this.loaded) return;
    this.loaded = true;
    if (this.dirty) { this.dirty = false; this.refresh(); }
  }
}

/// Wraps `fn` so that however often it is called, it runs at most once per `schedule` tick (an
/// animation frame on the page): the tail's chunks and a burst of live rows would otherwise
/// re-render the 500-row table several times per frame.
export function frameThrottle(fn: () => void, schedule: (run: () => void) => unknown): () => void {
  let pending = false;
  return () => {
    if (pending) return;
    pending = true;
    schedule(() => { pending = false; fn(); });
  };
}

/// One live-stream message: apply it, repaint the evidence and ask for a graph refresh, which
/// `guard` holds until the world is shown. False when its offset was already applied.
export function liveRow(state: BootState, guard: RefreshGuard, message: Message, paint: () => void): boolean {
  if (!state.apply(message)) return false;
  paint();
  if (message.type !== 'noop') guard.request();
  return true;
}

export type BootDeps = {
  /// The pinned offset (`?at=`), or undefined on a live page.
  at: number | undefined;
  state: BootState;
  guard: RefreshGuard;
  /// True once this load was superseded (a restart, a retry, page navigation).
  aborted(): boolean;
  /// Best-effort requests: a failure never blocks the table or the graph.
  presentation(): Promise<void>;
  proposals(): void;
  sources(): Promise<SourceInfo[]>;
  /// Live: the last 500 events through the server's head, streamed (`api.evidenceTail`).
  tail(stream: EvidenceStream): Promise<Evidence>;
  /// Pinned: the 500 events through `at` (`api.evidence`); never a `last` request.
  evidence(at: number): Promise<Evidence>;
  loadWorld(): Promise<Loaded>;
  /// Opens the live stream from `state.lastAppliedOffset` under `state.epoch`.
  open(): void;
  /// The evidence table, Active now and the position line (throttled by the page).
  paintEvidence(): void;
  /// Shows the applied world view: renderer, legend, status, the served request.
  mount(loaded: Loaded): void;
  restartStale(): void;
};

/// Runs one load. Resolves when both the evidence seed and the world view are shown; rejects
/// with the first essential failure (seed or world), which the page retries or reports.
export async function bootstrap(d: BootDeps): Promise<void> {
  const live = d.at === undefined;
  let stale = false;
  const gone = () => stale || d.aborted();
  // Each response names the history its offsets belong to. The first one known becomes the
  // page's; any other means the served history was replaced mid-load, so start over. '' is an
  // unknown epoch (a pinned seed older than the server keeps), never a mismatch.
  const sameHistory = (epoch: string): boolean => {
    if (gone()) return false;
    if (epoch === '' || epoch === d.state.epoch) return true;
    if (d.state.epoch === '') { d.state.epoch = epoch; return true; }
    stale = true;
    d.restartStale();
    return false;
  };
  const rows = (messages: Message[]): void => {
    if (gone()) return;
    let applied = false;
    for (const message of messages) applied = d.state.apply(message) || applied;
    if (applied) d.paintEvidence();
  };

  d.presentation().catch(() => { /* non-essential */ });
  d.proposals();
  const sources = live
    ? d.sources().then(list => { if (!gone()) d.state.sources = list; }, () => { /* non-essential */ })
    : Promise.resolve();
  const world = d.loadWorld();
  const seed = live
    ? d.tail({ onHead: (_head, epoch) => { sameHistory(epoch); }, onRows: rows }).then(tail => {
      if (!sameHistory(tail.epoch)) return;
      // The tail covered every event through the head, rows or not: stream from there. `max`,
      // because live rows can never be behind the tail, and an offset never moves backward.
      d.state.lastAppliedOffset = Math.max(d.state.lastAppliedOffset, tail.head);
      d.paintEvidence();
      d.open();
    })
    : d.evidence(d.at!).then(pinned => {
      if (!sameHistory(pinned.epoch)) return;
      rows(pinned.messages);
      d.state.lastAppliedOffset = Math.max(d.state.lastAppliedOffset, d.at!);
      d.paintEvidence();
    });
  const graph = world.then(async loaded => {
    if (!sameHistory(loaded.view.epoch)) return;
    // `snapshot` leaves `lastAppliedOffset` alone: rows already applied stay applied, and a view
    // below the tail's head never rewinds the stream.
    d.state.snapshot(loaded.view);
    await sources;
    if (gone()) return;
    d.mount(loaded);
    d.guard.ready();
  });
  await Promise.all([seed, graph]);
}
