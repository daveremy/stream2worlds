// Captures the private stream (s2w#371) as an SSE capture file in the Wikipedia corpora's frame
// format, plus a provenance sidecar for the answer key (#372). Host: the hub (Node 22, sqlite3,
// gh, git). Plan and rulings: the s2w#371 issue comments.
//
// usage: node --experimental-strip-types research/h-measure/private/capture.ts \
//   --name private-test --since 2026-09-29T00:00:00Z --until 2026-10-01T00:00:00Z [--dir DIR] [--copy-dir DIR]
//
// Reads, read-only: the dev-worker status DB, the review-seat logs, the sprint log, `gh api` for
// both repositories, and the local clones (fetch them first: shas are resolved there). Writes
// <dir>/<name>.raw.sse and <dir>/<name>.provenance.jsonl (mode 0600, never over an existing
// file, never inside a git work tree), then prints the corpora.toml stanza. Every line it would
// write passes the scrub gate first (scrub.ts); one match and nothing is written.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { parseArgs } from "node:util";
import { EXTRACTION_TABLE, extractDetail } from "./extract.ts";
import { assertClean } from "./scrub.ts";
import {
  MARK, REPOS, TIMELINE_KINDS, commitEvents, itemEvents, legEvents, render, repoOfKey, seatEvents, sprintEvents,
  sprintRows, timelineEvents, window,
  type CommitRow, type Event, type GhItem, type Joins, type Repo, type SeatRow,
  type TimelineRow, type WorkerRow,
} from "./events.ts";

const H = homedir();
const { values: a } = parseArgs({
  options: {
    name: { type: "string" }, since: { type: "string" }, until: { type: "string" },
    dir: { type: "string", default: join(H, ".local/share/stream2worlds/h-measure") },
    "copy-dir": { type: "string" },
    db: { type: "string", default: join(H, ".dev-worker/dev-worker.db") },
    seats: { type: "string", default: join(H, "lifeos/logs/review-seats") },
    sprints: { type: "string", default: join(H, "lifeos/docs/sprints/README.md") },
    "clone-lifeos": { type: "string", default: join(H, "lifeos") },
    "clone-s2w": { type: "string", default: join(H, "code/stream2worlds") },
    "gh-lifeos": { type: "string", default: "daveremy/lifeos" },
    "gh-s2w": { type: "string", default: "daveremy/stream2worlds" },
    actor: { type: "string", default: "daveremy" },
    author: { type: "string", default: "Dave Remy" },
  },
});
const ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/;
if (!a.name || !/^[a-z0-9-]+$/.test(a.name) || !ISO.test(a.since ?? "") || !ISO.test(a.until ?? "")) {
  console.error("usage: capture.ts --name <slug> --since <YYYY-MM-DDTHH:MM:SSZ> --until <same> [--dir DIR] [--copy-dir DIR]");
  process.exit(2);
}
const since = a.since!, until = a.until!;
const clone: Record<Repo, string> = { lifeos: a["clone-lifeos"]!, s2w: a["clone-s2w"]! };
const ghRepo: Record<Repo, string> = { lifeos: a["gh-lifeos"]!, s2w: a["gh-s2w"]! };
const dropped = new Map<string, number>();
const drop = (why: string, n = 1) => dropped.set(why, (dropped.get(why) ?? 0) + n);
const run = (cmd: string, args: string[]) =>
  execFileSync(cmd, args, { encoding: "utf8", maxBuffer: 1 << 30, stdio: ["ignore", "pipe", "pipe"] });
const jsonl = <T>(text: string): T[] => text.split("\n").filter((l) => l.trim()).map((l) => JSON.parse(l) as T);

// ---- joins: sha resolution in the clones, public handle only ----
const shaCache = new Map<string, string | undefined>();
const joins: Joins = {
  sha(repo, s) {
    const k = `${repo}:${s}`;
    if (!shaCache.has(k)) {
      let full: string | undefined;
      try { full = run("git", ["-C", clone[repo], "rev-parse", "--verify", "--quiet", `${s}^{commit}`]).trim(); } catch { full = undefined; }
      shaCache.set(k, full && /^[0-9a-f]{40}$/.test(full) ? full : undefined);
    }
    return shaCache.get(k);
  },
  actor: (l) => (l === a.actor || l === a.author ? a.actor! : "other"),
};

// ---- dev-worker status DB (read-only URI) ----
const dbUri = `file:${a.db}?mode=ro`;
const sql = (q: string) => JSON.parse(run("sqlite3", ["-json", dbUri, q]) || "[]");
const workers: WorkerRow[] = sql(
  `select id, key, step, detail, timestamp from worker_history where timestamp >= '${since}' and timestamp < '${until}' order by id`);
