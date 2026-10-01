// Tests for the private-stream capture (s2w#371). Run from the repository root:
//   node --experimental-strip-types --test research/h-measure/private/capture.test.ts
// Planted secrets are assembled at run time so this file holds no secret-shaped literal.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { EXTRACTION_TABLE, extractDetail, issueRefs, ptsFromLabels } from "./extract.ts";
import {
  MARK, MAX_REFS, commitEvents, itemEvents, legEvents, render, sprintRows, timelineEvents, window, type Event, type Joins,
} from "./events.ts";
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
  assert.equal(ptsFromLabels(["P1", "pts:3"]), 3);
  assert.equal(ptsFromLabels(["P1"]), undefined);
  // `engines` takes only engine-code characters, so a path in that slot is not extracted.
  assert.deepEqual(extractDetail("P:- I:o R:/var/x"), {});
});

test("refs carry their repo: bare = own repo, known prefixes resolve, any other prefix is counted", () => {
  const s2w = (number: number) => ({ repo: "s2w", number });
  const lifeos = (number: number) => ({ repo: "lifeos", number });
  assert.deepEqual(issueRefs("Part of #371. Closes #12, see lifeos#5 and a/#7 and #12", "s2w"),
    { refs: [lifeos(5), s2w(12), s2w(371)], dropped: 1 });
  // `s2w#12`, `daveremy/stream2worlds#12` and a bare `#12` in an s2w event are one ref.
  assert.deepEqual(issueRefs("#12 s2w#12 daveremy/stream2worlds#12", "s2w"), { refs: [s2w(12)], dropped: 0 });
  // The same text in a lifeos event: the bare ref is lifeos's, the prefixed one stays s2w's.
  assert.deepEqual(issueRefs("#12 s2w#12", "lifeos"), { refs: [lifeos(12), s2w(12)], dropped: 0 });
  assert.deepEqual(issueRefs("daveremy/lifeos#5 (lifeos#6)", "s2w"), { refs: [lifeos(5), lifeos(6)], dropped: 0 });
  assert.deepEqual(issueRefs("foo_lifeos#5 other/repo#3 Lifeos#4", "s2w"), { refs: [], dropped: 3 });
  // No `#`, no ref: a bare number or a word-glued `#` is not a reference.
  assert.deepEqual(issueRefs("step 12 of v2#beta and x#y", "s2w"), { refs: [], dropped: 0 });
  assert.deepEqual(issueRefs(null, "s2w"), { refs: [], dropped: 0 });
  assert.match(EXTRACTION_TABLE, / refs=\S+#/);
});

test("refs past MAX_REFS are cut and counted; unknown prefixes are counted as ref-other-repo", () => {
  const drops = new Map<string, number>();
  const j: Joins = { sha: () => undefined, actor: () => "other", drop: (why, n = 1) => drops.set(why, (drops.get(why) ?? 0) + n) };
  const body = Array.from({ length: MAX_REFS + 1 }, (_, i) => `#${i + 1}`).join(" ") + " a/#7";
  const [opened] = itemEvents([{
    repo: "s2w", id: 1, number: 99, is_pr: true, created_at: "2026-01-01T00:00:00Z", actor: "x", body,
  }], j);
  const refs = opened.data.refs as { repo: string; number: number }[];
  assert.equal(refs.length, MAX_REFS);
  assert.deepEqual(refs.at(-1), { repo: "s2w", number: MAX_REFS });
  const [commit] = commitEvents([{
    repo: "lifeos", sha: "a".repeat(40), parents: [], author: "x", committed: "2026-01-01T00:00:00Z",
    subject: "fix (#3), s2w#4, x/y#5", co_authored: false,
  }], j);
  assert.deepEqual(commit.data.refs, [{ repo: "lifeos", number: 3 }, { repo: "s2w", number: 4 }]);
  assert.deepEqual([...drops].sort(), [["ref-other-repo", 2], ["refs-overflow", 1]]);
});

test("cross-repo positive control: the fixture PR's refs name lifeos#900 and s2w#900 apart", () => {
  const lines = readFileSync(FIXTURE_PATH, "utf8").split("\n");
  const frames = lines.filter((l) => l.startsWith("data: ")).map((l) => JSON.parse(l.slice(6)));
  const pr = frames.find((d) => d.kind === "pr.opened");
  assert.deepEqual(pr.refs, [{ repo: "lifeos", number: 900 }, { repo: "s2w", number: 900 }]);
  // Each ref resolves to a distinct item in the fixture: the same number, two repos, two issues.
  for (const ref of pr.refs) {
    assert.equal(frames.filter((d) => d.kind === "issue.opened" && d.repo === ref.repo && d.number === ref.number).length, 1,
      `${ref.repo}#${ref.number}`);
  }
  const dropped = lines.find((l) => l.startsWith(": events="));
  assert.match(dropped ?? "", /dropped=\S*\bref-other-repo:1\b/);
});

test("sprint rows: every slot-cell form parses; the ran time wins over the slot hour; MST", () => {
  const md = [
    "| Sprint | Slot | Name |",
    "| 1 | 2026-09-20-10a | x | [file](2026-09-20-1-a.md) |",
    "| 58 | 2026-09-27-9a (ran 08:54) | x | [file](2026-09-27-58-b.md) |",
    "| 60 | 2026-09-27-1p | x | — |",
    // `(ran <h>a|p)`, sprint 19's form.
    "| 19 | 2026-09-22-1a (ran 7a) | x | [file](2026-09-22-19-c.md) |",
    // `(ran HH:MM; …)`, sprint 48's form.
    "| 48 | 2026-09-25-9p (ran 21:25; stalled 21:30→22:10 on a permission prompt) | x | — |",
    // `(slot HH:MM–HH:MM)`, sprints 63 onward: the start is the slot cell's `HH:MM–` (s2w#405).
    "| 63 | 2026-09-29-9a (slot 09:00–11:00) | x | [file](2026-09-29-63-d.md) |",
    "| 70 | 2026-09-30-3p (slot 15:00–17:00; early start) | x | — |",
    // Sprint 72's real row: an early start, 24 minutes before the 5p slot hour.
    "| 72 | 2026-09-28-5p (slot 16:36–19:00; early start) | x | — |",
    // Sprint 66's real row: a late start.
    "| 66 | 2026-09-28-5a (slot 05:30–07:00; late start from the S65 stall) | x | — |",
    // `ran` outranks `slot`; a parenthetical that is neither keeps the slot hour.
    "| 90 | 2026-09-30-5a (ran 05:10; slot 05:00–07:00) | x | — |",
    "| 91 | 2026-09-30-7a (stalled, slot 06:40–09:00) | x | — |",
  ].join("\n");
  assert.deepEqual(sprintRows(md), { unparsed: 0, rows: [
    { sprint: 1, slot: "2026-09-20-10a", ts: "2026-09-20T10:00:00-07:00", file_id: "2026-09-20-1" },
    { sprint: 58, slot: "2026-09-27-9a", ts: "2026-09-27T08:54:00-07:00", file_id: "2026-09-27-58" },
    { sprint: 60, slot: "2026-09-27-1p", ts: "2026-09-27T13:00:00-07:00", file_id: null },
    { sprint: 19, slot: "2026-09-22-1a", ts: "2026-09-22T07:00:00-07:00", file_id: "2026-09-22-19" },
    { sprint: 48, slot: "2026-09-25-9p", ts: "2026-09-25T21:25:00-07:00", file_id: null },
    { sprint: 63, slot: "2026-09-29-9a", ts: "2026-09-29T09:00:00-07:00", file_id: "2026-09-29-63" },
    { sprint: 70, slot: "2026-09-30-3p", ts: "2026-09-30T15:00:00-07:00", file_id: null },
    { sprint: 72, slot: "2026-09-28-5p", ts: "2026-09-28T16:36:00-07:00", file_id: null },
    { sprint: 66, slot: "2026-09-28-5a", ts: "2026-09-28T05:30:00-07:00", file_id: null },
    { sprint: 90, slot: "2026-09-30-5a", ts: "2026-09-30T05:10:00-07:00", file_id: null },
    { sprint: 91, slot: "2026-09-30-7a", ts: "2026-09-30T07:00:00-07:00", file_id: null },
  ] });
});

test("sprint rows: a sprint-shaped row that fails the pattern is counted, not dropped silently", () => {
  const md = [
    "| Sprint | Slot | Name |",
    "| 1 | 2026-09-20-10a | x | — |",
    "| 2 | 2026-09-20 10:00 | x | — |",
    "| 3 | 2026-09-20-10a(ran 10:05) | x | — |",
    "| not | a sprint row |",
  ].join("\n");
  const { rows, unparsed } = sprintRows(md);
  assert.deepEqual(rows.map((r) => r.sprint), [1]);
  assert.equal(unparsed, 2);
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
  const j: Joins = { sha: () => undefined, actor: () => "other", drop: () => {} };
  const row = { repo: "s2w" as const, number: 9, ordinal: 1, id: 1, created_at: "2026-01-01T00:00:00Z", actor: "x", label: "pts:3" };
  const [on, off] = timelineEvents([{ ...row, event: "labeled" }, { ...row, ordinal: 2, id: 2, event: "unlabeled" }], j);
  assert.equal(on.data.pts, 3);
  assert.equal("pts" in off.data, false);
  const leg = (step: string) => legEvents([{ id: 1, key: "s2w#9", step, detail: null, timestamp: "2026-01-01T00:00:00.000Z" }], j)[0].data.step;
  assert.equal(leg("review-code"), "review-code");
  assert.equal(leg("see /var/x"), null);
});
