import type { Node } from './api';

// The dashboard manifest's presentation, as the viewer uses it (decision 0029, s2w#289). Pure:
// no DOM, no fetch, so tests/manifest.test.mjs imports it directly. Domain-free: every name,
// noun and sentence comes from the manifest; the code knows only the closed kind set.

/// Mirrors s2w-model's closed `Kind`.
export type Kind = 'person' | 'document' | 'category' | 'place' | 'organisation' | 'event' | 'other';
export type TypeLabel = { attr: string } | { key: number };
export type TypeRow = { type: string; primary: boolean; noun?: string; label?: TypeLabel; kind?: Kind };
/// The parts of GET /worlds/{world}/dashboard the viewer reads.
export type Dashboard = {
  manifest: { domain: { name: string; summary: string }; types: TypeRow[]; events?: unknown[] | null } | null;
};
/// Mirrors query/sentences.rs.
export type SentenceEntity = { type: string; key: string; entity?: number; label?: string };
export type SentenceRow = { position: number; source: string; sentence: string | null; entities: SentenceEntity[] };

/// What the viewer keeps of an effective manifest.
export type Manifest = { rows: Map<string, TypeRow>; sentences: boolean; domain: string };

/// One icon per kind: Unicode glyphs, never images.
export const KIND_ICON: Record<Kind, string> = {
  person: '\u{1F464}', document: '\u{1F4C4}', category: '\u{1F3F7}\u{FE0F}', place: '\u{1F4CD}',
  organisation: '\u{1F3E2}', event: '\u{1F4C5}', other: '\u{2022}',
};

/// The viewer's manifest, or undefined when none is in effect (today's view then).
export function fromDashboard(dashboard: Dashboard | undefined): Manifest | undefined {
  const manifest = dashboard?.manifest;
  if (!manifest) return undefined;
  return {
    rows: new Map(manifest.types.map(row => [row.type, row])),
    sentences: Array.isArray(manifest.events) && manifest.events.length > 0,
    domain: manifest.domain.name,
  };
}

/// The kind icon of a type, or '' when the manifest names none.
export function iconFor(manifest: Manifest | undefined, entityType: string): string {
  const kind = manifest?.rows.get(entityType)?.kind;
  return kind === undefined ? '' : KIND_ICON[kind] ?? '';
}

/// `text` prefixed with the type's icon, when it has one.
export function withIcon(manifest: Manifest | undefined, entityType: string, text: string): string {
  const icon = iconFor(manifest, entityType);
  return icon ? `${icon} ${text}` : text;
}

/// The type's noun, else the type label itself.
export function nounFor(manifest: Manifest | undefined, entityType: string): string {
  return manifest?.rows.get(entityType)?.noun ?? entityType;
}

const isDisplaySpace = (c: string) => c === ' ' || c === '\t' || c === '\n' || c === '\f' || c === '\r' || c === ' ';

/// The same rules as s2w-model's `display_text`: `/* … */` spans removed (an unterminated `/*`
/// stays), whitespace runs collapsed and trimmed, and one token containing `_` shown with
/// spaces. Whitespace is ASCII whitespace plus the no-break space, exactly as in Rust.
export function displayText(raw: string): string {
  let stripped = '', rest = raw;
  for (;;) {
    const open = rest.indexOf('/*');
    if (open < 0) break;
    const close = rest.indexOf('*/', open + 2);
    if (close < 0) break;
    stripped += rest.slice(0, open) + ' ';
    rest = rest.slice(close + 2);
  }
  stripped += rest;
  const words: string[] = [];
  let word = '';
  for (const c of stripped) {
    if (isDisplaySpace(c)) { if (word) words.push(word); word = ''; } else word += c;
  }
  if (word) words.push(word);
  if (words.length === 1 && words[0].includes('_')) return words[0].split('_').filter(Boolean).join(' ');
  return words.join(' ');
}

/// One encoded key part as text: a JSON string as its value, any other JSON as its text.
function partText(part: string): string {
  try { const value = JSON.parse(part) as unknown; return typeof value === 'string' ? value : String(value); }
  catch { return part; }
}

