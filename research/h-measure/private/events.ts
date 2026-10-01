// Pure event builders and SSE rendering for the private-stream capture (s2w#371, plan 2-5).
//
// Readers (capture.ts) turn the sources into the row types below; this module turns rows into
// events, the provenance sidecar and the SSE text. No I/O, so the synthetic fixture and the
// tests run the same code as a real capture.

import { extractDetail, issueRefs, ptsFromLabels } from "./extract.ts";

/** Header line 1 starts with this plus `private` (a capture) or `synthetic` (the fixture);
 * `cargo xtask check` refuses any repository file with a line starting `: ` + MARK + `private`. */
export const MARK = "s2w-private-capture provenance=";

export type Repo = "lifeos" | "s2w";
export const REPOS: readonly Repo[] = ["lifeos", "s2w"];

/** A worker step is a status-board verb (`review-code`); anything else is not copied. */
const STEP = /^[a-z][a-z0-9-]{0,31}$/;
export interface WorkerRow { id: number; key: string; step: string; detail: string | null; timestamp: string }
export interface SeatRow {
  offset: number; // YYYYMMDD * 100000 + line number in that day's log
  repo: Repo;
  ts_start: string; ts_end: string | null;
  engine: string | null; model: string | null; tier: string | null; verdict: string | null;
  issue: number | null; branch: string | null; head_sha: string | null;
  seat_id: string | null; script: string | null;
}
export interface GhItem {
  repo: Repo; id: number; number: number; is_pr: boolean; created_at: string; actor: string;
  body: string | null;
  head_sha?: string; head_ref?: string; base_ref?: string;
  merged_at?: string | null; closed_at?: string | null; merge_commit_sha?: string | null;
  closed_by?: string | null;
}
export interface TimelineRow {
  repo: Repo; number: number; ordinal: number; id: number | null; event: string; created_at: string;
  actor: string; label?: string | null; ref_repo?: Repo | null; ref_number?: number | null; commit_sha?: string | null;
  comment_bytes?: number | null;
}
export interface CommitRow {
  repo: Repo; sha: string; parents: string[]; author: string; committed: string; subject: string;
  co_authored: boolean;
}
export interface SprintRow { sprint: number; slot: string; ts: string; file_id: string | null }

export interface Event {
  topic: string;
  offset: number | string;
  data: Record<string, unknown> & { kind: string; repo: Repo; ts: string };
  provenance: Record<string, unknown>;
}

export interface Joins {
  /** Short or full sha -> 40-char sha, or undefined when the clone does not have it. */
  sha(repo: Repo, sha: string): string | undefined;
  /** Login -> the public handle kept in the capture ("other" for everyone else). */
  actor(login: string | null | undefined): string;
}

export const TIMELINE_KINDS: ReadonlySet<string> = new Set([
  "labeled", "unlabeled", "closed", "reopened", "cross-referenced", "referenced", "commented", "milestoned",
]);

function iso(ts: string): string {
  const t = new Date(ts);
  if (Number.isNaN(t.getTime())) throw new Error(`unparseable timestamp in a source row`);
  return t.toISOString();
}

export function repoOfKey(key: string): Repo | undefined {
  const m = /^(lifeos|s2w)#\d+$/.exec(key);
  return m ? (m[1] as Repo) : undefined;
}

export function legEvents(rows: WorkerRow[], j: Joins): Event[] {
  const out: Event[] = [];
  for (const r of rows) {
    const repo = repoOfKey(r.key);
    if (!repo) continue;
    const f = extractDetail(r.detail);
    const prov: Record<string, unknown> = { source: "worker_history", row_id: r.id };
    let sha: string | undefined;
    if (f.sha) {
      sha = j.sha(repo, f.sha);
      prov.sha_written = f.sha;
      prov.sha_resolved = sha ?? null;
    }
    const { sha: _short, ...rest } = f;
    out.push({
      topic: "legs", offset: r.id, provenance: prov,
      data: {
        kind: "leg.status", repo, ts: iso(r.timestamp), row_id: r.id, key: r.key,
        issue: Number(r.key.split("#")[1]), step: STEP.test(r.step) ? r.step : null, ...rest, ...(sha ? { sha } : {}),
      },
    });
  }
  return out;
}

