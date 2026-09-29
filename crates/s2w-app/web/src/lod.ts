import type { WorldView } from './api';

/// The page never asks the server for an unbounded entity graph (#262). An entity view with no
/// focus lists every entity in the world: 310,669 nodes and 370 MB on the demo world, which hung
/// the browser. The type view is small by construction and carries each type's `count`, so the
/// page asks for it first and uses it as the size probe.

/// Above this many entities an entity view without a focus is not requested: the page shows the
/// type view instead and says why.
export const ENTITY_VIEW_LIMIT = 5000;

/// The entities a type view stands for: a `type` node's `count`, plus 1 for each hub, which the
/// type view emits as itself (query/view.rs). Pure.
export function entityCount(view: WorldView): number {
  return view.nodes.reduce((total, node) => total + (node.kind === 'type' ? node.count : 1), 0);
}

/// True when a `/world` request cannot return the whole entity graph: the type view, or a focus
/// entity with its hops (the server allows at most 5 and stops each walk at a hub). Pure.
export function bounded(request: URLSearchParams): boolean {
  return request.get('lod') === 'type' || request.has('focus');
}

/// What `loadWorld` fetched: the view to show, the parameters that produced it (the page refreshes
/// with these), and a status note when the page shows less than it was asked for.
export type Loaded = { view: WorldView; request: URLSearchParams; note?: string };

/// Fetches the view the page asked for without ever sending an unbounded request:
/// - a focus bounds the request, so it goes as asked;
/// - otherwise the type view goes first; the entity view (the page's default) follows only when
///   the probe counts at most `ENTITY_VIEW_LIMIT` entities.
/// `fetchView` is `api.snapshot` on the page and a recording fake in tests.
export async function loadWorld(params: URLSearchParams,
  fetchView: (request: URLSearchParams) => Promise<WorldView>): Promise<Loaded> {
  const lod = params.get('lod') ?? 'entity';
  // A focus bounds the request; a level the page does not know (`cluster`) goes as asked, and
  // the server answers it with its own error.
  if (params.has('focus') || (lod !== 'entity' && lod !== 'type')) {
    return { view: await fetchView(params), request: params };
  }
  const probe = new URLSearchParams(params); probe.set('lod', 'type');
  const view = await fetchView(probe);
  if (lod === 'type') return { view, request: probe };
  let count = entityCount(view);
  if (count <= ENTITY_VIEW_LIMIT) {
    const request = new URLSearchParams(params); request.set('lod', 'entity');
    const entities = await fetchView(request);
    // A live world can grow past the limit between the probe and this fetch.
    if (!outgrown(request, entities)) return { view: entities, request };
    count = entities.nodes.length;
  }
  return { view, request: probe, note: `${count.toLocaleString('en-US')} entities is too many to draw at ` +
    'once. Showing types; set a Focus entity to see its neighbourhood.' };
}

/// True when an entity view without a focus came back larger than the limit: a small live world
/// grew past it. The page then reloads, and `loadWorld`'s probe picks the type view. Pure.
export function outgrown(request: URLSearchParams, view: WorldView): boolean {
  return !bounded(request) && view.nodes.length > ENTITY_VIEW_LIMIT;
}
