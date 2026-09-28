import type { Link, Message, Node } from './api';

const TYPE_COLORS = ['#70c9ee', '#f6bd60', '#84dcc6', '#f28482', '#b8a1e3', '#90be6d', '#ff9f68', '#7eb6ff', '#e78ac3'];
const LINK_COLORS = ['#9ec5fe', '#ffd166', '#80ed99', '#ff8fa3', '#c8b6ff', '#72ddf7', '#f4a261', '#a7c957'];

type LabelKeyMap = Map<string, string | undefined>;

function hash(value: string): number {
  let result = 0;
  for (const char of value) result = (Math.imul(result, 31) + char.codePointAt(0)!) >>> 0;
  return result;
}

function isIdLike(value: string): boolean {
  if (value.length > 80) return true;
  if (/\s/.test(value) || value.length === 0) return false;
  const matching = [...value].filter(char => /[0-9a-f-]/i.test(char)).length;
  return matching / [...value].length > 0.8;
}

export function pickLabelKeys(nodes: Node[]): LabelKeyMap {
  const byType = new Map<string, Extract<Node, { kind: 'entity' | 'hub' }>[]>();
  for (const node of nodes) {
    if (node.kind === 'type') continue;
    const group = byType.get(node.entity_type) ?? [];
    group.push(node);
    byType.set(node.entity_type, group);
  }

  const result: LabelKeyMap = new Map();
  for (const [entityType, entities] of byType) {
    const candidates = new Map<string, string[]>();
    for (const entity of entities) {
      for (const [key, value] of Object.entries(entity.attrs)) {
        if (!('Str' in value) || value.Str.length === 0) continue;
        const values = candidates.get(key) ?? [];
        values.push(value.Str);
        candidates.set(key, values);
      }
    }
    const ranked = [...candidates].flatMap(([key, values]) => {
      const coverage = values.length / entities.length;
      const idLike = values.filter(isIdLike).length;
      if (coverage < 0.5 || idLike > values.length / 2) return [];
      const distinct = new Set(values).size;
      return [{ key, whitespace: values.some(value => /\s/.test(value)),
        score: coverage * (distinct / values.length), distinct }];
    }).sort((left, right) => Number(right.whitespace) - Number(left.whitespace) ||
      right.score - left.score || right.distinct - left.distinct || (left.key < right.key ? -1 : left.key > right.key ? 1 : 0));
    result.set(entityType, ranked[0]?.key);
  }
  return result;
}

export function labelFor(node: Node, keyByType: LabelKeyMap): string {
  if (node.kind === 'type') return `${node.entity_type} (${node.count})`;
  const key = keyByType.get(node.entity_type);
  const value = key === undefined ? undefined : node.attrs[key];
  if (value !== undefined && 'Str' in value && value.Str.length > 0) return value.Str;
  return node.keys.join(', ') || `#${node.entity}`;
}

export function labeledSummary(message: Message, nodesById: Map<number, Node>, keyByType: LabelKeyMap): string {
  const idFields = new Set(['entity', 'resolved', 'source', 'target', 'hub', 'survivor', 'absorbed']);
  const labeled: Record<string, unknown> = { ...message };
  for (const key of Object.keys(labeled)) {
    const value = labeled[key];
    if (!idFields.has(key) || typeof value !== 'number') continue;
    const node = nodesById.get(value);
    labeled[key] = node === undefined ? `#${value}` : labelFor(node, keyByType);
  }
  return JSON.stringify(labeled);
}

export function typeColor(entityType: string): string {
  return TYPE_COLORS[hash(entityType) % TYPE_COLORS.length];
}

export function linkColor(kind: string): string {
  return LINK_COLORS[hash(kind) % LINK_COLORS.length];
}

export function nodeById(nodes: Node[]): Map<number, Node> {
  return new Map(nodes.flatMap(node => node.kind === 'type' ? [] : [[node.entity, node] as const]));
}

export function degreeById(nodes: Node[], links: Link[]): Map<string, number> {
  const result = new Map(nodes.map(node => [node.id, 0]));
  for (const link of links) {
    if (result.has(link.source)) result.set(link.source, result.get(link.source)! + 1);
    if (link.target !== link.source && result.has(link.target)) result.set(link.target, result.get(link.target)! + 1);
  }
  return result;
}

export function activeNow(
  evidence: { offset: number; kind: string; entityIds: number[]; summary: string }[],
  nodesById: Map<number, Node>, keyByType: LabelKeyMap, windowSize = 50,
): { label: string; count: number }[] {
  const counts = new Map<number, number>();
  for (const row of evidence.slice(-windowSize)) {
    for (const id of row.entityIds) counts.set(id, (counts.get(id) ?? 0) + 1);
  }
  return [...counts].sort((left, right) => right[1] - left[1]).slice(0, 5).map(([id, count]) => {
    const node = nodesById.get(id);
    return { label: node === undefined ? `#${id}` : labelFor(node, keyByType), count };
  });
}

export function topHubs(nodes: Node[], links: Link[], keyByType: LabelKeyMap, limit = 5): { label: string; degree: number }[] {
  const degrees = degreeById(nodes, links);
  return nodes.filter((node): node is Extract<Node, { kind: 'entity' | 'hub' }> => node.kind !== 'type')
    .map(node => ({ node, degree: node.kind === 'hub' ? node.in_degree : degrees.get(node.id) ?? 0 }))
    .sort((left, right) => right.degree - left.degree)
    .slice(0, limit)
    .map(({ node, degree }) => ({ label: labelFor(node, keyByType), degree }));
}
