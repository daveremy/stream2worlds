// Tests for the private-stream answer key (s2w#372). Run from the repository root:
//   node --experimental-strip-types --test research/h-measure/private/key.test.ts
// Synthetic fixture only: no real capture is ever read here.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { FIXTURE_PATH, FIXTURE_PROVENANCE_PATH, syntheticCapture } from "./fixture.ts";
import {
  EDGES, FORMAT, KEY_DIR, KEY_FILES, LATEST, RULES, SHAPE_PATH, check, draw, edgesOf, execute,
  fixtureShape, parseSse, pathId, render, spec, worksheet,
} from "./key.ts";

const REGEN = "regenerate with: node --experimental-strip-types research/h-measure/private/key.ts --write";
const sse = readFileSync(FIXTURE_PATH, "utf8");
const prov = readFileSync(FIXTURE_PROVENANCE_PATH, "utf8");
const { header, frames } = parseSse(sse);

test("the committed key files and fixture shape are exactly what key.ts generates", () => {
  // v0 and v1 are pinned and never rewritten; `spec` must still render them byte for byte.
  for (const version of [0, 1, 2] as const) for (const v of ["base", "context-scored"] as const)
    assert.equal(readFileSync(join(KEY_DIR, KEY_FILES[version][v]), "utf8"), render(spec(v, version)), REGEN);
  assert.equal(readFileSync(SHAPE_PATH, "utf8"), fixtureShape(), REGEN);
});

test("the committed fixture sidecar is exactly what fixture.ts generates", () => {
  assert.equal(prov, syntheticCapture().provenance, "regenerate with: node --experimental-strip-types research/h-measure/private/fixture.ts --write");
});

// Names only: the sha256 pin itself is checked by xtask's private_key_tests (Pins::key).
test("the keys are pinned in keys.toml", () => {
  const toml = readFileSync(join(KEY_DIR, "keys.toml"), "utf8");
  for (const f of Object.values(KEY_FILES).flatMap(Object.values)) assert.ok(toml.includes(`file = "${f}"`), f);
});

test("repo and actor are unscored in the base key and scored in the variant (ruling 4)", () => {
  const scored = (v: "base" | "context-scored") => new Set(spec(v).types.flatMap((t) => t.mentions.map((m) => pathId(m.path))));
  const unscored = (v: "base" | "context-scored") => JSON.stringify(spec(v).unscored);
  for (const p of ["data.repo", "data.ref_repo", "data.actor", "data.author"]) {
    assert.ok(!scored("base").has(p) && unscored("base").includes(`"${p.split(".")[1]}"`), p);
    assert.ok(scored("context-scored").has(p) && !unscored("context-scored").includes(`"${p.split(".")[1]}"`), p);
  }
  // v2 (s2w#395): a ref's repo follows `ref_repo`, unscored in the base key and a `repo` mention
  // in the variant; v0 and v1 leave all of `refs` unscored under one prefix.
  for (const i of [0, 7]) {
    const p = JSON.stringify(["data", "refs", i, "repo"]);
    assert.ok(unscored("base").includes(p) && !scored("base").has(`data.refs.${i}.repo`), p);
    assert.ok(scored("context-scored").has(`data.refs.${i}.repo`) && !unscored("context-scored").includes(p), p);
  }
  assert.ok(!unscored("base").includes('"prefix"'));
  for (const v of [0, 1] as const) assert.ok(JSON.stringify(spec("base", v).unscored).includes('{"prefix":["data","refs"]}'), `v${v}`);
});

