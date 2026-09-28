import type { Message, Node, Link, WorldView } from './api';
// @ts-expect-error tsconfig's Bundler resolution forbids the extension; Node's native
// TypeScript stripping (state.test.mjs importing this file directly) requires it.
import { degreeById, labelMap, nodeById, pickLabelKeys, topHubs } from './profile.ts';
import type { LabelKeyMap } from './profile';
export type EvidenceRow = { offset: number; kind: string; entityIds: number[]; summary: string; message: Message; ts?: string };
export class ViewState {
  nodes = new Map<string, Node>();
  links = new Map<string, Link>();
  keyByType: LabelKeyMap = new Map();
  labels = new Map<string, string>();
  nodesById = new Map<number, Node>();
  degreeMap = new Map<string, number>();
  hubs: { label: string; degree: number }[] = [];
  evidence: EvidenceRow[] = [];
  lastAppliedOffset = 0;
  offset = 0;
  params: URLSearchParams;
  constructor(params: URLSearchParams) { this.params = params; }
  snapshot(view: WorldView): void {
    this.offset = view.offset;
    this.nodes = new Map(view.nodes.map(node => [node.id, node]));
    this.links = new Map(view.links.map(link => [JSON.stringify([link.source, link.target, link.kind]), link]));
    this.keyByType = pickLabelKeys(view.nodes);
    this.labels = labelMap(view.nodes, this.keyByType);
    this.nodesById = nodeById(view.nodes);
    this.degreeMap = degreeById(view.nodes, view.links);
    this.hubs = topHubs(view.nodes, view.links, this.keyByType, 5, this.labels);
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
