// The private-stream answer key (s2w#372): generator, verifier and hand-inspection sampler.
// Plan and rulings: the s2w#372 issue comments of 2026-10-01 05:28Z.
//
// The key is data in key format 2 (xtask/src/h_measure/key.rs), read by `cargo xtask h-measure
// score` exactly as dev-key-v2.json is. Identity is the source system's own identifier: an
// item's (repo, number), a commit's 40-hex sha, a branch's (repo, name), a sprint's number,
// a review seat's id, a comment's id. The table below is the source; the two JSON files are
// its rendering and the test suite fails if either drifts.
//
// usage (from the repository root, `node --experimental-strip-types research/h-measure/private/key.ts`):
//   --write                                  rewrite the two key files from the table, and both
//                                            keys' partition shape on the synthetic fixture
//   --edges                                  print the declared relationship edges (JSON)
//   --check  --corpus SSE --provenance JSONL print the counts-only summary; exit 1 on a failed rule
//   --sample N --seed S --corpus SSE --provenance JSONL --out FILE
//                                            write the hand-inspection worksheet (outside any
//                                            git work tree, mode 0600, never over a file)
//
// --check prints counts, rule names and the capture's own header lines 2-3 (the command with
// its window, the drop counts, the extraction table), never a value from a frame, so its output
// is the publishable summary of the key. The worksheet holds values and stays under the
// h-measure --dir.

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { MARK, REPOS } from "./events.ts";
import { FIXTURE_PATH } from "./fixture.ts";

type Seg = string | number;
export type Path = Seg[];
type Scalar = string | number | boolean;
export interface Rule { path: Path; identity: Path[]; no_identity?: Scalar[] }
export interface KeyType { type: string; mentions: Rule[] }
type Unscored = Path | { prefix: Path };
/** A format-3 relationship row (`xtask/src/h_measure/key/relationships.rs`). */
export interface Relationship { type: string; from: Path; to: Path; unobservable?: string }
export interface Spec { version: 2 | 3; decode: Path[]; types: KeyType[]; unscored: Unscored[]; relationships?: Relationship[] }

const d = (...s: Seg[]): Path => ["data", ...s];
const rule = (path: Path, ...identity: Path[]): Rule => ({ path, identity: identity.length ? identity : [path] });

// ---- the key ----

/** Scored in both keys. Issues and PRs share one number space per repository, so one type. */
const BASE_TYPES: KeyType[] = [
  { type: "item", mentions: [
    rule(d("number"), d("repo"), d("number")),
    rule(d("issue"), d("repo"), d("issue")),
    rule(d("key"), d("repo"), d("issue")), // "s2w#372" on a leg is an alias of (repo, issue)
    rule(d("pr"), d("repo"), d("pr")),
    rule(d("ref_number"), d("ref_repo"), d("ref_number")), // a cross-repo ref keeps its own repo
  ] },
  { type: "commit", mentions: [
    rule(d("sha")), rule(d("head_sha")), rule(d("merge_commit_sha")), rule(d("commit_sha")),
    rule(d("parents", 0)), rule(d("parents", 1)),
  ] },
  { type: "branch", mentions: [
    rule(d("branch"), d("repo"), d("branch")),
    rule(d("head_ref"), d("repo"), d("head_ref")),
  ] },
  { type: "sprint", mentions: [rule(d("sprint")), rule(d("slot"), d("sprint")), rule(d("file_id"), d("sprint"))] },
  { type: "seat", mentions: [rule(d("seat_id"))] }, // one seat across its fallback or re-run attempts (not singleton-only; s2w#372 note)
  { type: "comment", mentions: [rule(d("comment_id"))] }, // singleton type
];

/** Scored only in the context-scored variant (ruling 4): two repo values and one observable
 * actor would carry a large share of the micro score for a trivially keyed field. */
const CONTEXT_TYPES: KeyType[] = [
  { type: "repo", mentions: [rule(d("repo")), rule(d("ref_repo"))] },
  { type: "actor", mentions: [
    { ...rule(d("actor")), no_identity: ["other"] }, { ...rule(d("author")), no_identity: ["other"] },
  ] },
];
const CONTEXT_PATHS: Path[] = CONTEXT_TYPES.flatMap((t) => t.mentions.map((m) => m.path));

