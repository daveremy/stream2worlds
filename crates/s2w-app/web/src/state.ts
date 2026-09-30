import type { Message, Node, Link, WorldView, SourceInfo } from './api';
// @ts-expect-error tsconfig's Bundler resolution forbids the extension; Node's native
// TypeScript stripping (state.test.mjs importing this file directly) requires it.
import { degreeById, labelMap, nodeById, pickLabelKeys, topHubs } from './profile.ts';
import type { LabelKeyMap } from './profile';
// @ts-expect-error see above
import { manifestLabel, typeNodeLabel } from './manifest.ts';
import type { Manifest, SentenceRow } from './manifest';
export type EvidenceRow = { offset: number; kind: string; entityIds: number[]; summary: string; message: Message; ts?: string };

/// Names the empty-graph state for issue #143: distinguishes "nothing has arrived" from
/// "events are arriving but no System 1 engine claims them" so the page never reads as broken
/// when it is only unconfigured. Pure so it can be unit-tested without a DOM or a fetch.
export function unroutedStatus(sources: SourceInfo[]): string | undefined {
  // Most-active unrouted source with traffic wins ties; a genuinely idle or fully-routed set
  // returns undefined so the caller falls through to its existing idle/pinned copy.
  const candidate = sources
    .filter(s => s.unrouted > 0 && s.consumed > 0)
    .sort((a, b) => b.consumed - a.consumed)[0];
  return candidate === undefined ? undefined :
    `${candidate.consumed} events logged from ${candidate.source}, no engine routed yet (#71)`;
}
/// Names a live rebuild in progress (#184): `serve` is refolding the log under a newly accepted
/// mapping, so a partial or empty graph is expected, not broken. Pure.
export function rebuildingStatus(sources: SourceInfo[]): string | undefined {
  const source = sources.find(s => s.rebuilding !== undefined);
  return source?.rebuilding === undefined ? undefined :
    `Rebuilding world under mapping ${source.rebuilding.identity}: ${source.consumed} events so far`;
}
/// Milliseconds to wait before the `restarts`-th consecutive `stale_epoch` restart (0-based):
/// the first is immediate, then 1 s doubling to 30 s, so a server that keeps replacing its
/// history (a rebuild superseded again and again) is not hammered with snapshot reads. Pure.
export function staleEpochDelay(restarts: number): number {
  return restarts <= 0 ? 0 : Math.min(1000 * 2 ** (restarts - 1), 30_000);
}
/// The stable error code of an `ApiError` (`code`) or of the SSE stream's final `event: error`
/// frame (`data` holds `{"error": ...}`); undefined for anything else. Pure.
function errorCode(error: unknown): unknown {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as { code?: unknown }).code;
  if (code !== undefined) return code;
  const data = (error as { data?: unknown }).data;
  if (typeof data !== 'string') return undefined;
  try { return (JSON.parse(data) as { error?: unknown } | null)?.error; }
  catch { return undefined; }
}
/// True for a `stale_epoch` answer: an `ApiError` (HTTP 410) or the SSE stream's final
/// `event: error` frame. The served history was replaced, so every offset the page holds names
/// another world: the caller rebuilds from a fresh snapshot instead of reconnecting (#184).
export function isStaleEpoch(error: unknown): boolean {
  return errorCode(error) === 'stale_epoch';
}
/// True for an `offset_before_base` answer, in the same two shapes. On a live stream it means
/// the page fell further behind than the events the server keeps (decision 0026): reconnecting
/// from the same offset would fail forever, so the caller restarts from a fresh snapshot.
export function isBeforeBase(error: unknown): boolean {
  return errorCode(error) === 'offset_before_base';
}
export class ViewState {
  nodes = new Map<string, Node>();
  links = new Map<string, Link>();
  keyByType: LabelKeyMap = new Map();
  labels = new Map<string, string>();
  nodesById = new Map<number, Node>();
  degreeMap = new Map<string, number>();
  hubs: { label: string; degree: number }[] = [];
  evidence: EvidenceRow[] = [];
  // Populated once per `initialize()` when the graph is empty (main.ts): lets the table show
  // raw unrouted events instead of an empty body when no claim has been judged yet (#143).
  sources: SourceInfo[] = [];
  lastAppliedOffset = 0;
  offset = 0;
  // The epoch the page's offsets belong to; set by the first snapshot (main.ts).
  epoch = '';
  params: URLSearchParams;
  // The effective dashboard manifest, once /dashboard answered with one (s2w#289): names,
  // nouns and icons. `feed` holds the last /sentences rows, live pages only.
  manifest: Manifest | undefined;
  feed: SentenceRow[] | undefined;
  private viewNodes: Node[] = [];
  private viewLinks: Link[] = [];
  constructor(params: URLSearchParams) { this.params = params; }
  snapshot(view: WorldView): void {
    this.offset = view.offset;
    this.nodes = new Map(view.nodes.map(node => [node.id, node]));
    this.links = new Map(view.links.map(link => [JSON.stringify([link.source, link.target, link.kind]), link]));
    this.viewNodes = view.nodes; this.viewLinks = view.links;
    this.nodesById = nodeById(view.nodes);
    this.degreeMap = degreeById(view.nodes, view.links);
    this.relabel();
  }
  /// Recomputes every label from the last snapshot: the manifest's label for a type with a
  /// row, today's heuristic otherwise. Called again when the manifest arrives.
  relabel(): void {
    this.keyByType = pickLabelKeys(this.viewNodes);
    const manifest = this.manifest;
    this.labels = labelMap(this.viewNodes, this.keyByType, manifest === undefined ? undefined :
      node => node.kind === 'type' ? typeNodeLabel(node, manifest) : manifestLabel(node, manifest));
    this.hubs = topHubs(this.viewNodes, this.viewLinks, this.keyByType, 5, this.labels);
  }
  apply(message: Message): boolean {
    if (message.offset <= this.lastAppliedOffset) return false;
    this.lastAppliedOffset = message.offset;
    const ids = message.type === 'entity' ? [message.entity, message.resolved] :
      message.type === 'link' ? [message.source, message.target] :
      message.type === 'hub_ref' ? [message.source, message.hub] :
      message.type === 'noop' ? [] : [message.survivor, message.absorbed];
    this.evidence.push({ offset: message.offset, kind: message.type, entityIds: [...new Set(ids)],
      summary: JSON.stringify(message), message });
    if (this.evidence.length > 500) this.evidence.splice(0, this.evidence.length - 500);
    return true;
  }
}