/// Key part `index` (counted after the type label) of the node's first key, as text.
export function keyPart(node: Extract<Node, { kind: 'entity' | 'hub' }>, index: number): string | undefined {
  const part = node.keys[0]?.split('\u001f')[index + 1];
  return part === undefined ? undefined : partText(part);
}

/// A natural key as a reader sees it: the parts after the type label, each as display text,
/// joined by spaces; the raw key when nothing is left.
export function keyText(key: string): string {
  const text = key.split('\u001f').slice(1).map(part => displayText(partText(part))).filter(Boolean).join(' ');
  return text || key;
}

/// The label the type row names for a node, as display text; undefined when the row names none
/// or the value is missing or empty.
export function manifestLabel(node: Node, manifest: Manifest | undefined): string | undefined {
  if (node.kind === 'type') return undefined;
  const label = manifest?.rows.get(node.entity_type)?.label;
  if (label === undefined) return undefined;
  let raw: string | undefined;
  if ('attr' in label) {
    const value = node.attrs[label.attr];
    raw = value === undefined ? undefined : 'Str' in value ? value.Str : 'Int' in value ? String(value.Int) : String(value.Bool);
  } else raw = keyPart(node, label.key);
  const text = raw === undefined ? '' : displayText(raw);
  return text || undefined;
}

/// A type node's label with a manifest row: `<icon> <noun> (<count>)`.
export function typeNodeLabel(node: Extract<Node, { kind: 'type' }>, manifest: Manifest | undefined): string | undefined {
  const row = manifest?.rows.get(node.entity_type);
  if (row === undefined) return undefined;
  return withIcon(manifest, node.entity_type, `${row.noun ?? node.entity_type} (${node.count})`);
}

/// `1 change`, `2 changes`. Manifest v0 names no event noun, so the viewer says "changes"
/// for every stream (decision 0018; s2w#347 adds the manifest's own word).
export function changesText(count: number): string {
  return `${count} ${count === 1 ? 'change' : 'changes'}`;
}

/// The feed's sentences, newest first; rows with no sentence are skipped.
export function feedSentences(rows: SentenceRow[]): string[] {
  const sentences: string[] = [];
  for (let i = rows.length - 1; i >= 0; i--) {
    const sentence = rows[i].sentence;
    if (sentence) sentences.push(sentence);
  }
  return sentences;
}

/// Active now from the feed: the primary types' entities named by the most events, one count
/// per event per entity, ties to the most recent. Labels: the row's own label, else the node's
/// label, else the key as display text.
export function sentenceActive(
  rows: SentenceRow[], manifest: Manifest | undefined, labelOf: (entity: number) => string | undefined, limit = 5,
): { type: string; label: string; count: number }[] {
  const counts = new Map<string, { type: string; label: string; count: number }>();
  for (let i = rows.length - 1; i >= 0; i--) {
    const seen = new Set<string>();
    for (const entity of rows[i].entities) {
      if (manifest?.rows.get(entity.type)?.primary !== true) continue;
      const id = entity.entity === undefined ? `k:${entity.type}\u0000${entity.key}` : `e:${entity.entity}`;
      if (seen.has(id)) continue;
      seen.add(id);
      const item = counts.get(id);
      if (item !== undefined) { item.count++; continue; }
      const label = entity.label ?? (entity.entity === undefined ? undefined : labelOf(entity.entity)) ??
        keyText(entity.key);
      counts.set(id, { type: entity.type, label, count: 1 });
    }
  }
  // Map order is first-seen, newest first; a stable sort keeps it for ties.
  return [...counts.values()].sort((a, b) => b.count - a.count).slice(0, limit);
}

/// Whether a failed manifest load or sentence poll can succeed later: network errors, 429 and
/// the gateway answers a proxy gives while the server restarts (502, 503, 504) can; any other
/// API answer (400, 404, 500, ...) fails the same way forever, so the poll stops.
export function pollRetries(error: unknown): boolean {
  const status = (error as { status?: unknown } | null)?.status;
  return typeof status !== 'number' || status === 429 || status === 502 || status === 503 || status === 504;
}
