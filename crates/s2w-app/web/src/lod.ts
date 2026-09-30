import type { WorldView } from './api';

/// The page never asks the server for an unbounded entity graph (#262). An entity view with no
/// focus lists every entity in the world: 310,669 nodes and 370 MB on the demo world, which hung
/// the browser. The type summary (`lod=type&links=none`, #296) is small by construction and carries
/// each type's `count` and every hub, so the page asks for it first and uses it as the size probe.

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
/// - otherwise the type summary (`lod=type&links=none`) goes first as the size probe. The entity
///   view (the page's default) follows only when the summary counts at most `ENTITY_VIEW_LIMIT`
///   entities; otherwise `onSummary` gets the summary to draw, and the full type view (links
///   included) follows and is what `loadWorld` returns. The summary is never returned, so the page
///   never refreshes with it (#303).
/// `fetchView` is `api.snapshot` on the page and a recording fake in tests.
export async function loadWorld(params: URLSearchParams,
  fetchView: (request: URLSearchParams) => Promise<WorldView>,
  onSummary: (summary: WorldView) => void = () => {}): Promise<Loaded> {
  const lod = params.get('lod') ?? 'entity';
  // A focus bounds the request; a level the page does not know (`cluster`) goes as asked, and
  // the server answers it with its own error.
  if (params.has('focus') || (lod !== 'entity' && lod !== 'type')) {
    return { view: await fetchView(params), request: params };
  }
  const probe = new URLSearchParams(params); probe.set('lod', 'type'); probe.set('links', 'none');
  const summary = await fetchView(probe);
  let count = entityCount(summary);
  if (lod === 'entity' && count <= ENTITY_VIEW_LIMIT) {
    const request = new URLSearchParams(params); request.set('lod', 'entity'); request.delete('links');
    const entities = await fetchView(request);
    // A live world can grow past the limit between the probe and this fetch.
    if (!outgrown(request, entities)) return { view: entities, request };
    count = entities.nodes.length;
  } else {
    onSummary(summary);
  }
  const request = new URLSearchParams(params); request.set('lod', 'type'); request.delete('links');
  const view = await fetchView(request);
  if (lod === 'type') return { view, request };
  return { view, request, note: `${count.toLocaleString('en-US')} entities is too many to draw at ` +
    'once. Showing types; set a Focus entity to see its neighbourhood.' };
}

/// True when an entity view without a focus came back larger than the limit: a small live world
/// grew past it. The page then reloads, and `loadWorld`'s probe picks the type view. Pure.
export function outgrown(request: URLSearchParams, view: WorldView): boolean {
  return !bounded(request) && view.nodes.length > ENTITY_VIEW_LIMIT;
}

/// The Detail level `loaded` actually shows (#269): `type` when a large world fell back to the type
/// view, whatever the page asked for, so the Detail selector matches what is drawn. Pure.
export function servedLod(loaded: Loaded): string {
  return loaded.request.get('lod') ?? 'entity';
}

/// The `lod` a form submit sends (#269). When the selector still shows a fallback the page set
/// (`shown` differs from the `asked` level), the user did not choose it: submit what they asked
/// for, so adding a Focus to the default page still gets that entity's neighbourhood rather than
/// a type view. A level the user picks goes as picked. Pure.
export function submittedLod(selected: string, shown: string, asked: string): string {
  return selected === shown && shown !== asked ? asked : selected;
}
