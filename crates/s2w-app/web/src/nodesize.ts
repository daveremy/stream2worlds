import type { Node } from './api';
// Node radius in graph units (s2w#265). Kept free of force-graph so node:test can import it.
// force2d.ts sets nodeRelSize(1) and nodeVal = radius², so the drawn radius is exactly this value.

/** force-graph's default nodeRelSize: entity and hub nodes keep radius 4·sqrt(value). */
export const REL_SIZE = 4;
/** The smallest type still reads as a node, not a speck. */
export const MIN_TYPE_RADIUS = 6;
/** Canvas share bounding the largest node. */
export const CANVAS_SHARE = 0.08;
/** A type view at most this large gets the collide force, a stronger charge and the fit. */
export const SMALL_VIEW = 200;

/** The type view (lod=type): its nodes are types (and hubs), and there are few of them. */
export function isTypeView(nodes: readonly Node[]): boolean {
  return nodes.length <= SMALL_VIEW && nodes.some(node => node.kind === 'type');
}

/** The largest radius any node may have on a canvas of this size. */
export function maxRadius(width: number, height: number): number {
  const side = Math.min(width, height);
  if (!(side > 0)) return 32;
  return Math.min(48, Math.max(12, side * CANVAS_SHARE));
}

export function maxTypeCount(nodes: Iterable<Node>): number {
  let max = 1;
  for (const node of nodes) if (node.kind === 'type') max = Math.max(max, node.count);
  return max;
}

/**
 * Type nodes: area proportional to entity count, relative to the view's largest type, so the
 * largest is exactly `maxR` and none is smaller than MIN_TYPE_RADIUS. Hub and entity nodes keep
 * force-graph's default radius, 4·sqrt(in-degree or degree), clamped to `maxR`.
 */
export function nodeRadius(node: Node, degreeMap: Map<string, number>, typeMax: number, maxR: number): number {
  if (node.kind === 'type') {
    const share = Math.max(1, node.count) / Math.max(1, typeMax);
    return Math.max(MIN_TYPE_RADIUS, Math.min(maxR, maxR * Math.sqrt(share)));
  }
  const value = node.kind === 'hub' ? node.in_degree : degreeMap.get(node.id) ?? 1;
  return Math.min(maxR, REL_SIZE * Math.sqrt(Math.max(1, value)));
}

/** Label metrics at 11px system-ui, in graph units at zoom 1 (an estimate: layout only). */
export const CHAR_WIDTH = 6.2;
export const LINE_HEIGHT = 13;

/** Type names join source fields with `+`; one field per line keeps a label narrow. */
export function labelLines(text: string): string[] {
  return text.split(/(?<=\+)/);
}

/** Half extents of a node plus its label drawn below it: what small-view collision keeps apart. */
export function labelBox(radius: number, lines: string[]): { halfW: number; halfH: number } {
  const widest = Math.max(0, ...lines.map(line => line.length)) * CHAR_WIDTH;
  return { halfW: Math.max(radius, widest / 2), halfH: radius + lines.length * LINE_HEIGHT };
}

type Body = { x?: number; y?: number; vx?: number; vy?: number };
type Box = { halfW: number; halfH: number };
/**
 * A d3-style collision force for small views: boxes (node plus label) that overlap, with
 * `padding` between them, are pushed apart along the line between their centres. O(n²), so force2d
 * installs it only on a type view (isTypeView). `box` is read every tick, so a canvas resize
 * reaches the layout.
 */
export function collideForce<N extends Body>(box: (node: N) => Box, padding = 6) {
  let nodes: N[] = [];
  const force = (alpha: number): void => {
    const boxes = nodes.map(box), strength = 0.5 * Math.max(alpha, 0.1);
    for (let i = 0; i < nodes.length; i++) {
      const a = nodes[i];
      for (let j = i + 1; j < nodes.length; j++) {
        const b = nodes[j];
        let dx = (b.x ?? 0) - (a.x ?? 0), dy = (b.y ?? 0) - (a.y ?? 0);
        if (dx === 0 && dy === 0) { dx = (j - i) * 1e-3; dy = 1e-3; }
        const overlapX = boxes[i].halfW + boxes[j].halfW + padding - Math.abs(dx);
        const overlapY = boxes[i].halfH + boxes[j].halfH + padding - Math.abs(dy);
        if (overlapX <= 0 || overlapY <= 0) continue;
        // Push along the line between centres, so a small view spreads in two dimensions.
        const push = (Math.min(overlapX, overlapY) / Math.hypot(dx, dy)) * strength;
        a.vx = (a.vx ?? 0) - dx * push; a.vy = (a.vy ?? 0) - dy * push;
        b.vx = (b.vx ?? 0) + dx * push; b.vy = (b.vy ?? 0) + dy * push;
      }
    }
  };
  force.initialize = (next: N[]): void => { nodes = next; };
  return force;
}

export type Placed = { x: number; y: number; radius: number; lines: string[] };
/**
 * Camera for a small view: the zoom (never above 1) and centre at which every node and its label
 * is inside a `width`×`height` canvas. Radii and positions scale with zoom; labels are drawn at a
 * fixed screen size, so their extents are budgeted in pixels, not graph units.
 */
export function fitView(placed: Placed[], width: number, height: number, padding = 16):
  { x: number; y: number; k: number } | undefined {
  if (placed.length === 0 || !(width > 0) || !(height > 0)) return undefined;
  const xs = placed.map(node => node.x), ys = placed.map(node => node.y);
  const minX = Math.min(...xs), maxX = Math.max(...xs), minY = Math.min(...ys), maxY = Math.max(...ys);
  const maxR = Math.max(...placed.map(node => node.radius));
  const labelHalfW = Math.max(...placed.map(node => labelBox(0, node.lines).halfW));
  const labelH = Math.max(...placed.map(node => node.lines.length)) * LINE_HEIGHT + 2;
  const availW = width - 2 * padding, availH = height - 2 * padding;
  // Horizontally a node's reach is the larger of its scaled radius and its fixed-size label.
  const k = Math.max(0.05, Math.min(1,
    ratio(availW - 2 * labelHalfW, maxX - minX),
    ratio(availW, maxX - minX + 2 * maxR),
    ratio(availH - labelH, maxY - minY + 2 * maxR)));
  // Labels hang below the lowest node: shift the centre down by half their screen height.
  return { x: (minX + maxX) / 2, y: (minY + maxY) / 2 + labelH / 2 / k, k };
}

/** a / b, where a zero span constrains nothing (one node, or nodes in a line). */
function ratio(a: number, b: number): number {
  return b > 0 ? a / b : Infinity;
}
