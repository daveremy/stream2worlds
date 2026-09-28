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
    this.graph?.graphData({ nodes: structuredClone([...state.nodes.values()]),
      links: structuredClone([...state.links.values()]) });
  }
  destroy(): void { this.resize?.disconnect(); this.graph?._destructor(); this.graph = undefined; }
}
