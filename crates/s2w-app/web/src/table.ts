import type { ViewState } from './state';
export function renderTable(table: HTMLTableElement, state: ViewState): void {
  const timestamp = state.evidence.some(row => row.ts !== undefined);
  const head = document.createElement('thead');
  const header = head.insertRow();
  for (const name of ['Offset', 'Kind', 'Entities', 'Evidence', ...(timestamp ? ['Time'] : [])]) {
    const cell = document.createElement('th'); cell.textContent = name; header.append(cell);
  }
  const body = document.createElement('tbody');
  for (const evidence of [...state.evidence].reverse()) {
    const row = body.insertRow();
    for (const value of [String(evidence.offset), evidence.kind, evidence.entityIds.join(', '),
      evidence.summary, ...(timestamp ? [evidence.ts ?? ''] : [])]) row.insertCell().textContent = value;
  }
  table.replaceChildren(head, body);
}