/** Unscored in both (contract B3 "ambiguous or unobservable"): plumbing, values and transport.
 * `refs` is ambiguous by construction: `issueRefs` keeps `#n` and drops a `lifeos#`/`s2w#`
 * prefix, so the repo of a ref is unknown (ruling 2; v1 if the capture carries it). */
const UNSCORED: Unscored[] = [
  ...["kind", "ts", "ts_start", "ts_end", "is_pr", "step", "leg", "round", "verdict", "engines",
    "engine", "model", "tier", "script", "event", "label", "pts", "comment_bytes", "co_authored",
    "base_ref", "row_id"].map((k) => d(k)),
  { prefix: d("refs") },
];

export type Variant = "base" | "context-scored";
export type Format = 2 | 3;
/** The key files per key format: v0 is format 2 (pinned, never rewritten); v1 is format 3, v0
 * plus `EDGES` as relationship rows (s2w#388). `--write` writes only the newest. */
export const KEY_FILES: Record<Format, Record<Variant, string>> = {
  2: { base: "private-key-v0.json", "context-scored": "private-key-v0.context-scored.json" },
  3: { base: "private-key-v1.json", "context-scored": "private-key-v1.context-scored.json" },
};
export const KEY_DIR = fileURLToPath(new URL("..", import.meta.url));

export function spec(variant: Variant, version: Format = 3): Spec {
  const s: Spec = variant === "base"
    ? { version, decode: [["data"]], types: BASE_TYPES, unscored: [...CONTEXT_PATHS, ...UNSCORED] }
    : { version, decode: [["data"]], types: [...BASE_TYPES, ...CONTEXT_TYPES], unscored: UNSCORED };
  return version === 3 ? { ...s, relationships: relationships() } : s;
}

/** Compact JSON with `, ` and `: ` separators and `{ … }` objects, as the dev keys are written. */
const j = (v: unknown): string =>
  Array.isArray(v) ? `[${v.map(j).join(", ")}]`
    : v !== null && typeof v === "object" ? `{ ${Object.entries(v).map(([k, x]) => `${JSON.stringify(k)}: ${j(x)}`).join(", ")} }`
      : JSON.stringify(v);

/** The key file's bytes: dev-key-v2.json's layout, one mention rule and one unscored entry per line. */
export function render(s: Spec): string {
  const types = s.types.map((t) => `    { "type": ${j(t.type)}, "mentions": [\n` +
    t.mentions.map((m) => `      ${j(m)}`).join(",\n") + "\n    ] }");
  const rows = s.relationships?.length
    ? `,\n  "relationships": [\n${s.relationships.map((r) => `    ${j(r)}`).join(",\n")}\n  ]` : "";
  return `{\n  "version": ${s.version},\n  "decode": ${j(s.decode)},\n  "types": [\n${types.join(",\n")}\n  ],\n` +
    `  "unscored": [\n${s.unscored.map((u) => `    ${j(u)}`).join(",\n")}\n  ]${rows}\n}\n`;
}

// ---- relationships: `EDGES` is rendered into the format-3 key (s2w#388); `kinds`/`event` stay notes here ----

/** A typed directed edge between two mentions in one frame: `from` and `to` are mention paths of
 * the base key, present together on frames of kind `kinds` (and timeline `event`, if given).
 * `observable: false` marks an edge the capture cannot carry. */
export interface Edge { label: string; kinds: string[]; event?: string; from: Path; to: Path; observable: boolean; note?: string }
const edge = (label: string, kinds: string[], from: Path, to: Path, extra: Partial<Edge> = {}): Edge =>
  ({ label, kinds, from, to, observable: true, ...extra });