for (const r of workers) if (!repoOfKey(r.key)) drop("worker-other-repo");
const maxWorkerId = sql("select max(id) as m from worker_history")[0]?.m ?? 0;

// ---- review-seat logs: one file per UTC day. Repo: the seat's cwd (then dropped), else its
// branch, matched to a PR head ref or a leg's branch in exactly one repo (resolved below) ----
const rawSeats: { offset: number; r: any }[] = [];
const dayLo = since.slice(0, 10), dayHi = until.slice(0, 10);
for (const f of readdirSync(a.seats!).filter((f) => /^\d{4}-\d{2}-\d{2}\.jsonl$/.test(f)).sort()) {
  const day = f.slice(0, 10);
  if (day < dayLo || day > dayHi) continue;
  readFileSync(join(a.seats!, f), "utf8").split("\n").forEach((line, i) => {
    if (line.trim()) rawSeats.push({ offset: Number(day.replaceAll("-", "")) * 100000 + i + 1, r: JSON.parse(line) });
  });
}

// ---- GitHub: issues and PRs updated since `since`, their timelines ----
const items: GhItem[] = [];
const timeline: TimelineRow[] = [];
for (const repo of REPOS) {
  const R = ghRepo[repo];
  const pulls = new Map<number, any>();
  for (const p of jsonl<any>(run("gh", ["api", "--paginate", `repos/${R}/pulls?state=all&per_page=100`, "--jq", ".[]"])))
    pulls.set(p.number, p);
  const issues = jsonl<any>(run("gh", ["api", "--paginate", `repos/${R}/issues?state=all&since=${since}&per_page=100`, "--jq", ".[]"]));
  for (const it of issues) {
    const p = it.pull_request ? pulls.get(it.number) : undefined;
    const rows = jsonl<any>(run("gh", ["api", "--paginate", `repos/${R}/issues/${it.number}/timeline?per_page=100`, "--jq", ".[]"]));
    let closedBy: string | null = null;
    rows.forEach((t, ordinal) => {
      if (!t.created_at) return drop("timeline-undated");
      if (t.event === "closed" || t.event === "merged") closedBy = t.actor?.login ?? null;
      const src = t.source?.issue;
      const sameRepo = src?.repository_url?.endsWith(`/repos/${R}`);
      timeline.push({
        repo, number: it.number, ordinal, id: typeof t.id === "number" ? t.id : null, event: t.event,
        created_at: t.created_at, actor: t.actor?.login ?? t.user?.login ?? null,
        label: t.label?.name ?? null, ref_number: src && sameRepo ? src.number : null,
        commit_sha: t.event === "referenced" ? (t.commit_id ?? null) : null,
        comment_bytes: t.event === "commented" ? Buffer.byteLength(t.body ?? "") : null,
      });
    });
    items.push({
      repo, id: it.id, number: it.number, is_pr: Boolean(p), created_at: it.created_at,
      actor: it.user?.login ?? "", labels: (it.labels ?? []).map((l: any) => l.name), body: p ? p.body : null,
      head_sha: p?.head?.sha, head_ref: p?.head?.ref, base_ref: p?.base?.ref, merged_at: p?.merged_at ?? null,
      closed_at: p ? p.closed_at : null, merge_commit_sha: p?.merge_commit_sha ?? null, closed_by: closedBy,
    });
  }
}
const branchPr = new Map<string, number>();
for (const it of items) if (it.is_pr && it.head_ref) branchPr.set(`${it.repo}:${it.head_ref}`, it.number);

const legBranchRepos = new Map<string, Set<Repo>>();
for (const w of workers) {
  const repo = repoOfKey(w.key), br = extractDetail(w.detail).branch;
  if (repo && br) legBranchRepos.set(br, (legBranchRepos.get(br) ?? new Set()).add(repo));
}
const seatRows: SeatRow[] = [];
for (const { offset, r } of rawSeats) {
  const cwd = String(r.cwd ?? "");
  let repo: Repo | undefined = /stream2worlds/.test(cwd) ? "s2w" : /lifeos/.test(cwd) ? "lifeos" : undefined;
  if (!repo && !cwd && r.branch) {
    const hits = new Set<Repo>(legBranchRepos.get(r.branch) ?? []);
    for (const x of REPOS) if (branchPr.has(`${x}:${r.branch}`)) hits.add(x);
    if (hits.size === 1) repo = [...hits][0];
  }
  if (!repo) { drop(cwd ? "seat-other-repo" : "seat-repo-unknown"); continue; }
  const issue = typeof r.issue === "number" ? r.issue : /^\d+$/.test(String(r.issue ?? "")) ? Number(r.issue) : null;
  seatRows.push({
    offset, repo, ts_start: r.ts_start, ts_end: r.ts_end ?? null, engine: r.engine ?? null, model: r.model ?? null,
    tier: r.tier ?? null, verdict: r.verdict ?? null, issue, branch: r.branch ?? null, head_sha: r.head_sha ?? null,
    seat_id: r.seat_id ?? null, script: r.script ?? null,
  });
}

