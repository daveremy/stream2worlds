// @ts-expect-error tsconfig's Bundler resolution forbids the extension; Node's native
// TypeScript stripping (tests/*.test.mjs importing this file directly) requires it.
import { SseParser } from './sse.ts';
// These tagged unions mirror query/{view,delta}.rs; graph deltas are not snapshots.
type Entity = { id: string; entity: number; entity_type: string; keys: string[];
  attrs: Record<string, { Str: string } | { Int: number } | { Bool: boolean }>;
  members: number[]; hub_refs: { kind: string; hub: string }[] };
export type Node = (Entity & { kind: 'entity' }) | (Entity & { kind: 'hub';
  in_degree: number; by_kind: Record<string, number>; last_seen_offset: number }) |
  { kind: 'type'; id: string; entity_type: string; count: number };
export type Link = { source: string; target: string; kind: string; weight: number };
// `epoch`: the served history (16 hex); pinning it makes a replaced history a 410 `stale_epoch`.
export type WorldView = { offset: number; epoch: string; branch: string; fold_version: number;
  hub_in_degree_cap: number; lod: 'entity' | 'type'; focus: number | null; nodes: Node[]; links: Link[] };
export type Delta = { type: 'entity'; entity: number; resolved: number; minted: boolean } |
  { type: 'link'; source: number; target: number; kind: string; weight: number } |
  { type: 'hub_ref'; source: number; hub: number; kind: string; tripped: boolean; in_degree: number } |
  { type: 'merge' | 'split'; survivor: number; absorbed: number } | { type: 'noop'; event: string };