export const EDGES: Edge[] = [
  edge("cross-references", ["issue.event"], d("number"), d("ref_number"), { event: "cross-referenced" }),
  edge("references", ["issue.event"], d("number"), d("commit_sha"), { event: "referenced" }),
  edge("has-comment", ["issue.event"], d("number"), d("comment_id"), { event: "commented" }),
  edge("head-branch", ["pr.opened"], d("number"), d("head_ref")),
  edge("head-commit", ["pr.merged", "pr.closed"], d("number"), d("head_sha")),
  edge("merged-as", ["pr.merged"], d("number"), d("merge_commit_sha")),
  edge("reviews-commit", ["review.seat"], d("seat_id"), d("head_sha")),
  edge("reviews-branch", ["review.seat"], d("seat_id"), d("branch")),
  edge("reviews-item", ["review.seat"], d("seat_id"), d("issue"),
    { note: "absent where the sidecar marks issue_unobservable" }),
  edge("leg-branch", ["leg.status"], d("key"), d("branch")),
  edge("leg-commit", ["leg.status"], d("key"), d("sha")),
  edge("leg-pr", ["leg.status"], d("key"), d("pr")),
  edge("parent", ["commit"], d("sha"), d("parents", 0)),
  edge("parent", ["commit"], d("sha"), d("parents", 1)),
  edge("names", ["pr.opened"], d("number"), d("refs"), { observable: false, note: "refs drop the repo prefix (ruling 2)" }),
  edge("names", ["commit"], d("sha"), d("refs"), { observable: false, note: "refs drop the repo prefix (ruling 2)" }),
];

/** `EDGES` as format-3 rows: `label` is the type; an unobservable edge carries its note as the
 * reason. `kinds`, `event` and an observable edge's note stay here: format 3 has no guard, so an
 * edge is pure co-occurrence of its two mentions in one frame (s2w#388 ruling 3). */
function relationships(): Relationship[] {
  return EDGES.map((e) => ({
    type: e.label, from: e.from, to: e.to,
    ...(e.observable ? {} : { unobservable: e.note ?? "unobservable" }),
  }));
}

// ---- executor: the Rust key executor's semantics (xtask/src/h_measure/mentions.rs) ----

/** `s2w_discover::rule_id`: segments joined by `.` (no key in this key holds a `.`). */
export const pathId = (p: Path) => p.join(".");

/** `s2w_system1::decode::key_part`: a string, an i64 integer or a boolean; typed, so 0 ≠ "0". */
// A number is keyed when it is a safe integer. Rust's `as_i64` also rejects a float literal
// such as `1.0`, which JSON.parse reads as 1; the capture writes through JSON.stringify, which
// never emits one, so the two agree on every capture this tool reads.
function keyPart(v: unknown): string | undefined {
  if (typeof v === "string" || typeof v === "boolean") return JSON.stringify(v);
  if (typeof v === "number" && Number.isSafeInteger(v)) return JSON.stringify(v);
  return undefined;
}

function lookup(value: unknown, path: Path): unknown {
  let at = value;
  for (const s of path) {
    if (typeof s === "number") at = Array.isArray(at) ? at[s] : undefined;
    else at = at !== null && typeof at === "object" && !Array.isArray(at) && Object.hasOwn(at, s) ? (at as Record<string, unknown>)[s] : undefined;
    if (at === undefined) return undefined;
  }
  return at;
}

export interface Frame { id: string; record: { data: Record<string, unknown> } | null }

/** Header lines (`: …`) and frames of one SSE corpus as the capture renders it. */
export function parseSse(text: string): { header: string[]; frames: Frame[] } {
  const [head, ...blocks] = text.split("\n\n");
  const header = head.split("\n").filter((l) => l.startsWith(": ")).map((l) => l.slice(2));
  const frames: Frame[] = [];
  for (const b of blocks) {
    if (!b.trim()) continue;
    const lines = b.split("\n");
    const id = lines.find((l) => l.startsWith("id: "))?.slice(4);
    const data = lines.filter((l) => l.startsWith("data: "));
    if (id === undefined || data.length !== 1) throw new Error(`frame ${frames.length}: not one id and one data line`);
    let record: Frame["record"] = null;
    try { record = { data: JSON.parse(data[0].slice(6)) }; } catch { record = null; }
    frames.push({ id, record });
  }
  return { header, frames };
}

export interface Mention { frame: number; path: string; type: string; cluster: string; value: unknown }

