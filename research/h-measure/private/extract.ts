// Fixed regex extraction from a dev-worker `detail` string (s2w#371, plan section 2).
//
// These patterns are the whole published table: a field the table does not name never leaves
// `detail`, and the raw text is dropped. The capture header carries `EXTRACTION_TABLE` so the
// answer key (#372) can state what is absent by construction.

import type { Repo } from "./events.ts";

export interface DetailFields {
  pr?: number;
  sha?: string; // short or full hex as written; the capture resolves it to 40 chars
  branch?: string;
  leg?: string;
  round?: number;
  verdict?: "BLOCK" | "APPROVE";
  engines?: string;
}

const PATTERNS = {
  pr: /\bPR #(\d+)\b/,
  sha: /@([0-9a-f]{7,40})\b/,
  branch: /\b((?:feat|fix|chore|docs|pair)\/[A-Za-z0-9._-]+(?:\/[A-Za-z0-9._-]+)*)/,
  leg: /\bleg ([A-Z])\b/,
  round: /\bround (\d+)\b/i,
  verdict: /\b(BLOCK|APPROVE)\b/,
  engines: /\b(P:[A-Za-z0-9+:,-]+ I:[A-Za-z0-9+:,-]+ R:[A-Za-z0-9+:,-]+)/,
} as const;

/** An issue or PR reference in a PR body or commit subject: an optional repo prefix, then `#n`. */
const REF = /(?<![\w/])([\w.\/-]+)?#(\d+)\b/g;

/** The prefixes a ref may carry (s2w#395 Q3). GitHub writes the long form in cross-repo bodies. */
const REF_ALIASES: Readonly<Record<string, Repo>> = {
  lifeos: "lifeos", s2w: "s2w", "daveremy/lifeos": "lifeos", "daveremy/stream2worlds": "s2w",
};

/** One line per field, `name=/source/`; written into the capture header. */
export const EXTRACTION_TABLE = [...Object.entries(PATTERNS), ["refs", REF] as const]
  .map(([k, re]) => `${k}=${re.source}`)
  .join(" ");

export function extractDetail(detail: string | null): DetailFields {
  const d = detail ?? "";
  const out: DetailFields = {};
  const m = (re: RegExp) => re.exec(d)?.[1];
  const pr = m(PATTERNS.pr);
  if (pr !== undefined) out.pr = Number(pr);
  const sha = m(PATTERNS.sha);
  if (sha !== undefined) out.sha = sha;
  const branch = m(PATTERNS.branch);
  if (branch !== undefined) out.branch = branch.replace(/[.,;:)]+$/, "");
  const leg = m(PATTERNS.leg);
  if (leg !== undefined) out.leg = leg;
  const round = m(PATTERNS.round);
  if (round !== undefined) out.round = Number(round);
  const verdict = m(PATTERNS.verdict);
  if (verdict === "BLOCK" || verdict === "APPROVE") out.verdict = verdict;
  const engines = m(PATTERNS.engines);
  if (engines !== undefined) out.engines = engines;
  return out;
}

export interface Ref { repo: Repo; number: number }

/** The issues and PRs a PR body or commit subject names (s2w#395). A bare `#n` is the event's own
 * repo (`own`), as GitHub reads it; `lifeos#n`, `s2w#n` and their `daveremy/...` long forms name
 * that repo; any other prefix (`a/#7`, `foo_lifeos#5`) is dropped and counted in `dropped`.
 * Deduplicated on `(repo, number)`, sorted by repo then number. */
export function issueRefs(text: string | null, own: Repo): { refs: Ref[]; dropped: number } {
  const seen = new Map<string, Ref>();
  let dropped = 0;
  for (const x of (text ?? "").matchAll(REF)) {
    const repo = x[1] === undefined ? own : REF_ALIASES[x[1]];
    if (!repo) { dropped += 1; continue; }
    const number = Number(x[2]);
    seen.set(`${repo}#${number}`, { repo, number });
  }
  const refs = [...seen.values()].sort((a, b) => (a.repo < b.repo ? -1 : a.repo > b.repo ? 1 : a.number - b.number));
  return { refs, dropped };
}

/** `pts:N` label value, or undefined. */
export function ptsFromLabels(labels: string[]): number | undefined {
  for (const l of labels) {
    const x = /^pts:(\d+(?:\.\d+)?)$/.exec(l);
    if (x) return Number(x[1]);
  }
  return undefined;
}
