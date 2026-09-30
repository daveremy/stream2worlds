// Measures how often the viewer's Active-now rows show a human name (s2w#302, s2w#288 §4).
//
//   node --experimental-strip-types scripts/measure-labels.mjs <base-url> <world> [samples] [gap-seconds]
//   node --experimental-strip-types scripts/measure-labels.mjs <base-url> <world> --sentences [last]
//
// Each sample fetches `/world` (entity lod) and the last 50 events, feeds them through the
// viewer's own `ViewState`, and takes the five Active-now rows (`activeNow`, profile.ts). A row
// shows a human name when its label resolves to a non-empty string that `isIdLike` rejects.
// It scores three label sources over the same rows:
//   baseline  the viewer's `pickLabelKeys` guess (what the live page shows today);
//   manifest  the dashboard manifest's `types[].label`, on every row;
//   primary   the manifest label, on rows whose type row has `primary: true`.
// `--sentences` prints `/sentences?last=N` for a spot check against the raw events.
// Read-only: GET requests only.

// @ts-expect-error Node's TypeScript stripping needs the extension.
import { activeNow, isIdLike, labelFor } from '../src/profile.ts';
// @ts-expect-error as above.
import { SseParser } from '../src/sse.ts';
// @ts-expect-error as above.
import { ViewState } from '../src/state.ts';

const [base, world, ...rest] = process.argv.slice(2);
if (!base || !world) {
  console.error('usage: measure-labels.mjs <base-url> <world> [samples] [gap-seconds] | --sentences [last]');
  process.exit(2);
}
const url = path => new URL(`/worlds/${encodeURIComponent(world)}/${path}`, base);

async function json(target) {
  const response = await fetch(target);
  if (!response.ok) throw new Error(`${target}: HTTP ${response.status} ${await response.text()}`);
  return response.json();
}

async function events(last) {
  const response = await fetch(url(`events?last=${last}`));
  if (!response.ok) throw new Error(`/events: HTTP ${response.status}`);
  const parser = new SseParser();
  const messages = [];
  for await (const chunk of response.body) messages.push(...parser.push(chunk));
  messages.push(...parser.end());
  return messages;
}

if (rest[0] === '--sentences') {
  const view = await json(url(`sentences?last=${Number(rest[1] ?? 20)}`));
  for (const row of view.rows) console.log(`${row.position}\t${row.source}\t${row.sentence}`);
  process.exit(0);
}

const samples = Number(rest[0] ?? 12);
const gap = Number(rest[1] ?? 50);

// The key part a `{key: i}` label names: composite keys join their parts with U+001F, and a
// string part is JSON-encoded.
function keyPart(node, index) {
  const part = node.keys[0]?.split('\u001f')[index];
  if (part === undefined) return undefined;
  try { const value = JSON.parse(part); return typeof value === 'string' ? value : String(value); }
  catch { return part; }
}

function manifestLabel(node, row) {
  if (node === undefined || node.kind === 'type' || row?.label === undefined) return undefined;
  if ('attr' in row.label) {
    const value = node.attrs[row.label.attr];
    return value !== undefined && 'Str' in value ? value.Str : undefined;
  }
  return keyPart(node, row.label.key);
}

const human = label => typeof label === 'string' && label.length > 0 && !isIdLike(label);

const dashboard = await json(url('dashboard'));
const typeRows = new Map((dashboard.manifest?.types ?? []).map(row => [row.type, row]));
const tally = { rows: 0, baseline: 0, manifest: 0, primaryRows: 0, primary: 0 };
const examples = [];

for (let sample = 0; sample < samples; sample++) {
  if (sample > 0) await new Promise(resolve => setTimeout(resolve, gap * 1000));
  const state = new ViewState(new URLSearchParams({ world }));
  state.snapshot(await json(url('world?lod=entity')));
  for (const message of await events(50)) state.apply(message);
  for (const row of activeNow(state.evidence, state.nodesById, state.keyByType, state.labels)) {
    const node = state.nodesById.get(row.id);
    const typeRow = node && node.kind !== 'type' ? typeRows.get(node.entity_type) : undefined;
    const baseline = node === undefined ? undefined : pickedLabel(node, state.keyByType);
    const manifest = manifestLabel(node, typeRow);
    tally.rows++;
    if (human(baseline)) tally.baseline++;
    if (human(manifest)) tally.manifest++;
    if (typeRow?.primary) { tally.primaryRows++; if (human(manifest)) tally.primary++; }
    examples.push([node?.entity_type ?? '?', baseline ?? '-', manifest ?? '-']);
  }
  console.error(`sample ${sample + 1}/${samples}: ${tally.rows} rows`);
}

// The baseline's label only when `pickLabelKeys` chose a key; `labelFor` falls back to the raw
// key, which the page shows but which is not a chosen name.
function pickedLabel(node, keyByType) {
  return keyByType.get(node.entity_type) === undefined ? undefined : labelFor(node, keyByType);
}

const pct = (n, d) => d === 0 ? 'n/a' : `${(100 * n / d).toFixed(1)}% (${n}/${d})`;
console.log(`rows ${tally.rows}; baseline ${pct(tally.baseline, tally.rows)}; ` +
  `manifest all ${pct(tally.manifest, tally.rows)}; manifest primary ${pct(tally.primary, tally.primaryRows)}`);
for (const [type, baseline, manifest] of examples.slice(0, 10)) console.log(`  ${type}\t${baseline}\t${manifest}`);