export function execute(s: Spec, frames: Frame[]) {
  const mentions: Mention[] = [];
  const abstained: Record<string, number> = {};
  const excluded: Record<string, number> = {};
  frames.forEach((f, frame) => {
    if (!f.record) return;
    for (const t of s.types) for (const r of t.mentions) {
      const value = lookup(f.record, r.path);
      const part = keyPart(value);
      if (part === undefined) continue;
      const path = pathId(r.path);
      if ((r.no_identity ?? []).some((x) => keyPart(x) === part)) { excluded[path] = (excluded[path] ?? 0) + 1; continue; }
      const parts = r.identity.map((p) => keyPart(lookup(f.record, p)));
      if (parts.some((p) => p === undefined)) { abstained[path] = (abstained[path] ?? 0) + 1; continue; }
      mentions.push({ frame, path, type: t.type, cluster: `${t.type}\u001f${parts.join("\u001f")}`, value });
    }
  });
  // The gold edges: per frame and observable row whose two paths both hold a gold mention there,
  // `type \u001f from cluster \u001f to cluster`, unique (the Rust `key_mentions` edges).
  const edges = new Set<string>();
  const rows = (s.relationships ?? []).filter((r) => r.unobservable === undefined);
  const here = new Map<string, string>();
  for (let i = 0; i <= mentions.length; i += 1) {
    const m = mentions[i];
    if (i > 0 && (m === undefined || m.frame !== mentions[i - 1].frame)) {
      for (const r of rows) {
        const from = here.get(pathId(r.from)), to = here.get(pathId(r.to));
        if (from !== undefined && to !== undefined) edges.add(`${r.type}\u001f${from}\u001f${to}`);
      }
      here.clear();
    }
    if (m) here.set(m.path, m.cluster);
  }
  return { mentions, abstained, excluded, edges, undecodable: frames.filter((f) => !f.record).length };
}

/** The partition's shape: what `key.test.ts` and the Rust test both compute on the fixture. */
export function shape(s: Spec, frames: Frame[]) {
  const { mentions, abstained, excluded, edges } = execute(s, frames);
  const sorted = <T>(o: Record<string, T>) => Object.fromEntries(Object.entries(o).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
  const per_path: Record<string, number> = {};
  const clusters = new Map<string, { type: string; size: number }>();
  for (const m of mentions) {
    per_path[m.path] = (per_path[m.path] ?? 0) + 1;
    const c = clusters.get(m.cluster) ?? { type: m.type, size: 0 };
    c.size += 1;
    clusters.set(m.cluster, c);
  }
  const entity_sizes: Record<string, Record<string, number>> = {};
  for (const { type, size } of clusters.values()) {
    const h = (entity_sizes[type] ??= {});
    h[size] = (h[size] ?? 0) + 1;
  }
  return {
    mentions: mentions.length, entities: clusters.size, mentions_per_path: sorted(per_path),
    entity_sizes: sorted(entity_sizes), abstained: sorted(abstained), excluded: sorted(excluded),
    ...(s.relationships?.length ? { edges_per_type: sorted(perType(edges)) } : {}),
  };
}

/** Unique gold edges per relationship type. */
function perType(edges: Set<string>): Record<string, number> {
  const out: Record<string, number> = {};
  for (const e of edges) { const t = e.split("\u001f")[0]; out[t] = (out[t] ?? 0) + 1; }
  return out;
}

/** Both keys' partition shapes on the synthetic fixture. `xtask/src/h_measure/private_key_tests.rs`
 * computes the same with the Rust executor and compares, so the two executors cannot drift. */
export const SHAPE_PATH = fileURLToPath(new URL("./fixture/synthetic-20.key-shape.json", import.meta.url));
export function fixtureShape(): string {
  const { frames } = parseSse(readFileSync(FIXTURE_PATH, "utf8"));
  // One line per variant and section, so a drift diff names the section that moved.
  const variant = (v: Variant) => Object.entries(shape(spec(v), frames))
    .map(([k, x]) => `    ${JSON.stringify(k)}: ${JSON.stringify(x)}`).join(",\n");
  return `{\n  "base": {\n${variant("base")}\n  },\n  "context-scored": {\n${variant("context-scored")}\n  }\n}\n`;
}

// ---- verification against the provenance sidecar ----

export const RULES = ["sidecar-aligned", "sha-shape", "sha-resolved", "ref-repo", "seat-issue", "leg-key"] as const;
type RuleName = (typeof RULES)[number];

export function check(header: string[], frames: Frame[], provenanceText: string) {
  const s = spec("base");
  const failures: Record<RuleName, number> = Object.fromEntries(RULES.map((r) => [r, 0])) as Record<RuleName, number>;
  const side = provenanceText.split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l) as Record<string, unknown>);
  if (side.length !== frames.length) failures["sidecar-aligned"] += Math.abs(side.length - frames.length);
  frames.forEach((f, i) => { if (side[i] && side[i].id !== f.id) failures["sidecar-aligned"] += 1; });
  const { mentions, undecodable } = execute(s, frames);
  const commitPaths = new Set(s.types.find((t) => t.type === "commit")!.mentions.map((m) => pathId(m.path)));
  for (const m of mentions) if (commitPaths.has(m.path) && !/^[0-9a-f]{40}$/.test(String(m.value))) failures["sha-shape"] += 1;
  const opened = new Set<string>();
  for (const f of frames) if (f.record?.data.kind === "pr.opened") opened.add(`${f.record.data.repo}#${f.record.data.number}`);
  let legPrUnopened = 0;
  frames.forEach((f, i) => {
    const x = f.record?.data;
    const p = side[i];
    if (!x || !p) return;
    if ("sha_written" in p) {
      const written = x.kind === "review.seat" ? (x.head_sha ?? null) : (x.sha ?? null);
      if (written !== p.sha_resolved) failures["sha-resolved"] += 1;
    }
    if (x.ref_number != null && !REPOS.includes(x.ref_repo as never)) failures["ref-repo"] += 1;
    if (x.kind === "review.seat") {
      const expect = x.issue === null && p.branch_pr === null;
      if (p.issue_unobservable !== expect) failures["seat-issue"] += 1;
    }
    if (x.kind === "leg.status") {
      // A tripwire: today's capture derives `issue` from `key`, so this only fires if that changes.
      if (x.key !== `${x.repo}#${x.issue}`) failures["leg-key"] += 1;
      if (x.pr != null && !opened.has(`${x.repo}#${x.pr}`)) legPrUnopened += 1;
    }
  });
  const edges: Record<string, number> = {};
  for (const e of EDGES) {
    const k = `${e.label} ${pathId(e.from)} -> ${pathId(e.to)}`;
    edges[k] = e.observable ? frames.filter((f) => {
      const x = f.record?.data;
      return x && e.kinds.includes(String(x.kind)) && (!e.event || x.event === e.event) &&
        keyPart(lookup(f.record, e.from)) !== undefined && keyPart(lookup(f.record, e.to)) !== undefined;
    }).length : 0;
  }
  const failed = Object.values(failures).reduce((a, b) => a + b, 0);
  return {
    frames: frames.length,
    undecodable,
    capture_header: header.slice(1, 3), // line 1 is the provenance line; 2-3 are the command and the drop counts
    key: shape(s, frames),
    context_scored: shape(spec("context-scored"), frames),
    failures, failed,
    // a leg names a PR whose `pr.opened` lies outside the span: counted, not a failure
    leg_pr_unopened: legPrUnopened,
    edges_observed: edges,
  };
}

