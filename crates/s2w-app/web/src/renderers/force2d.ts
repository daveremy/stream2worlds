import ForceGraph from 'force-graph';
import type { Node } from '../api';
import type { GraphRenderer } from '../renderer';
import type { ViewState } from '../state';
export class Force2D implements GraphRenderer {
  private graph?: ForceGraph<Node>;
  private resize?: ResizeObserver;
  mount(element: HTMLElement, state: ViewState): void {
    this.graph = new ForceGraph<Node>(element).backgroundColor('#101c2b')
      .nodeAutoColorBy('entity_type').linkColor(() => '#7890a6').linkDirectionalArrowLength(4)
      .nodeLabel((node: Node) => {
        // Tooltip libraries accept HTML strings: return a text-only element for stream data.
        const label = document.createElement('span');
        label.textContent = node.kind === 'type' ? `${node.entity_type} (${node.count})` :
          `${node.keys.join(', ')} · ${node.entity_type}${node.kind === 'hub' ? ` · hub (${node.in_degree})` : ''}`;
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
    const previous: Map<string, Partial<Node>> = new Map(
      (this.graph?.graphData().nodes ?? []).map((node) => [(node as Node).id, node as Partial<Node>]));
    const nodes = structuredClone([...state.nodes.values()]).map((node) => {
      const prior = previous.get(node.id);
      if (!prior) return node;
      const { x, y, vx, vy, fx, fy } = prior as Record<string, number | undefined>;
      return Object.assign(node, { x, y, vx, vy, fx, fy });
    });
    this.graph?.graphData({ nodes, links: structuredClone([...state.links.values()]) });
  }
  destroy(): void { this.resize?.disconnect(); this.graph?._destructor(); this.graph = undefined; }
}