export function seatEvents(rows: SeatRow[], j: Joins, branchPr: Map<string, number>): Event[] {
  return rows.map((r) => {
    const sha = r.head_sha ? j.sha(r.repo, r.head_sha) : undefined;
    const pr = r.branch ? branchPr.get(`${r.repo}:${r.branch}`) : undefined;
    const prov: Record<string, unknown> = {
      source: "review_seats", offset: r.offset, sha_written: r.head_sha, sha_resolved: sha ?? null,
      branch_pr: pr ?? null, issue_unobservable: r.issue === null && pr === undefined,
    };
    const data: Event["data"] = {
      kind: "review.seat", repo: r.repo, ts: iso(r.ts_start), seat_id: r.seat_id, engine: r.engine,
      model: r.model, tier: r.tier, verdict: r.verdict, issue: r.issue, branch: r.branch,
      head_sha: sha ?? null, ts_start: iso(r.ts_start), ts_end: r.ts_end ? iso(r.ts_end) : null,
      script: r.script,
    };
    return { topic: "seats", offset: r.offset, provenance: prov, data };
  });
}

export function itemEvents(items: GhItem[], j: Joins): Event[] {
  const out: Event[] = [];
  for (const it of items) {
    const base = { repo: it.repo, number: it.number, is_pr: it.is_pr };
    // Only what was true at open time: labels (and `pts`) arrive as timeline `labeled` events,
    // and the head sha is today's, so it goes on the merge or close, never on the open.
    const opened: Event["data"] = {
      kind: it.is_pr ? "pr.opened" : "issue.opened", ...base, ts: iso(it.created_at), actor: j.actor(it.actor),
    };
    if (it.is_pr) {
      opened.head_ref = it.head_ref ?? null;
      opened.base_ref = it.base_ref ?? null;
      opened.refs = issueRefs(it.body);
    }
    const prov = { source: "github", item_id: it.id };
    out.push({ topic: it.is_pr ? "pulls" : "issues", offset: it.id, provenance: prov, data: opened });
    if (it.is_pr && (it.merged_at || it.closed_at)) {
      const merged = Boolean(it.merged_at);
      out.push({
        topic: merged ? "merges" : "closes", offset: it.id, provenance: prov,
        data: {
          kind: merged ? "pr.merged" : "pr.closed", ...base, ts: iso((it.merged_at ?? it.closed_at)!),
          merge_commit_sha: merged ? (it.merge_commit_sha ?? null) : null,
          head_sha: it.head_sha ?? null, actor: j.actor(it.closed_by),
        },
      });
    }
  }
  return out;
}

export function timelineEvents(rows: TimelineRow[], j: Joins): Event[] {
  return rows.filter((r) => TIMELINE_KINDS.has(r.event)).map((r) => {
    // `pts` only where the label was added; an `unlabeled` event names the removed label alone.
    const pts = r.label && r.event === "labeled" ? ptsFromLabels([r.label]) : undefined;
    return {
    topic: "timeline",
    offset: r.id ?? `${r.repo}#${r.number}/${r.ordinal}`,
    provenance: { source: "github-timeline", event_id: r.id, ordinal: r.ordinal },
    data: {
      kind: "issue.event", repo: r.repo, ts: iso(r.created_at), number: r.number, event: r.event,
      actor: j.actor(r.actor), label: r.label ?? null, ...(pts !== undefined ? { pts } : {}),
      ref_repo: r.ref_number != null ? (r.ref_repo ?? r.repo) : null, ref_number: r.ref_number ?? null,
      commit_sha: r.commit_sha ?? null,
      ...(r.event === "commented" ? { comment_id: r.id, comment_bytes: r.comment_bytes ?? null } : {}),
    },
  };
  });
}

export function commitEvents(rows: CommitRow[], j: Joins): Event[] {
  return rows.map((c) => ({
    topic: "commits", offset: c.sha, provenance: { source: "git", sha: c.sha },
    data: {
      kind: "commit", repo: c.repo, ts: iso(c.committed), sha: c.sha, parents: c.parents,
      author: j.actor(c.author), refs: issueRefs(c.subject), co_authored: c.co_authored,
    },
  }));
}

