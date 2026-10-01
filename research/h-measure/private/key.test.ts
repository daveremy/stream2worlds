// Tests for the private-stream answer key (s2w#372). Run from the repository root:
//   node --experimental-strip-types --test research/h-measure/private/key.test.ts
// Synthetic fixture only: no real capture is ever read here.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { FIXTURE_PATH, FIXTURE_PROVENANCE_PATH, syntheticCapture } from "./fixture.ts";
import {
  BASE_FILE, CONTEXT_FILE, EDGES, KEY_DIR, RULES, SHAPE_PATH, check, draw, execute, fixtureShape, parseSse,
  pathId, render, spec, worksheet,
} from "./key.ts";

const REGEN = "regenerate with: node --experimental-strip-types research/h-measure/private/key.ts --write";
const sse = readFileSync(FIXTURE_PATH, "utf8");
const prov = readFileSync(FIXTURE_PROVENANCE_PATH, "utf8");
const { header, frames } = parseSse(sse);

test("the committed key files and fixture shape are exactly what key.ts generates", () => {
  assert.equal(readFileSync(join(KEY_DIR, BASE_FILE), "utf8"), render(spec("base")), REGEN);
  assert.equal(readFileSync(join(KEY_DIR, CONTEXT_FILE), "utf8"), render(spec("context-scored")), REGEN);
  assert.equal(readFileSync(SHAPE_PATH, "utf8"), fixtureShape(), REGEN);
});

test("the committed fixture sidecar is exactly what fixture.ts generates", () => {
  assert.equal(prov, syntheticCapture().provenance, "regenerate with: node --experimental-strip-types research/h-measure/private/fixture.ts --write");
});

// Names only: the sha256 pin itself is checked by xtask's private_key_tests (Pins::key).
test("the keys are pinned in keys.toml", () => {
  const toml = readFileSync(join(KEY_DIR, "keys.toml"), "utf8");
  for (const f of [BASE_FILE, CONTEXT_FILE]) assert.ok(toml.includes(`file = "${f}"`), f);
});

test("repo and actor are unscored in the base key and scored in the variant (ruling 4)", () => {
  const scored = (v: "base" | "context-scored") => new Set(spec(v).types.flatMap((t) => t.mentions.map((m) => pathId(m.path))));
  const unscored = (v: "base" | "context-scored") => JSON.stringify(spec(v).unscored);
  for (const p of ["data.repo", "data.ref_repo", "data.actor", "data.author"]) {
    assert.ok(!scored("base").has(p) && unscored("base").includes(`"${p.split(".")[1]}"`), p);
    assert.ok(scored("context-scored").has(p) && !unscored("context-scored").includes(`"${p.split(".")[1]}"`), p);
  }
  assert.ok(unscored("base").includes('{"prefix":["data","refs"]}'), "refs is unscored (ruling 2)");
});

test("the fixture's entities: items keep their repo, aliases join, singletons stay apart", () => {
  const { mentions } = execute(spec("base"), frames);
  const entity = (path: string, value: unknown) => mentions.find((m) => m.path === path && m.value === value)?.cluster;
  const s900 = entity("data.key", "s2w#900");
  assert.ok(s900);
  const paths900 = new Set(mentions.filter((m) => m.cluster === s900).map((m) => m.path));
  assert.deepEqual([...paths900].sort(), ["data.issue", "data.key", "data.number"]);
  const lifeos900 = mentions.filter((m) => m.path === "data.number" && m.value === 900).map((m) => m.cluster);
  assert.equal(new Set(lifeos900).size, 2, "lifeos#900 and s2w#900 are two items");
  const s901 = entity("data.pr", 901);
  assert.deepEqual([...new Set(mentions.filter((m) => m.cluster === s901).map((m) => m.path))].sort(),
    ["data.number", "data.pr", "data.ref_number"]);
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
  assert.equal(r.key.mentions, 47);
  assert.equal(r.key.entities, 12);
  assert.equal(r.edges_observed["reviews-commit data.seat_id -> data.head_sha"], 3);
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
  const r = check(header, f, prov);
  assert.equal(r.failures["sha-shape"], 1);
  assert.equal(r.failures["ref-repo"], 1);
  assert.equal(r.failures["leg-key"], 1);
});

test("every declared edge joins two mention paths of the base key, or names an unscored path", () => {
  const scored = new Set(spec("base").types.flatMap((t) => t.mentions.map((m) => pathId(m.path))));
  for (const e of EDGES) {
    assert.ok(scored.has(pathId(e.from)), `${e.label}: ${pathId(e.from)}`);
    if (e.observable) assert.ok(scored.has(pathId(e.to)), `${e.label}: ${pathId(e.to)}`);
    else assert.ok(pathId(e.to).startsWith("data.refs"), e.label);
  }
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