// ---- the hand-inspection worksheet (run on the private span in PR 2) ----

/** mulberry32: a small seeded PRNG, so a (seed, N) worksheet is reproducible. */
function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** N mentions stratified by mention path: each path's mentions shuffled, then taken round-robin
 * over the paths in sorted order, so every path appears once N reaches the number of paths. */
export function draw(mentions: Mention[], n: number, seed: number): Mention[] {
  const rand = prng(seed);
  const by = new Map<string, Mention[]>();
  for (const m of mentions) by.set(m.path, [...(by.get(m.path) ?? []), m]);
  const strata = [...by.keys()].sort().map((k) => {
    const xs = [...by.get(k)!];
    for (let i = xs.length - 1; i > 0; i--) { const r = Math.floor(rand() * (i + 1)); [xs[i], xs[r]] = [xs[r], xs[i]]; }
    return xs;
  });
  const out: Mention[] = [];
  for (let round = 0; out.length < n && strata.some((xs) => xs.length > round); round++)
    for (const xs of strata) if (round < xs.length && out.length < n) out.push(xs[round]);
  return out;
}

const GH: Record<string, string> = { lifeos: "daveremy/lifeos", s2w: "daveremy/stream2worlds" };
const CLONE: Record<string, string> = { lifeos: "$HOME/lifeos", s2w: "$HOME/code/stream2worlds" };

function known(table: Record<string, string>, repo: unknown): string {
  const v = Object.hasOwn(table, String(repo)) ? table[String(repo)] : undefined;
  if (v === undefined) throw new Error(`key.ts: no source lookup for repo ${JSON.stringify(repo)}`);
  return v;
}