test("v2 scores each refs slot as an item mention with identity (its repo, its number)", () => {
  const item = spec("base").types.find((t) => t.type === "item")!.mentions;
  const slots = item.filter((m) => m.path[1] === "refs");
  assert.equal(slots.length, 8);
  for (const [i, m] of slots.entries())
    assert.deepEqual(m, { path: ["data", "refs", i, "number"], identity: [["data", "refs", i, "repo"], ["data", "refs", i, "number"]] });
  for (const v of [0, 1] as const)
    assert.ok(!spec("base", v).types.flatMap((t) => t.mentions).some((m) => m.path[1] === "refs"), `v${v}`);
  assert.deepEqual([FORMAT[0], FORMAT[1], FORMAT[2]], [2, 3, 3]);
  assert.equal(LATEST, 2);
});

test("positive control: a PR naming lifeos#900 and s2w#900 joins each ref to its own repo's item", () => {
  const { mentions } = execute(spec("base"), frames);
  const opened = frames.findIndex((f) => f.record?.data.kind === "pr.opened");
  const ref = (i: number) => mentions.find((m) => m.frame === opened && m.path === `data.refs.${i}.number`)!.cluster;
  const s900 = mentions.find((m) => m.path === "data.key" && m.value === "s2w#900")!.cluster;
  const lifeos900 = mentions.find((m) => m.path === "data.number" && m.value === 900 && m.cluster !== s900)!.cluster;
  assert.deepEqual([ref(0), ref(1)], [lifeos900, s900], "refs sort by repo: lifeos first");
  // Both items grew by their ref mentions: v1's item sizes were 1, 5, 17 (lifeos#900, s2w#901,
  // s2w#900); the PR body adds one to each #900, the commits one to s2w#900 and one to s2w#901.
  const items = (v: 1 | 2) => execute(spec("base", v), frames).mentions.filter((m) => m.type === "item");
  const size = (v: 1 | 2, c: string) => items(v).filter((m) => m.cluster === c).length;
  assert.deepEqual([size(1, lifeos900), size(2, lifeos900)], [1, 2]);
  assert.deepEqual([size(1, s900), size(2, s900)], [17, 19]);
  // The same key with a ref keyed by its number alone merges the two #900 items: the false
  // merge the (repo, number) identity exists to prevent, now reached through `refs`.
  const byNumber = structuredClone(spec("base"));
  for (const m of byNumber.types.find((t) => t.type === "item")!.mentions)
    if (m.path[1] === "refs") m.identity = [m.path];
  const merged = execute(byNumber, frames).mentions.filter((m) => m.frame === opened && m.path.startsWith("data.refs."));
  assert.equal(new Set(merged.map((m) => m.cluster)).size, 1);
});

test("the fixture's entities: items keep their repo, aliases join, singletons stay apart", () => {
  const { mentions } = execute(spec("base"), frames);
  const entity = (path: string, value: unknown) => mentions.find((m) => m.path === path && m.value === value)?.cluster;
  const s900 = entity("data.key", "s2w#900");
  assert.ok(s900);
  const paths900 = new Set(mentions.filter((m) => m.cluster === s900).map((m) => m.path));
  assert.deepEqual([...paths900].sort(), ["data.issue", "data.key", "data.number", "data.refs.0.number", "data.refs.1.number"]);
  const lifeos900 = mentions.filter((m) => m.path === "data.number" && m.value === 900).map((m) => m.cluster);
  assert.equal(new Set(lifeos900).size, 2, "lifeos#900 and s2w#900 are two items");
  const s901 = entity("data.pr", 901);
  assert.deepEqual([...new Set(mentions.filter((m) => m.cluster === s901).map((m) => m.path))].sort(),
    ["data.number", "data.pr", "data.ref_number", "data.refs.0.number"]);
  const branch = mentions.filter((m) => m.type === "branch");
  assert.equal(new Set(branch.map((m) => m.cluster)).size, 1);
  assert.ok(branch.some((m) => m.path === "data.head_ref"));
  const sprint = mentions.filter((m) => m.type === "sprint");
  assert.equal(new Set(sprint.map((m) => m.cluster)).size, 1);
  assert.equal(sprint.length, 3);
  const seats = mentions.filter((m) => m.type === "seat");
  assert.equal(new Set(seats.map((m) => m.cluster)).size, seats.length);
});

