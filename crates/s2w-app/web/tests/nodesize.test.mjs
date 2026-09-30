import { test } from 'node:test';
import assert from 'node:assert/strict';
import { CHAR_WIDTH, LINE_HEIGHT, MIN_TYPE_RADIUS, REL_SIZE, collideForce, fitView, labelBox, labelLines, maxRadius, maxTypeCount, nodeRadius } from '../src/nodesize.ts';

// s2w#265: type nodes were sized by raw count; on the demo world (~320k entities) three types
// covered the canvas and the rest were not visible at all.
const type = count => ({ kind: 'type', id: `type:t${count}`, entity_type: `t${count}`, count });
const SPAN = [1, 10, 100, 1_000, 10_000, 100_000, 300_000].map(type);
// /worlds/default/world?lod=type on the demo, 2026-09-29.
const DEMO = [21622, 22263, 22555, 22555, 32258, 22558, 167735, 3922, 2476, 2476].map(type);
const CANVASES = [[740, 230], [1400, 800], [390, 300]];

test('type radius grows with count, sub-linearly, within [MIN_TYPE_RADIUS, maxR]', () => {
  for (const [w, h] of CANVASES) {
    const maxR = maxRadius(w, h), typeMax = maxTypeCount(SPAN);
    const radii = SPAN.map(node => nodeRadius(node, new Map(), typeMax, maxR));
    for (let i = 1; i < radii.length; i++) assert.ok(radii[i] >= radii[i - 1], `monotone at ${i}`);
    assert.ok(radii.every(r => r >= MIN_TYPE_RADIUS && r <= maxR), radii.join(','));
    assert.equal(radii.at(-1), maxR);
    // 300,000x the entities is nowhere near 300,000x the area.
    assert.ok((radii.at(-1) / radii[0]) ** 2 < 100);
  }
});

test('no node is larger than a small share of the canvas', () => {
  for (const [w, h] of CANVASES) {
    const maxR = maxRadius(w, h);
    assert.ok(2 * maxR <= 0.25 * Math.min(w, h), `${w}x${h}: diameter ${2 * maxR}`);
    const hub = { kind: 'hub', id: 'e:1', in_degree: 10_000 };
    assert.equal(nodeRadius(hub, new Map(), 1, maxR), maxR);
  }
});

test('the demo world\'s types together cover a small part of the canvas', () => {
  for (const [w, h] of CANVASES) {
    const maxR = maxRadius(w, h), typeMax = maxTypeCount(DEMO);
    const area = DEMO.reduce((sum, node) => sum + Math.PI * nodeRadius(node, new Map(), typeMax, maxR) ** 2, 0);
    assert.ok(area <= 0.25 * w * h, `${w}x${h}: ${Math.round(area)} of ${w * h}`);
  }
});

test('entity radius is force-graph\'s default until the clamp', () => {
  const degrees = new Map([['e:1', 1], ['e:2', 9], ['e:3', 10_000]]);
  const entity = id => ({ kind: 'entity', id });
  assert.equal(nodeRadius(entity('e:1'), degrees, 1, 32), REL_SIZE);
  assert.equal(nodeRadius(entity('e:2'), degrees, 1, 32), REL_SIZE * 3);
  assert.equal(nodeRadius(entity('e:missing'), degrees, 1, 32), REL_SIZE);
  assert.equal(nodeRadius(entity('e:3'), degrees, 1, 32), 32);
});

test('maxRadius has a floor, a ceiling, and a default before layout', () => {
  assert.equal(maxRadius(0, 0), 32);
  assert.equal(maxRadius(100, 100), 12);
  assert.equal(maxRadius(4000, 3000), 48);
});

test('type labels wrap at each + so the widest line is one field', () => {
  assert.deepEqual(labelLines('editor/user_id+performer/user_id (22555)'), ['editor/user_id+', 'performer/user_id (22555)']);
  assert.deepEqual(labelLines('editor/user_text (32258)'), ['editor/user_text (32258)']);
  const box = labelBox(10, ['ab', 'abcd']);
  assert.equal(box.halfW, Math.max(10, 4 * CHAR_WIDTH / 2));
  assert.equal(box.halfH, 10 + 2 * LINE_HEIGHT);
});

test('the collide force separates overlapping node-and-label boxes', () => {
  const nodes = [{ x: 0, y: 0 }, { x: 1, y: 0 }, { x: 0, y: 0 }, { x: 5, y: 3 }];
  const force = collideForce(() => ({ halfW: 30, halfH: 10 }), 0);
  force.initialize(nodes);
  for (let tick = 0; tick < 300; tick++) {
    force(1);
    for (const node of nodes) { node.x += node.vx; node.y += node.vy; node.vx *= 0.6; node.vy *= 0.6; }
  }
  for (let i = 0; i < nodes.length; i++) for (let j = i + 1; j < nodes.length; j++) {
    const apart = Math.abs(nodes[i].x - nodes[j].x) >= 59.5 || Math.abs(nodes[i].y - nodes[j].y) >= 19.5;
    assert.ok(apart, `${i}-${j}: ${JSON.stringify([nodes[i], nodes[j]])}`);
  }
});

test('fitView keeps every node and its label inside the canvas, never zooming past 1', () => {
  const name = i => (i % 2 ? `editor/field_${i}+performer/field_${i} (2${i}000)` : `redirect_page_link/wikibase_${i} (24${i})`);
  const layouts = [
    Array.from({ length: 10 }, (_, i) => ({ x: Math.cos(i) * 400, y: Math.sin(i) * 250 })),
    Array.from({ length: 10 }, (_, i) => ({ x: i * 3, y: -i * 2 })),
    [{ x: 5, y: 5 }],
  ];
  for (const [w, h] of CANVASES) for (const layout of layouts) {
    const placed = layout.map((p, i) => ({ ...p, radius: i === 0 ? maxRadius(w, h) : MIN_TYPE_RADIUS, lines: labelLines(name(i)) }));
    const { x, y, k } = fitView(placed, w, h);
    assert.ok(k > 0 && k <= 1, `k=${k}`);
    for (const node of placed) {
      const sx = (node.x - x) * k + w / 2, sy = (node.y - y) * k + h / 2, r = node.radius * k;
      const half = Math.max(r, labelBox(0, node.lines).halfW);
      const label = `${w}x${h} ${node.lines.join('')}`;
      assert.ok(sx - half >= -0.5 && sx + half <= w + 0.5, `${label}: x ${sx}±${half}`);
      assert.ok(sy - r >= -0.5 && sy + r + node.lines.length * LINE_HEIGHT + 2 <= h + 0.5, `${label}: y ${sy}`);
    }
  }
  assert.equal(fitView([], 100, 100), undefined);
  assert.equal(fitView([{ x: 0, y: 0, radius: 4, lines: ['a'] }], 0, 0), undefined);
});
