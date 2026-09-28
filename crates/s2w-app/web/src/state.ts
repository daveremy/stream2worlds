import type { Message, Node, Link, WorldView } from './api';
export type EvidenceRow = { offset: number; kind: string; entityIds: number[]; summary: string; message: Message; ts?: string };
export class ViewState {
  nodes = new Map<string, Node>();
  links = new Map<string, Link>();
  evidence: EvidenceRow[] = [];
  lastAppliedOffset = 0;
  offset = 0;
  params: URLSearchParams;
  constructor(params: URLSearchParams) { this.params = params; }
  snapshot(view: WorldView): void {
    this.offset = view.offset;
    this.nodes = new Map(view.nodes.map(node => [node.id, node]));
    this.links = new Map(view.links.map(link => [JSON.stringify([link.source, link.target, link.kind]), link]));
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