test("--check passes on the fixture and its sidecar, counts only", () => {
  const r = check(header, frames, prov);
  assert.equal(r.failed, 0, JSON.stringify(r.failures));
  assert.deepEqual(Object.keys(r.failures), [...RULES]);
  assert.equal(r.frames, 20);
  assert.equal(r.key.mentions, 51); // v1's 47 plus 4 refs: 2 on the PR, 1 on each of two commits
  assert.equal(r.key.entities, 12); // every ref names an item already in the fixture
  assert.equal(r.edges_observed["reviews-commit data.seat_id -> data.head_sha"], 3);
  assert.equal(r.edges_observed["names data.number -> data.refs.1.number"], 1);
  assert.equal(r.edges_observed["commit-names data.sha -> data.refs.0.number"], 2);
  assert.equal(r.edges_observed["commit-names data.sha -> data.refs.1.number"], 0);
  assert.deepEqual([r.key.edges_per_type?.names, r.key.edges_per_type?.["commit-names"]], [2, 2]);
  // counts only: no value from the corpus is in the report
  const text = JSON.stringify(r);
  for (const v of ["aa2f4fa7", "feat/900-widget", "s2w#900", "2030-01-02", "maintainer"]) assert.ok(!text.includes(v), v);
});

function planted(i: number, edit: (p: Record<string, unknown>) => void): string {
  const lines = prov.trimEnd().split("\n");
  const p = JSON.parse(lines[i]);
  edit(p);
  lines[i] = JSON.stringify(p);
  return lines.join("\n") + "\n";
}

const seatAt = frames.findIndex((f) => f.record?.data.kind === "review.seat");

test("a sidecar whose resolved sha differs from the frame fails sha-resolved", () => {
  const r = check(header, frames, planted(seatAt, (p) => { p.sha_resolved = "0".repeat(40); }));
  assert.equal(r.failures["sha-resolved"], 1);
});

test("a seat with no issue, no branch PR and not marked unobservable fails seat-issue", () => {
  const f = structuredClone(frames);
  const i = f.findIndex((x) => x.record?.data.kind === "review.seat" && x.record.data.issue === null);
  const r = check(header, f, planted(i, (p) => { p.branch_pr = null; }));
  assert.equal(r.failures["seat-issue"], 1);
});

test("a misaligned sidecar, a short sha, a ref without a repo and a wrong leg key each fail", () => {
  assert.ok(check(header, frames, planted(0, (p) => { p.id = "x"; })).failures["sidecar-aligned"] >= 1);
  assert.ok(check(header, frames, prov.split("\n").slice(1).join("\n")).failures["sidecar-aligned"] >= 1);
  const f = structuredClone(frames);
  const at = (kind: string) => f.find((x) => x.record?.data.kind === kind)!.record!.data;
  at("commit").sha = "abc1234";
  f.find((x) => x.record?.data.ref_number != null)!.record!.data.ref_repo = null;
  at("leg.status").key = "lifeos#900";
  (at("pr.opened").refs as { repo: string }[])[0].repo = "other";
  const r = check(header, f, prov);
  assert.equal(r.failures["sha-shape"], 1);
  assert.equal(r.failures["ref-repo"], 1);
  assert.equal(r.failures["ref-repo-known"], 1);
  assert.equal(r.failures["leg-key"], 1);
});

test("every declared edge joins two mention paths of the base key, or names an unscored path", () => {
  for (const v of [1, 2] as const) {
    const scored = new Set(spec("base", v).types.flatMap((t) => t.mentions.map((m) => pathId(m.path))));
    for (const e of edgesOf(v)) {
      assert.ok(scored.has(pathId(e.from)), `v${v} ${e.label}: ${pathId(e.from)}`);
      if (e.observable) assert.ok(scored.has(pathId(e.to)), `v${v} ${e.label}: ${pathId(e.to)}`);
      else assert.ok(pathId(e.to).startsWith("data.refs"), e.label);
    }
  }
  assert.ok(EDGES.every((e) => e.observable), "v2 has no unobservable edge");
});

