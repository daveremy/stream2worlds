// These tagged unions mirror query/{view,delta}.rs; graph deltas are not snapshots.
type Entity = { id: string; entity: number; entity_type: string; keys: string[];
  attrs: Record<string, { Str: string } | { Int: number } | { Bool: boolean }>;
  members: number[]; hub_refs: { kind: string; hub: string }[] };
export type Node = (Entity & { kind: 'entity' }) | (Entity & { kind: 'hub';
  in_degree: number; by_kind: Record<string, number>; last_seen_offset: number }) |
  { kind: 'type'; id: string; entity_type: string; count: number };
export type Link = { source: string; target: string; kind: string; weight: number };
export type WorldView = { offset: number; branch: string; fold_version: number;
  hub_in_degree_cap: number; lod: 'entity' | 'type'; focus: number | null; nodes: Node[]; links: Link[] };
export type Delta = { type: 'entity'; entity: number; resolved: number; minted: boolean } |
  { type: 'link'; source: number; target: number; kind: string; weight: number } |
  { type: 'hub_ref'; source: number; hub: number; kind: string; tripped: boolean; in_degree: number } |
  { type: 'merge' | 'split'; survivor: number; absorbed: number } | { type: 'noop'; event: string };
export type Message = Delta & { offset: number };
export const kinds = ['entity', 'link', 'hub_ref', 'merge', 'split', 'noop'] as const;
// Mirrors query/http.rs's SourceInfo: what the bridge did with one member source.
export type RawEventInfo = { offset: number; received_at: number; payload: unknown };
export type SourceInfo = { source: string; consumed: number; unrouted: number; recent_unrouted: RawEventInfo[] };
export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) { super(message); }
}
export function endpoint(params: URLSearchParams, route: string): URL {
  const url = new URL(`/worlds/${encodeURIComponent(params.get('world') || 'default')}/${route}`, location.origin);
  url.search = params.toString();
  url.searchParams.delete('world');
  return url;
}
async function checked(url: URL | string, signal: AbortSignal): Promise<Response> {
  const response = await fetch(url, { signal });
  if (!response.ok) {
    const body = await response.json();
    throw new ApiError(response.status, body.error, body.message);
  }
  return response;
}
export async function snapshot(params: URLSearchParams, signal: AbortSignal): Promise<WorldView> {
  return (await checked(endpoint(params, 'world'), signal)).json();
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
export function eventsUrl(params: URLSearchParams, from: number, at?: number): URL {
  const url = endpoint(params, 'events');
  url.searchParams.set('from', String(from));
  if (at === undefined) url.searchParams.delete('at');
  else url.searchParams.set('at', String(at));
  return url;
}
// A finite SSE response from the same route seeds pinned and live evidence alike.
export async function evidence(params: URLSearchParams, at: number, signal: AbortSignal): Promise<Message[]> {
  const response = await checked(eventsUrl(params, Math.max(0, at - 500), at), signal);
  const text = await response.text();
  return text.split(/\r?\n\r?\n/).flatMap(block => {
    const data = block.split(/\r?\n/).filter(line => line.startsWith('data:'))
      .map(line => line.slice(5).trimStart()).join('\n');
    return data ? [JSON.parse(data) as Message] : [];
  });
}
// EventSource hides HTTP status. A finite, empty replay probes the same guarded route.
export async function streamStatus(params: URLSearchParams, from: number, signal: AbortSignal): Promise<void> {
  const response = await checked(eventsUrl(params, from, from), signal);
  await response.body?.cancel();
}
