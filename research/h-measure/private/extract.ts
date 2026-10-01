// Fixed regex extraction from a dev-worker `detail` string (s2w#371, plan section 2).
//
// These patterns are the whole published table: a field the table does not name never leaves
// `detail`, and the raw text is dropped. The capture header carries `EXTRACTION_TABLE` so the
// answer key (#372) can state what is absent by construction.

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
  engines: /\b(P:\S+ I:\S+ R:\S+)/,
} as const;

/** One line per field, `name=/source/`; written into the capture header. */
export const EXTRACTION_TABLE = Object.entries(PATTERNS)
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

/** Issue numbers a PR body or commit subject names: `#n` (covers `Closes #n`, `Part of #n`). */
export function issueRefs(text: string | null): number[] {
  const seen = new Set<number>();
  for (const x of (text ?? "").matchAll(/(?<![\w/])#(\d+)\b/g)) seen.add(Number(x[1]));
  return [...seen].sort((a, b) => a - b);
}

/** `pts:N` label value, or undefined. */
export function ptsFromLabels(labels: string[]): number | undefined {
  for (const l of labels) {
    const x = /^pts:(\d+(?:\.\d+)?)$/.exec(l);
    if (x) return Number(x[1]);
  }
  return undefined;
}
