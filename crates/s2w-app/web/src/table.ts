import type { ViewState } from './state';
// @ts-expect-error tsconfig's Bundler resolution forbids the extension; Node's native
// TypeScript stripping (table.test.mjs importing this file directly) requires it.
import { labeledSummary, labelFor } from './profile.ts';

type RenderedTable = {
  state: ViewState;
  body: HTMLTableSectionElement;
  evidenceLength: number;
  lastEvidence?: ViewState['evidence'][number];
  labels: Map<string, string>;
  timestamp: boolean;
};

const renderedTables = new WeakMap<HTMLTableElement, RenderedTable>();

/// Renders each unrouted source's most recent raw events (bridge-capped, newest first) as
/// unjudged rows. Returns false (and leaves `table` untouched) when there is nothing to show,
/// so the caller falls through to the normal empty-table path.
function renderRawEvidence(table: HTMLTableElement, state: ViewState): boolean {
  const rows = state.sources
    .flatMap(source => source.recent_unrouted.map(raw => ({ ...raw, source: source.source })))
    .sort((a, b) => b.offset - a.offset);
  if (rows.length === 0) return false;
  const head = document.createElement('thead');
  const header = head.insertRow();
  for (const name of ['Offset', 'Source', 'Raw event (unjudged — no engine routed, #71)']) {
    const cell = document.createElement('th'); cell.textContent = name; header.append(cell);
  }
  const body = document.createElement('tbody');
  for (const row of rows) {
    const tr = body.insertRow();
    for (const value of [String(row.offset), row.source, JSON.stringify(row.payload)]) {
      tr.insertCell().textContent = value;
    }
  }
  table.replaceChildren(head, body);
  return true;
}

export function renderTable(table: HTMLTableElement, state: ViewState): void {
  // No claim has been judged yet: show the raw log instead of an empty body, so a viewer with
  // an unrouted source sees the stream is alive (#143). Falls through to the normal judged
  // path — and clears any stale raw-render cache — the moment evidence starts arriving.
  if (state.evidence.length === 0) {
    renderedTables.delete(table);
    if (renderRawEvidence(table, state)) return;
  }
  const timestamp = state.evidence.some(row => row.ts !== undefined);
  const rendered = renderedTables.get(table);
  const stableBase = rendered !== undefined && rendered.state === state && rendered.labels === state.labels &&
    rendered.timestamp === timestamp && rendered.evidenceLength > 0;
  const newLen = state.evidence.length;
  // Below the 500-row cap: exactly one row was pushed and nothing evicted.
  const appendedOnly = stableBase && newLen === rendered!.evidenceLength + 1 &&
    state.evidence[rendered!.evidenceLength - 1] === rendered!.lastEvidence;
  // At the 500-row cap (state.ts evicts the oldest row as soon as a new one pushes past it):
  // length is unchanged, but the item that used to be newest is now second-newest.
  const appendedAndEvictedOldest = stableBase && newLen === rendered!.evidenceLength &&
    newLen >= 2 && state.evidence[newLen - 2] === rendered!.lastEvidence;
  if (appendedOnly || appendedAndEvictedOldest) {
    insertEvidenceRow(rendered!.body, state.evidence[newLen - 1], state, timestamp, 0);
    if (appendedAndEvictedOldest) rendered!.body.deleteRow(rendered!.body.rows.length - 1);
    rendered!.evidenceLength = newLen;
    rendered!.lastEvidence = state.evidence[newLen - 1];
    return;
  }

  const head = document.createElement('thead');
  const header = head.insertRow();
  for (const name of ['Offset', 'Kind', 'Entities', 'Evidence', ...(timestamp ? ['Time'] : [])]) {
    const cell = document.createElement('th'); cell.textContent = name; header.append(cell);
  }
  const body = document.createElement('tbody');
  for (const evidence of [...state.evidence].reverse()) insertEvidenceRow(body, evidence, state, timestamp);
  table.replaceChildren(head, body);
  renderedTables.set(table, { state, body, evidenceLength: state.evidence.length,
    lastEvidence: state.evidence[state.evidence.length - 1], labels: state.labels, timestamp });
}

function insertEvidenceRow(
  body: HTMLTableSectionElement, evidence: ViewState['evidence'][number], state: ViewState,
  timestamp: boolean, index?: number,
): void {
  const row = body.insertRow(index);
  const entities = evidence.entityIds.map(id => {
    const node = state.nodesById.get(id);
    return node === undefined ? `#${id}` : state.labels.get(node.id) ?? labelFor(node, state.keyByType);
  }).join(', ');
  for (const value of [String(evidence.offset), evidence.kind, entities,
    labeledSummary(evidence.message, state.nodesById, state.keyByType, state.labels),
    ...(timestamp ? [evidence.ts ?? ''] : [])]) {
    row.insertCell().textContent = value;
  }
}
