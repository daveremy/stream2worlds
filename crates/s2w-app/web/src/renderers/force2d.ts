import ForceGraph from 'force-graph';
import type { NodeObject } from 'force-graph';
import type { Link, Node } from '../api';
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
      .nodeVal((node: GraphNode) => sizeFor(node, this.degreeMap))
      .nodeCanvasObjectMode(() => 'after')
      .nodeCanvasObject((node: GraphNode, context: CanvasRenderingContext2D, globalScale: number) => {
        if (globalScale < 0.7 || node.x === undefined || node.y === undefined) return;
        const fontSize = 11 / globalScale;
        context.font = `${fontSize}px system-ui, sans-serif`;
        context.textAlign = 'center'; context.textBaseline = 'top'; context.fillStyle = this.inkColor;
        context.fillText(this.label(node), node.x, node.y + 5 / globalScale);
      })
      .linkColor(link => linkColor(link.kind)).linkDirectionalArrowLength(4)
      .nodeLabel((node: GraphNode) => {
        // Tooltip libraries accept HTML strings: return a text-only element for stream data.
        const label = document.createElement('span');
        label.textContent = node.kind === 'type' ? `${node.entity_type} (${node.count})` :
          `${this.label(node)} · ${node.entity_type}${node.kind === 'hub' ? ` · hub (${node.in_degree})` : ''}`;
        return label;
      });
    this.resize = new ResizeObserver(() => {
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
    const previous: Map<string, Partial<GraphNode>> = new Map(
      (this.graph?.graphData().nodes ?? []).map(node => [node.id, node]));
    const nodes = structuredClone(stateNodes).map((node) => {
      const prior = previous.get(node.id);
      if (!prior) return node;
      const { x, y, vx, vy, fx, fy } = prior as Record<string, number | undefined>;
      return Object.assign(node, { x, y, vx, vy, fx, fy });
    });
    this.graph?.graphData({ nodes, links: structuredClone([...state.links.values()]) });
  }
  destroy(): void { this.resize?.disconnect(); this.graph?._destructor(); this.graph = undefined; }
}

function sizeFor(node: Node, degreeMap: Map<string, number>): number {
  if (node.kind === 'hub') return Math.max(1, node.in_degree);
  if (node.kind === 'type') return Math.max(1, node.count);
  return Math.max(1, degreeMap.get(node.id) ?? 1);
}
