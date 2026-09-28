import type { ViewState } from './state';
import { labeledSummary, labelFor, nodeById, pickLabelKeys } from './profile';
export function renderTable(table: HTMLTableElement, state: ViewState): void {
  const nodes = [...state.nodes.values()];
  const nodesById = nodeById(nodes);
  const keyByType = pickLabelKeys(nodes);
  const timestamp = state.evidence.some(row => row.ts !== undefined);
  const head = document.createElement('thead');
  const header = head.insertRow();
  for (const name of ['Offset', 'Kind', 'Entities', 'Evidence', ...(timestamp ? ['Time'] : [])]) {
    const cell = document.createElement('th'); cell.textContent = name; header.append(cell);
  }
  const body = document.createElement('tbody');
  for (const evidence of [...state.evidence].reverse()) {
    const row = body.insertRow();
    const entities = evidence.entityIds.map(id => {
      const node = nodesById.get(id);
      return node === undefined ? `#${id}` : labelFor(node, keyByType);
    }).join(', ');
    for (const value of [String(evidence.offset), evidence.kind, entities,
      labeledSummary(evidence.message, nodesById, keyByType), ...(timestamp ? [evidence.ts ?? ''] : [])]) {
      row.insertCell().textContent = value;
    }
  }
  table.replaceChildren(head, body);
}
