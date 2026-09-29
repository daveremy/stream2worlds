// Generic proposal ledger panel: who proposed what, who decided it, and how each (class, actor)
// is graded. Domain-free: classes, ids and bases are opaque strings from the log. Every value
// reaches the DOM through `textContent`, never `innerHTML`.
//
// Type-only import (see presentation.ts for why this module takes no runtime import from ./api).
import type { Actor, Proposals, ProposalGrade, Tally } from './api';

/// `n/d`, always. An empty denominator is "0/0": no evidence is never shown as a percentage or
/// as a perfect score.
export function formatFraction(fraction: readonly [number, number] | null | undefined): string {
  if (!fraction) return '0/0';
  const [num, den] = fraction;
  return den > 0 ? `${num}/${den}` : '0/0';
}

export function formatTally(tally: Tally | null | undefined): string {
  return formatFraction(tally?.fraction);
}

export function formatActor(actor: Actor | string): string {
  if (typeof actor === 'string') return actor;
  return actor.kind === 'human' ? `human:${actor.id}` : `agent:${actor.model}@${actor.version}`;
}

export const EMPTY_MESSAGE = 'No proposals yet.';

/// True when the panel should show its empty state instead of tables.
export function isEmpty(data: Proposals | null | undefined): boolean {
  return !data || (data.proposals?.length ?? 0) === 0;
}

export const GRADE_COLUMNS = ['Class', 'Actor', 'Proposed', 'Ungraded', 'Human', 'Evidence',
  'Agent opinion (not accuracy)', 'Policy routing (not accuracy)', 'Policy applied', 'Policy applied, ungraded'] as const;

export function gradeRow(grade: ProposalGrade): string[] {
  return [grade.class, formatActor(grade.actor), String(grade.proposed), String(grade.ungraded),
    formatTally(grade.human), formatTally(grade.evidence), formatTally(grade.agent),
    `${grade.policy_accepted} accepted · ${grade.policy_rejected} rejected`,
    formatTally(grade.policy_applied), String(grade.policy_applied_ungraded)];
}

function table(label: string, columns: readonly string[], rows: string[][]): HTMLElement {
  const wrap = document.createElement('div'); wrap.className = 'table-scroll';
  const element = document.createElement('table'); element.setAttribute('aria-label', label);
  const head = document.createElement('thead'); const header = head.insertRow();
  for (const name of columns) { const cell = document.createElement('th'); cell.textContent = name; header.append(cell); }
  const body = document.createElement('tbody');
  for (const values of rows) { const row = body.insertRow(); for (const value of values) row.insertCell().textContent = value; }
  element.replaceChildren(head, body); wrap.append(element);
  return wrap;
}

function heading(text: string): HTMLElement {
  const h = document.createElement('h3'); h.textContent = text; return h;
}

export function renderProposals(element: HTMLElement, data: Proposals | null | undefined): void {
  if (isEmpty(data)) {
    const empty = document.createElement('p'); empty.className = 'empty'; empty.textContent = EMPTY_MESSAGE;
    element.replaceChildren(empty);
    return;
  }
  const { proposals, decisions = [], grades = [] } = data!;
  const byProposal = new Map<string, Proposals['decisions']>();
  for (const decision of decisions) {
    const list = byProposal.get(decision.proposal_id) ?? [];
    list.push(decision); byProposal.set(decision.proposal_id, list);
  }
  const proposalRows = [...proposals].sort((a, b) => b.seq - a.seq).map(proposal => {
    const decided = (byProposal.get(proposal.id) ?? [])
      .map(d => `${d.decider}: ${d.outcome}${d.basis ? ` (${d.basis})` : ''}`).join('; ');
    return [String(proposal.seq), proposal.id, proposal.class, formatActor(proposal.actor),
      String(proposal.snapshot_offset), decided || 'undecided'];
  });
  element.replaceChildren(
    heading('Grades by class and actor'),
    table('Proposal grades', GRADE_COLUMNS, grades.map(gradeRow)),
    heading('Proposals and decisions'),
    table('Proposals', ['Seq', 'Id', 'Class', 'Actor', 'Snapshot offset', 'Decisions (decider: outcome, basis)'], proposalRows),
  );
}
