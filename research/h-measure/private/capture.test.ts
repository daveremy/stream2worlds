// Tests for the private-stream capture (s2w#371). Run from the repository root:
//   node --experimental-strip-types --test research/h-measure/private/capture.test.ts
// Planted secrets are assembled at run time so this file holds no secret-shaped literal.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { EXTRACTION_TABLE, extractDetail, issueRefs, ptsFromLabels } from "./extract.ts";
import { MARK, legEvents, render, sprintRows, timelineEvents, window, type Event, type Joins } from "./events.ts";
import { FIXTURE_PATH, synthetic } from "./fixture.ts";
import { RULES, assertClean, scrubViolations } from "./scrub.ts";

const r = (s: string, n: number) => s.repeat(n);
const PLANTED: Record<string, string> = {
  "home-path": "branch /home/someone/x",
  tilde: "cwd ~/code",
  obsidian: "vault Obsidian/daily",
  "op-ref": "op" + "://Vault/Item/field",
  email: "someone" + "@" + "example.org",
  phone: "call 206-555-0142",
  "aws-access-key-id": "AK" + "IA" + r("A", 16),
  "github-pat": "gh" + "p_" + r("a", 36),
  "github-fine-grained-pat": "github" + "_pat_" + r("a", 60),
  "anthropic-api-key": "sk" + "-ant-" + r("a", 24),
  "openai-api-key": "sk" + "-" + r("a", 32),
  "slack-token": "xo" + "xb-" + r("1", 10),
  "gitlab-pat": "gl" + "pat-" + r("a", 20),
  "google-api-key": "AI" + "za" + r("a", 35),
  "stripe-key": "sk" + "_live_" + r("a", 20),
  "npm-token": "np" + "m_" + r("a", 36),
  "telegram-bot-token": "123456789" + ":AA" + r("a", 33),
  jwt: "ey" + "J" + r("a", 10) + ".ey" + "J" + r("a", 10) + "." + r("a", 10),
  "posthog-personal-api-key": "ph" + "x_" + r("a", 32),
  "private-key": "-----BEGIN " + "PRIVATE " + "KEY-----",
};

test("every scrub rule has a planted sample, and each sample is refused by its own rule", () => {
  assert.deepEqual(Object.keys(PLANTED).sort(), RULES.map((x) => x.name).sort());
  for (const [rule, sample] of Object.entries(PLANTED)) {
    const v = scrubViolations(`data: {"x":1}\ndata: {"v":${JSON.stringify(sample)}}`);
    assert.ok(v.some((x) => x.rule === rule && x.line === 2), `${rule} did not fire`);
    assert.throws(() => assertClean(sample), (e: Error) => e.message.includes(rule) && !e.message.includes(sample));
  }
});

test("the scrub gate passes the values a capture is made of", () => {
  const clean = [
    'id: [{"topic":"timeline","partition":0,"offset":3354466123}]',
    'data: {"kind":"commit","ts":"2026-09-29T12:06:03.123Z","sha":"0123456789abcdef0123456789abcdef01234567"}',
    'data: {"branch":"feat/371-private-capture","engines":"P:- I:o R:f+grok","labels":["pts:3"]}',
    `: ${MARK}private captured 2026-10-01T00:00:00.000Z via research/h-measure/private/capture.ts`,
    `: events=10 dropped=none extraction: ${EXTRACTION_TABLE}`,
  ].join("\n");
  assert.deepEqual(scrubViolations(clean), []);
});

test("extraction table: only the named fields leave detail", () => {
  assert.deepEqual(extractDetail("leg A done: feat/371-x@abc1234, PR #12 round 2 BLOCK P:- I:o R:f+grok"), {
    pr: 12, sha: "abc1234", branch: "feat/371-x", leg: "A", round: 2, verdict: "BLOCK", engines: "P:- I:o R:f+grok",
  });
  assert.deepEqual(extractDetail("reproducing; hypothesis: something about a path"), {});
  assert.deepEqual(extractDetail(null), {});
  assert.deepEqual(extractDetail("worktree on fix/9-y."), { branch: "fix/9-y" });
  assert.deepEqual(issueRefs("Part of #371. Closes #12, see lifeos#5 and a/#7 and #12"), [12, 371]);
  assert.equal(ptsFromLabels(["P1", "pts:3"]), 3);
  assert.equal(ptsFromLabels(["P1"]), undefined);
  // `engines` takes only engine-code characters, so a path in that slot is not extracted.
  assert.deepEqual(extractDetail("P:- I:o R:/var/x"), {});
});