export type Message = Delta & { offset: number };
export const kinds = ['entity', 'link', 'hub_ref', 'merge', 'split', 'noop'] as const;
// Mirrors query/http.rs's SourceInfo: what the bridge did with one member source.
export type RawEventInfo = { offset: number; received_at: number; payload: unknown };
// `rebuilding`: present while `serve` rebuilds the world under a newly accepted mapping (#184).
export type SourceInfo = {
  source: string; consumed: number; unrouted: number; recent_unrouted: RawEventInfo[];
  rebuilding?: { identity: string; since_position: number };
};
// Explicit fields, not parameter properties: Node's type stripping (tests/*.test.mjs) rejects those.
export class ApiError extends Error {
  status: number;
  code: string;
  constructor(status: number, code: string, message: string) { super(message); this.status = status; this.code = code; }
}
export function endpoint(params: URLSearchParams, route: string): URL {
  const url = new URL(`/worlds/${encodeURIComponent(params.get('world') || 'default')}/${route}`, location.origin);
  url.search = params.toString();
  url.searchParams.delete('world');
  return url;
}
async function checked(url: URL | string, signal: AbortSignal, init: RequestInit = {}): Promise<Response> {
  const response = await fetch(url, { ...init, signal });
  if (!response.ok && response.status !== 304) {
    const body = await response.json();
    throw new ApiError(response.status, body.error, body.message);
  }
  return response;
}
// The last `/world` entity tag and the URL it answered (#216). A refresh sends it back as
// If-None-Match, so a view that has not changed costs a 304 instead of the whole graph.
let lastWorld: { href: string; etag: string } | undefined;
async function fetchWorld(params: URLSearchParams, signal: AbortSignal, conditional: boolean): Promise<WorldView | null> {
  const url = endpoint(params, 'world');
  const headers: Record<string, string> = {};
  if (conditional && lastWorld?.href === url.href) headers['If-None-Match'] = lastWorld.etag;
  // `no-store`: the page keeps the view itself; a large body in the HTTP cache helps no one.
  const response = await checked(url, signal, { headers, cache: 'no-store' });
  if (response.status === 304) return null;
  // Keep the tag only once the body parsed: a body cut short (the server ends a stalled stream
  // with an error) must not leave a tag for a view this page never showed.
  lastWorld = undefined;
  const view: WorldView = await response.json();
  const etag = response.headers.get('ETag');
  lastWorld = etag ? { href: url.href, etag } : undefined;
  return view;
}
export async function snapshot(params: URLSearchParams, signal: AbortSignal): Promise<WorldView> {
  return (await fetchWorld(params, signal, false))!;
}
// `null`: the server answered 304, so the view from the last fetch of this URL is current.
export async function refreshSnapshot(params: URLSearchParams, signal: AbortSignal): Promise<WorldView | null> {
  return fetchWorld(params, signal, true);
}
export type WorldSummary = { world: string; name: string; head: number; title?: string | null; tagline?: string | null };
export async function worlds(signal: AbortSignal): Promise<{ worlds: WorldSummary[] }> {
  return (await checked('/worlds', signal)).json();
}
// Mirrors s2w-log's `Palette`/`Typefaces`/`WorldPresentation`: the load-path type, tolerant of
// missing fields (every field defaults to absent, never errors on a partial/older row).
export type Palette = { ground: string; ink: string; accent: string; success: string; warning: string; danger: string };
export type Typefaces = { display: string; body: string; mono: string };
export type WorldPresentation = {
  title?: string | null; tagline?: string | null; description?: string | null;
  palette_light?: Palette | null; palette_dark?: Palette | null; typefaces?: Typefaces | null;
  stylesheet?: string | null;
};
export async function presentation(world: string, signal: AbortSignal): Promise<WorldPresentation> {
  const url = new URL(`/worlds/${encodeURIComponent(world)}/presentation`, location.origin);
  return (await checked(url, signal)).json();
}
// The sources view is independent of the pinned offset, so `at` is dropped.
export async function sources(params: URLSearchParams, signal: AbortSignal): Promise<SourceInfo[]> {
  const url = endpoint(params, 'sources');
  url.searchParams.delete('at');
  return (await checked(url, signal)).json();
}
export function eventsUrl(params: URLSearchParams, from: number, at?: number, epoch?: string): URL {
  const url = endpoint(params, 'events');
  url.searchParams.set('from', String(from));
  if (at === undefined) url.searchParams.delete('at');
  else url.searchParams.set('at', String(at));
  if (epoch) url.searchParams.set('epoch', epoch);
  return url;
}
// A finite SSE response's messages plus the history (`S2W-Epoch`) and head (`S2W-Head`) the
// server resolved it under; every `/events` response carries both (s2w#294).
export type Evidence = { messages: Message[]; head: number; epoch: string };
const EVIDENCE_DEPTH = 500;
/// Called as a finite `/events` body arrives: once with the resolved head and epoch when the
/// headers land, then with each network chunk's completed messages (s2w#295).
export type EvidenceStream = { onHead?: (head: number, epoch: string) => void; onRows?: (messages: Message[]) => void };
async function readEvidence(response: Response, stream: EvidenceStream = {}): Promise<Evidence> {
  const rawHead = response.headers.get('S2W-Head'), epoch = response.headers.get('S2W-Epoch');
  // A proxy that strips them must fail loudly, never read as an empty world at offset 0.
  if (rawHead === null || epoch === null) throw new Error('/events response without S2W-Head/S2W-Epoch');
  if (!/^\d+$/.test(rawHead)) throw new Error(`/events response with a non-numeric S2W-Head: ${rawHead}`);
  const head = Number(rawHead);
  stream.onHead?.(head, epoch);
  // Parse as the body arrives, so the page can paint the first rows before the whole body
  // (68 KB for 500 rows) has closed.
  const parser = new SseParser(), messages: Message[] = [];
  const take = (rows: Message[]) => { if (rows.length) { messages.push(...rows); stream.onRows?.(rows); } };
  if (response.body) {
    const reader = response.body.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      take(parser.push(value));
    }
  }
  take(parser.end());
  return { messages, head, epoch };
}
// The live page's evidence seed: the last 500 events through the server's head, in one request
// that needs no `/time` or `/world` first. Clamped to the events the server keeps (decision
// 0026), so it never answers `offset_before_base`.
export async function evidenceTail(params: URLSearchParams, signal: AbortSignal,
  stream: EvidenceStream = {}): Promise<Evidence> {
  const url = endpoint(params, 'events');
  for (const name of ['from', 'at', 'epoch']) url.searchParams.delete(name);
  url.searchParams.set('last', String(EVIDENCE_DEPTH));
  return readEvidence(await checked(url, signal), stream);
}
// A pinned page's evidence: the 500 events through `at`. The server keeps only recent events
// (decision 0026): when those reach past them (just after a restart from a snapshot), the page
// starts with no evidence rather than failing; that 410 carries no headers, so `head` and
// `epoch` are the request's own.
export async function evidence(params: URLSearchParams, at: number, epoch: string | undefined,
  signal: AbortSignal): Promise<Evidence> {
  let response: Response;
  try { response = await checked(eventsUrl(params, Math.max(0, at - EVIDENCE_DEPTH), at, epoch), signal); }
  catch (error) {
    if (error instanceof ApiError && error.code === 'offset_before_base') return { messages: [], head: at, epoch: epoch ?? '' };
    throw error;
  }
  return readEvidence(response);
}
// EventSource hides HTTP status. A finite, empty replay probes the same guarded route.
export async function streamStatus(params: URLSearchParams, from: number, epoch: string, signal: AbortSignal): Promise<void> {
  const response = await checked(eventsUrl(params, from, from, epoch), signal);
  await response.body?.cancel();
}
// Mirrors GET /worlds/{world}/proposals: the generic proposal ledger and its grades.
// snapshot_offset is an event-log position, not a fold offset: no epoch, unchanged by a rebuild (s2w#201).
export type Actor = { kind: 'human'; id: string } | { kind: 'agent'; model: string; version: string };
export type Proposal = { seq: number; id: string; class: string; actor: Actor; snapshot_offset: number;
  payload_hash: string; proposed_at_ms: number };
export type Decision = { seq: number; proposal_id: string; decider: 'policy' | 'human' | 'evidence' | 'agent';
  outcome: 'accept' | 'reject'; basis: string; decided_at_ms: number };
export type Tally = { accepted: number; rejected: number; fraction: [number, number] };
export type ProposalGrade = { class: string; actor: Actor; proposed: number; ungraded: number;
  policy_accepted: number; policy_rejected: number; human: Tally; evidence: Tally; agent: Tally;
  policy_applied: Tally; policy_applied_ungraded: number };
export type Proposals = { proposals: Proposal[]; decisions: Decision[]; grades: ProposalGrade[] };
// The ledger is independent of the pinned offset, so `at` is dropped (same as `sources`).
export async function proposals(params: URLSearchParams, signal: AbortSignal): Promise<Proposals> {
  const url = endpoint(params, 'proposals');
  url.searchParams.delete('at');
  return (await checked(url, signal)).json();
}