// ---- git: default-branch commits in the span, then every sha another event names ----
const FMT = "%H%x1f%P%x1f%an%x1f%cI%x1f%s%x1f%(trailers:key=Co-authored-by,valueonly,separator=%x2C)%x1e";
const parseLog = (repo: Repo, out: string): CommitRow[] => out.split("\x1e").map((s) => s.replace(/^\n/, "")).filter(Boolean).map((s) => {
  const [sha, parents, author, committed, subject, co] = s.split("\x1f");
  return { repo, sha, parents: parents ? parents.split(" ") : [], author, committed, subject, co_authored: Boolean(co?.trim()) };
});
const commits = new Map<string, CommitRow>();
for (const repo of REPOS) {
  const head = run("git", ["-C", clone[repo], "rev-parse", "--abbrev-ref", "origin/HEAD"]).trim();
  for (const c of parseLog(repo, run("git", ["-C", clone[repo], "log", head, `--since=${since}`, `--until=${until}`, `--format=${FMT}`])))
    commits.set(c.sha, c);
}

const events: Event[] = [
  ...legEvents(workers, joins), ...seatEvents(seatRows, joins, branchPr), ...itemEvents(items, joins),
  ...timelineEvents(timeline, joins), ...sprintEvents(sprintRows(readFileSync(a.sprints!, "utf8"))),
];
for (const r of timeline) if (!TIMELINE_KINDS.has(r.event)) drop("timeline-other-kind");
const inSpan = window(events, since, until);
const named = new Set<string>();
for (const e of inSpan) for (const k of ["sha", "head_sha", "merge_commit_sha", "commit_sha"]) {
  const v = e.data[k];
  if (typeof v === "string" && /^[0-9a-f]{40}$/.test(v)) named.add(`${e.data.repo}:${v}`);
}
for (const k of named) {
  const [repo, sha] = k.split(":") as [Repo, string];
  if (commits.has(sha)) continue;
  try { for (const c of parseLog(repo, run("git", ["-C", clone[repo], "show", "-s", `--format=${FMT}`, sha]))) commits.set(c.sha, c); }
  catch { drop("named-sha-not-in-clone"); }
}
const commitEv = commitEvents([...commits.values()], joins);
const isNamed = (e: Event) => e.topic === "commits" && named.has(`${e.data.repo}:${e.data.sha}`);
const kept = window([...inSpan, ...commitEv], since, until, isNamed);
const total = events.length + commitEv.length;
if (kept.length < total) drop("outside-span", total - kept.length);

const counts = [...dropped].sort().map(([k, v]) => `${k}:${v}`).join(",") || "none";
const { sse, provenance } = render([
  `${MARK}private captured ${new Date().toISOString()} via research/h-measure/private/capture.ts`,
  `capture.ts --name ${a.name} --since ${since} --until ${until} --dir <dir>`,
  `events=${kept.length} dropped=${counts} max_worker_row=${maxWorkerId} extraction: ${EXTRACTION_TABLE}`,
], kept);
assertClean(sse);
assertClean(provenance);

for (const dir of [a.dir!, ...(a["copy-dir"] ? [a["copy-dir"]] : [])]) {
  mkdirSync(dir, { recursive: true, mode: 0o700 });
  let inRepo = true;
  try { run("git", ["-C", dir, "rev-parse", "--is-inside-work-tree"]); } catch { inRepo = false; }
  if (inRepo) throw new Error("refusing to write a private capture inside a git work tree");
  for (const [ext, body] of [["raw.sse", sse], ["provenance.jsonl", provenance]] as const) {
    const p = join(dir, `${a.name}.${ext}`);
    if (existsSync(p)) throw new Error(`refusing to overwrite ${a.name}.${ext}`);
    writeFileSync(p, body, { mode: 0o600, flag: "wx" });
  }
}
const sha = (s: string) => createHash("sha256").update(s).digest("hex");
process.stdout.write(`\n[corpus.${a.name}]\nrole = "<development|reserved>"\nfile = "${a.name}.raw.sse"\nstream = "private"\n` +
  `since = "${since}"\nuntil = "${until}"\nevents = ${kept.length}\nbytes = ${Buffer.byteLength(sse)}\nsha256 = "${sha(sse)}"\n` +
  `provenance_sha256 = "${sha(provenance)}"\n# dropped: ${counts}\n`);