test("sprint rows: the ran time wins over the slot hour; local time is MST", () => {
  const md = [
    "| Sprint | Slot | Name |",
    "| 1 | 2026-09-20-10a | x | [file](2026-09-20-1-a.md) |",
    "| 58 | 2026-09-27-9a (ran 08:54) | x | [file](2026-09-27-58-b.md) |",
    "| 60 | 2026-09-27-1p | x | — |",
  ].join("\n");
  assert.deepEqual(sprintRows(md), [
    { sprint: 1, slot: "2026-09-20-10a", ts: "2026-09-20T10:00:00-07:00", file_id: "2026-09-20-1" },
    { sprint: 58, slot: "2026-09-27-9a", ts: "2026-09-27T08:54:00-07:00", file_id: "2026-09-27-58" },
    { sprint: 60, slot: "2026-09-27-1p", ts: "2026-09-27T13:00:00-07:00", file_id: null },
  ]);
});

test("window: until is a hard bound; only keepEarly events may predate since", () => {
  const ev = (topic: string, ts: string, offset: number): Event =>
    ({ topic, offset, provenance: {}, data: { kind: "k", repo: "s2w", ts } });
  const es = [ev("b", "2030-01-01T02:00:00.000Z", 2), ev("a", "2030-01-01T02:00:00.000Z", 10),
    ev("a", "2030-01-01T02:00:00.000Z", 9), ev("c", "2030-01-01T00:00:00.000Z", 1), ev("d", "2030-01-01T03:00:00.000Z", 1)];
  const lo = "2030-01-01T01:00:00Z", hi = "2030-01-01T03:00:00Z";
  assert.deepEqual(window(es, lo, hi).map((e) => `${e.topic}${e.offset}`), ["a9", "a10", "b2"]);
  assert.deepEqual(window(es, lo, hi, (e) => e.topic !== "a").map((e) => `${e.topic}${e.offset}`), ["c1", "a9", "a10", "b2"]);
});

test("render: header lines, one frame and one provenance line per event", () => {
  const e: Event = { topic: "legs", offset: 7, provenance: { row_id: 7 }, data: { kind: "leg.status", repo: "s2w", ts: "x" } };
  const { sse, provenance } = render(["a", "b", "c"], [e, e]);
  assert.ok(sse.startsWith(": a\n: b\n: c\n\nevent: message\nid: "));
  assert.equal(sse.match(/^data: /gm)?.length, 2);
  assert.equal(provenance.trim().split("\n").length, 2);
  assert.throws(() => render(["a\nb", "", ""], []));
});

test("the committed synthetic fixture is exactly what fixture.ts generates", () => {
  const committed = readFileSync(FIXTURE_PATH, "utf8");
  assert.equal(committed, synthetic(), "regenerate with: node --experimental-strip-types research/h-measure/private/fixture.ts --write");
  assert.equal(committed.match(/^data: /gm)?.length, 20);
  assert.ok(committed.startsWith(`: ${MARK}synthetic `));
  assert.deepEqual(scrubViolations(committed), []);
});

test("pts only on labeled; a step that is not a board verb becomes null", () => {
  const j: Joins = { sha: () => undefined, actor: () => "other" };
  const row = { repo: "s2w" as const, number: 9, ordinal: 1, id: 1, created_at: "2026-01-01T00:00:00Z", actor: "x", label: "pts:3" };
  const [on, off] = timelineEvents([{ ...row, event: "labeled" }, { ...row, ordinal: 2, id: 2, event: "unlabeled" }], j);
  assert.equal(on.data.pts, 3);
  assert.equal("pts" in off.data, false);
  const leg = (step: string) => legEvents([{ id: 1, key: "s2w#9", step, detail: null, timestamp: "2026-01-01T00:00:00.000Z" }], j)[0].data.step;
  assert.equal(leg("review-code"), "review-code");
  assert.equal(leg("see /var/x"), null);
});
