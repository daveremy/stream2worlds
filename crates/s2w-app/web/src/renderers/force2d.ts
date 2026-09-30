import ForceGraph from 'force-graph';
import type { NodeObject } from 'force-graph';
import type { Link, Node } from '../api';
import { LINE_HEIGHT, collideForce, fitView, isTypeView, labelBox, labelLines, maxRadius, maxTypeCount, nodeRadius } from '../nodesize';
import { labelFor, linkColor, typeColor } from '../profile';
import type { GraphRenderer } from '../renderer';
import type { ViewState } from '../state';
type GraphNode = Node & NodeObject;
// Canvas draws (backgroundColor, label fillStyle) live outside CSS — custom properties never
// reach them on their own (round-1 review finding). These are the pre-presentation defaults,
// restored whenever `setColors` is called with no override for a given channel.
const DEFAULT_GROUND = '#101c2b';
const DEFAULT_INK = '#e3edf6';
export class Force2D implements GraphRenderer {
  private graph?: ForceGraph<GraphNode, Link>;
  private resize?: ResizeObserver;
  private degreeMap = new Map<string, number>();
  private keyByType = new Map<string, string | undefined>();
  private labelById = new Map<string, string>();
  private groundColor = DEFAULT_GROUND;
  private inkColor = DEFAULT_INK;
  private typeMax = 1;
  private maxR = maxRadius(0, 0);
  // Wrapped type labels, rebuilt on update: the collide force reads them every tick.
  private typeLines = new Map<string, string[]>();
  // Set when a type view arrives with no node in common with the last one: fit on its first
  // tick (so it never paints unfitted) and again when the layout settles.
  private fitOnTick = false;
  private fitOnStop = false;
  private radius(node: GraphNode): number {
    return nodeRadius(node, this.degreeMap, this.typeMax, this.maxR);
  }
  private lines(node: GraphNode): string[] {
    return this.typeLines.get(node.id) ?? [this.label(node)];
  }
  private label(node: GraphNode): string {
    return this.labelById.get(node.id) ?? labelFor(node, this.keyByType);
  }
  /**
   * Applies presentation-supplied colours to the canvas. Safe to call before `mount` (stores the
   * values for the initial draw) or after (round-2 review finding: presentation fetches async and
   * can resolve after `mount`, so this must update a live instance, not just a mount-time value).
   */
  setColors(colors: { ground?: string; ink?: string }): void {
    this.groundColor = colors.ground || DEFAULT_GROUND;
    this.inkColor = colors.ink || DEFAULT_INK;
    this.graph?.backgroundColor(this.groundColor);
  }
  mount(element: HTMLElement, state: ViewState): void {
    this.graph = new ForceGraph<GraphNode, Link>(element).backgroundColor(this.groundColor)
      .nodeColor((node: GraphNode) => typeColor(node.entity_type))
      .nodeRelSize(1).nodeVal((node: GraphNode) => this.radius(node) ** 2)
      .nodeCanvasObjectMode(() => 'after')
      .nodeCanvasObject((node: GraphNode, context: CanvasRenderingContext2D, globalScale: number) => {
        // Type views are small and each type node is the point of the view: always labelled.
        if ((node.kind !== 'type' && globalScale < 0.7) || node.x === undefined || node.y === undefined) return;
        const fontSize = 11 / globalScale;
        context.font = `${fontSize}px system-ui, sans-serif`;
        context.textAlign = 'center'; context.textBaseline = 'top'; context.fillStyle = this.inkColor;
        const top = node.y + this.radius(node) + 2 / globalScale;
        if (node.kind !== 'type') { context.fillText(this.label(node), node.x, top); return; }
        this.lines(node).forEach((line, index) =>
          context.fillText(line, node.x!, top + (index * LINE_HEIGHT) / globalScale));
      })
      .onEngineTick(() => { if (this.fitOnTick) { this.fitOnTick = false; this.fit(); } })
      .onEngineStop(() => { if (this.fitOnStop) { this.fitOnStop = false; this.fit(); } })
      .linkColor(link => linkColor(link.kind)).linkDirectionalArrowLength(4)
      .nodeLabel((node: GraphNode) => {
        // Tooltip libraries accept HTML strings: return a text-only element for stream data.
        const label = document.createElement('span');
        label.textContent = node.kind === 'type' ? `${node.entity_type} (${node.count})` :
          `${this.label(node)} · ${node.entity_type}${node.kind === 'hub' ? ` · hub (${node.in_degree})` : ''}`;
        return label;
      });
    this.maxR = maxRadius(element.clientWidth, element.clientHeight);
    this.resize = new ResizeObserver(() => {
      this.maxR = maxRadius(element.clientWidth, element.clientHeight);
      this.graph?.width(element.clientWidth).height(element.clientHeight);
    });
    this.resize.observe(element);
    this.update(state);
  }
  update(state: ViewState): void {
    // d3 mutates nodes/links; never hand it the authoritative state objects.
    // force-graph/d3-force-3d does not match incoming nodes to existing ones by id — any node
    // object without x/y/vx/vy gets a fresh initial position and reheats the whole simulation.
    // Since every coalesced refetch (main.ts scheduleRefresh) hands this a brand-new snapshot,
    // carry the live position fields over by id so the graph doesn't re-layout from scratch on
    // every refresh (round-1 review finding, blocking).
    const stateNodes = [...state.nodes.values()];
    this.degreeMap = state.degreeMap;
    this.keyByType = state.keyByType;
    this.labelById = state.labels;
    this.typeMax = maxTypeCount(stateNodes);
    const previous: Map<string, Partial<GraphNode>> = new Map(
      (this.graph?.graphData().nodes ?? []).map(node => [node.id, node]));
    const nodes = structuredClone(stateNodes).map((node) => {
      const prior = previous.get(node.id);
      if (!prior) return node;
      const { x, y, vx, vy, fx, fy } = prior as Record<string, number | undefined>;
      return Object.assign(node, { x, y, vx, vy, fx, fy });
    });
    // The type view: no overlap, and a new one is laid out before first paint and fitted. A
    // refresh of the same view keeps its settled layout and the viewer's zoom. Entity views keep
    // force-graph's defaults (charge -30, no collide, no warmup).
    const typeView = isTypeView(stateNodes);
    this.typeLines = new Map(stateNodes.flatMap(node =>
      node.kind === 'type' ? [[node.id, labelLines(labelFor(node, this.keyByType))] as const] : []));
    const fresh = typeView && !nodes.some(node => previous.has(node.id));
    if (fresh) this.fitOnTick = this.fitOnStop = true;
    this.graph?.d3Force('collide', typeView ? collideForce<GraphNode>(node => labelBox(this.radius(node), this.lines(node))) : null)
      .warmupTicks(fresh ? 100 : 0);
    this.graph?.d3Force('charge')?.strength(typeView ? -400 : -30);
    this.graph?.graphData({ nodes, links: structuredClone([...state.links.values()]) });
  }
  /** Fits a new small view, labels included, to the canvas; never magnifies past 1 so the radius bound holds. */
  private fit(): void {
    if (!this.graph) return;
    const placed = this.graph.graphData().nodes.flatMap(node => node.x === undefined || node.y === undefined ? [] :
      [{ x: node.x, y: node.y, radius: this.radius(node), lines: this.lines(node) }]);
    const camera = fitView(placed, this.graph.width(), this.graph.height());
    if (camera) this.graph.centerAt(camera.x, camera.y).zoom(camera.k);
  }
  destroy(): void { this.resize?.disconnect(); this.graph?._destructor(); this.graph = undefined; }
}