/** The exact source lookup a human runs to confirm one mention. */
function lookupCommand(x: Record<string, unknown>, p: Record<string, unknown>, path: string): string {
  switch (p.source) {
    case "worker_history":
      return `sqlite3 -json "file:$HOME/.dev-worker/dev-worker.db?mode=ro" "select id, key, step, detail, timestamp from worker_history where id=${Number(p.row_id)}"`;
    case "review_seats": {
      const o = Number(p.offset), day = String(Math.floor(o / 100000));
      return `sed -n '${o % 100000}p' "$HOME/lifeos/logs/review-seats/${day.slice(0, 4)}-${day.slice(4, 6)}-${day.slice(6)}.jsonl"`;
    }
    case "github":
      return `gh api repos/${known(GH, x.repo)}/${x.is_pr ? "pulls" : "issues"}/${Number(x.number)}`;
    case "github-timeline":
      return `gh api --paginate repos/${known(GH, x.repo)}/issues/${Number(x.number)}/timeline`;
    case "git":
      return `git -C "${known(CLONE, x.repo)}" show -s --format='%H %P' ${path.startsWith("data.parents") ? String(x.sha) : String(p.sha)}`;
    case "sprint-log":
      return `grep -n '^| ${Number(x.sprint)} |' "$HOME/lifeos/docs/sprints/README.md"`;
    default:
      return "(no lookup for this source)";
  }
}

export function worksheet(frames: Frame[], provenanceText: string, n: number, seed: number): string {
  const side = provenanceText.split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l) as Record<string, unknown>);
  const picked = draw(execute(spec("base"), frames).mentions, n, seed);
  const out = [`: ${MARK}private key-sample n=${n} seed=${seed}; holds values: never inside a work tree\n`];
  picked.forEach((m, i) => {
    const x = frames[m.frame].record!.data;
    out.push(`\n## ${i + 1}/${picked.length} ${m.path} (${m.type})\nframe: ${frames[m.frame].id}\nvalue: ${JSON.stringify(m.value)}\n` +
      `sidecar: ${JSON.stringify(side[m.frame] ?? null)}\nlookup: ${lookupCommand(x, side[m.frame] ?? {}, m.path)}\nagrees: \n`);
  });
  return out.join("");
}

/** Outside a work tree only on git's own "not a git repository" answer (as capture.ts). */
function refuseInsideWorkTree(dir: string): void {
  const g = spawnSync("git", ["-C", dir, "rev-parse", "--is-inside-work-tree"], { encoding: "utf8", env: { ...process.env, LC_ALL: "C" } });
  if (!(g.status !== 0 && /not a git repository/.test(g.stderr ?? "")))
    throw new Error("refusing to write a worksheet inside, or possibly inside, a git work tree");
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const { values: a } = parseArgs({
    options: {
      write: { type: "boolean" }, edges: { type: "boolean" }, check: { type: "boolean" },
      sample: { type: "string" }, seed: { type: "string" }, corpus: { type: "string" },
      provenance: { type: "string" }, out: { type: "string" },
    },
  });
  if (a.write) {
    for (const v of ["base", "context-scored"] as const) writeFileSync(join(KEY_DIR, KEY_FILES[3][v]), render(spec(v)));
    writeFileSync(SHAPE_PATH, fixtureShape());
  } else if (a.edges) {
    process.stdout.write(JSON.stringify(EDGES, null, 2) + "\n");
  } else if (a.corpus && a.provenance && (a.check || a.sample)) {
    const { header, frames } = parseSse(readFileSync(a.corpus, "utf8"));
    const prov = readFileSync(a.provenance, "utf8");
    if (a.check) {
      const report = check(header, frames, prov);
      process.stdout.write(JSON.stringify(report, null, 2) + "\n");
      if (report.failed > 0) process.exit(1);
    } else {
      const n = Number(a.sample), seed = Number(a.seed ?? "0");
      if (!Number.isSafeInteger(n) || n < 1 || !Number.isSafeInteger(seed) || !a.out) throw new Error("--sample N needs --seed S and --out FILE");
      refuseInsideWorkTree(dirname(a.out));
      if (existsSync(a.out)) throw new Error("refusing to overwrite the worksheet");
      writeFileSync(a.out, worksheet(frames, prov, n, seed), { mode: 0o600, flag: "wx" });
    }
  } else {
    process.stderr.write("usage: key.ts --write | --edges | --check|--sample N --seed S --out FILE  --corpus SSE --provenance JSONL\n");
    process.exit(2);
  }
}
