import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { EMPTY_MESSAGE, GRADE_COLUMNS, formatActor, formatFraction, formatTally, gradeRow, isEmpty,
  renderProposals } from '../src/proposals.ts';

class Element {
  constructor(tagName) { this.tagName = tagName; this.children = []; this.textContent = ''; this.className = ''; this.attrs = {}; }
  append(...c) { this.children.push(...c); }
  replaceChildren(...c) { this.children = [...c]; }
  setAttribute(k, v) { this.attrs[k] = v; }
  insertRow() { const row = new Element('tr'); this.children.push(row); return row; }
  insertCell() { const cell = new Element('td'); this.children.push(cell); return cell; }
  set innerHTML(_v) { throw new Error('innerHTML must never be used'); }
}
globalThis.document = { createElement: t => new Element(t) };

const fixture = JSON.parse(readFileSync(new URL('../fixtures/proposals.json', import.meta.url), 'utf8'));
const texts = el => [el.textContent, ...el.children.flatMap(texts)];

test('formatFraction shows n/d and an empty denominator as 0/0, never a percentage', () => {
  assert.equal(formatFraction([3, 4]), '3/4');
  assert.equal(formatFraction([0, 1]), '0/1');
  assert.equal(formatFraction([0, 0]), '0/0');
  assert.equal(formatFraction([5, 0]), '0/0');
  assert.equal(formatFraction(undefined), '0/0');
  assert.equal(formatTally(undefined), '0/0');
  assert.equal(formatTally({ accepted: 2, rejected: 0, fraction: [2, 2] }), '2/2');
});

test('formatActor names humans and agents generically', () => {
  assert.equal(formatActor({ kind: 'human', id: 'u1' }), 'human:u1');
  assert.equal(formatActor({ kind: 'agent', model: 'm', version: '2' }), 'agent:m@2');
});

test('gradeRow labels policy as routing and keeps empty denominators at 0/0', () => {
  const row = gradeRow(fixture.grades[1]);
  assert.equal(row.length, GRADE_COLUMNS.length);
  assert.deepEqual(row, ['class-b', 'agent:model-x@1', '1', '1', '0/0', '0/0', '0/0',
    '0 accepted · 0 rejected', '0/0', '0']);
  assert.ok(row.every(cell => !cell.includes('%')));
  assert.match(GRADE_COLUMNS[6], /opinion.*not accuracy/i);
  assert.match(GRADE_COLUMNS[7], /routing.*not accuracy/i);
});

test('empty state renders a message when there are no proposals', () => {
  for (const data of [undefined, null, { proposals: [], decisions: [], grades: [] }]) {
    assert.equal(isEmpty(data), true);
    const panel = new Element('div'); renderProposals(panel, data);
    assert.equal(panel.children.length, 1);
    assert.equal(panel.children[0].className, 'empty');
    assert.equal(panel.children[0].textContent, EMPTY_MESSAGE);
  }
  assert.equal(isEmpty(fixture), false);
});

test('renderProposals lists grades and each proposal with its decisions, as plain text', () => {
  const panel = new Element('div'); renderProposals(panel, fixture);
  const [, gradeWrap, , listWrap] = panel.children;
  const gradeBody = gradeWrap.children[0].children[1];
  assert.equal(gradeBody.children.length, 2);
  assert.equal(gradeBody.children[0].children[5].textContent, '0/1');
  const listBody = listWrap.children[0].children[1];
  assert.equal(listBody.children.length, 3);
  assert.equal(listBody.children[0].children[1].textContent, 'p-3'); // newest first
  assert.equal(listBody.children[0].children[5].textContent, 'undecided');
  assert.equal(listBody.children[1].children[5].textContent,
    'policy: accept (<b>rule-1</b>); evidence: reject (later-offset-20)');
  // Markup in data stays literal text.
  assert.ok(texts(panel).includes('policy: accept (<b>rule-1</b>); evidence: reject (later-offset-20)'));
});