test("format 3 carries every edge as a row; format 2 carries none", () => {
  assert.equal(spec("base", 0).relationships, undefined);
  for (const v of [1, 2] as const) {
    const rows = spec("base", v).relationships ?? [];
    assert.equal(rows.length, edgesOf(v).length);
    for (const [i, e] of edgesOf(v).entries()) {
      assert.deepEqual([rows[i].type, rows[i].from, rows[i].to], [e.label, e.from, e.to]);
      assert.equal(rows[i].unobservable === undefined, e.observable, e.label);
    }
    assert.deepEqual(spec("context-scored", v).relationships, rows);
  }
  // v2 = v1's 14 observable rows, then 16 observable `names` rows in place of v1's 2 unobservable ones
  const v1 = spec("base", 1).relationships!, v2 = spec("base", 2).relationships!;
  assert.deepEqual(v1.filter((r) => r.unobservable !== undefined).map((r) => r.type), ["names", "names"]);
  assert.deepEqual(v2.slice(0, 14), v1.slice(0, 14));
  assert.equal(v2.length, 30);
  assert.deepEqual(v2.slice(14).map((r) => r.type), [...Array(8).fill("names"), ...Array(8).fill("commit-names")]);
  assert.ok(v2.slice(14).every((r) => r.unobservable === undefined));
  assert.equal(new Set(v2.map((r) => JSON.stringify([r.from, r.to]))).size, 30, "distinct (from, to) pairs");
});

test("a frame with no refs yields no ref mention and no names edge", () => {
  const f = (data: Record<string, unknown>) => ({ id: "1", record: { data } });
  const { mentions, edges } = execute(spec("base"), [
    f({ kind: "pr.opened", repo: "s2w", number: 5, refs: [] }),
    f({ kind: "commit", repo: "s2w", sha: "a".repeat(40), refs: [] }),
  ]);
  assert.ok(!mentions.some((m) => m.path.startsWith("data.refs.")));
  assert.ok(![...edges].some((e) => /^(commit-)?names\u001f/.test(e)));
});

test("a gold edge needs both mentions in one frame, and repeats count once", () => {
  const s = { ...spec("base"), relationships: [{ type: "leg-pr", from: ["data", "key"], to: ["data", "pr"] }] };
  const f = (data: Record<string, unknown>) => ({ id: "1", record: { data } });
  const leg = { repo: "lifeos", issue: 1, key: "lifeos#1", pr: 2 };
  const { edges } = execute(s, [f(leg), f(leg), f({ repo: "lifeos", key: "lifeos#1" }), f({ repo: "lifeos", pr: 3 })]);
  assert.deepEqual([...edges], ["leg-pr\u001fitem\u001f\"lifeos\"\u001f1\u001fitem\u001f\"lifeos\"\u001f2"]);
});

test("the sample is deterministic, stratified, and covers every path once N reaches the paths", () => {
  const { mentions } = execute(spec("base"), frames);
  const paths = new Set(mentions.map((m) => m.path));
  const a = draw(mentions, paths.size, 372);
  assert.deepEqual(new Set(a.map((m) => m.path)), paths);
  assert.deepEqual(draw(mentions, 30, 372), draw(mentions, 30, 372));
  assert.notDeepEqual(draw(mentions, 30, 372), draw(mentions, 30, 7));
  assert.equal(draw(mentions, 1000, 1).length, mentions.length);
  const w = worksheet(frames, prov, 5, 372);
  assert.ok(w.startsWith(": s2w-private-capture provenance=private "), "check 20 refuses a worksheet in the tree");
  assert.equal(w.match(/^lookup: /gm)?.length, 5);
});