export function sprintEvents(rows: SprintRow[]): Event[] {
  return rows.map((s) => ({
    topic: "sprints", offset: s.sprint, provenance: { source: "sprint-log", sprint: s.sprint },
    data: { kind: "sprint.boundary", repo: "lifeos", ts: iso(s.ts), sprint: s.sprint, slot: s.slot, file_id: s.file_id },
  }));
}

/** Events with `since <= ts < until`, sorted by ts, then topic, then offset. `keepEarly` events
 * (commits another event names, so every sha mention resolves) may predate `since`; nothing at or
 * past `until` is ever kept. */
export function window(events: Event[], since: string, until: string, keepEarly?: (e: Event) => boolean): Event[] {
  const lo = iso(since), hi = iso(until);
  return events
    .filter((e) => e.data.ts < hi && (e.data.ts >= lo || (keepEarly?.(e) ?? false)))
    .sort((a, b) =>
      a.data.ts < b.data.ts ? -1 : a.data.ts > b.data.ts ? 1
        : a.topic < b.topic ? -1 : a.topic > b.topic ? 1
          : String(a.offset).localeCompare(String(b.offset), "en", { numeric: true }));
}

function idLine(e: Event): string {
  return JSON.stringify([{ topic: e.topic, partition: 0, offset: e.offset }]);
}

/** The SSE text (header + frames) and the provenance sidecar (one JSON line per frame). */
export function render(header: [string, string, string], events: Event[]): { sse: string; provenance: string } {
  for (const h of header) if (h.includes("\n")) throw new Error("header line contains a newline");
  const head = header.map((h) => `: ${h}\n`).join("") + "\n";
  const frames = events.map((e) => `event: message\nid: ${idLine(e)}\ndata: ${JSON.stringify(e.data)}\n\n`);
  const prov = events.map((e) => JSON.stringify({ id: idLine(e), ...e.provenance }) + "\n");
  return { sse: head + frames.join(""), provenance: prov.join("") };
}

/**
 * Sprint table rows: `| N | YYYY-MM-DD-<h>a|p [(…)] | ... | [file](YYYY-MM-DD-K-...md) ... |`,
 * local time MST. The slot cell's parenthetical takes any form; its start time is `ran HH:MM`,
 * else `ran <h>a|p`, else `slot HH:MM–` (the actual start, which an early or late sprint records
 * there: `(slot 16:36–19:00; early start)`), else the slot hour (s2w#405). Any other parenthetical,
 * e.g. `(stalled …)`, falls back to the slot hour. A row shaped `| N | YYYY-MM-DD` that still fails the pattern is counted in
 * `unparsed`, so a future table-format change shows as a `sprint-unparsed:N` drop (s2w#398).
 */
export function sprintRows(md: string): { rows: SprintRow[]; unparsed: number } {
  const rows: SprintRow[] = [];
  let unparsed = 0;
  for (const line of md.split("\n")) {
    const m = /^\| (\d+) \| (\d{4}-\d{2}-\d{2})-(\d{1,2})([ap])(?: \(([^)]*)\))? \|/.exec(line);
    if (!m) {
      if (/^\| \d+ \| \d{4}-\d{2}-\d{2}/.test(line)) unparsed++;
      continue;
    }
    const [, n, date, h, ap, paren] = m;
    let hour = (Number(h) % 12) + (ap === "p" ? 12 : 0);
    let min = 0;
    const ran24 = /^ran (\d{1,2}):(\d{2})\b/.exec(paren ?? "");
    const ran12 = /^ran (\d{1,2})([ap])\b/.exec(paren ?? "");
    const slot24 = /^slot (\d{1,2}):(\d{2})[–-]/.exec(paren ?? "");
    if (ran24) [hour, min] = [Number(ran24[1]), Number(ran24[2])];
    else if (ran12) hour = (Number(ran12[1]) % 12) + (ran12[2] === "p" ? 12 : 0);
    else if (slot24) [hour, min] = [Number(slot24[1]), Number(slot24[2])];
    const ts = `${date}T${String(hour).padStart(2, "0")}:${String(min).padStart(2, "0")}:00-07:00`;
    const file = /\((\d{4}-\d{2}-\d{2}-\d+)-[^)]*\.md\)/.exec(line)?.[1] ?? null;
    rows.push({ sprint: Number(n), slot: `${date}-${h}${ap}`, ts, file_id: file });
  }
  return { rows, unparsed };
}
