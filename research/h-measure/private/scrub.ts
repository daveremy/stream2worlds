// Fail-closed scrub gate for the private-stream capture (s2w#371, plan section 6).
//
// The capture writes nothing unless every line it would write passes `scrubViolations`.
// Each refused pattern has its own test in capture.test.ts (karpathy ruling 1, 2026-09-30).
// The secret shapes copy the high-confidence rows of lifeos `scripts/lib/secret_shapes.py`;
// a copy, not an import, so the gate runs without a lifeos checkout.

export interface Rule {
  name: string;
  re: RegExp;
}

export const RULES: readonly Rule[] = [
  { name: "home-path", re: /\/home\// },
  { name: "tilde", re: /~/ },
  { name: "obsidian", re: /obsidian/i },
  { name: "op-ref", re: /op:\/\//i },
  { name: "email", re: /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}/ },
  // North-American shape with separators: digit runs alone (ids, epoch values) stay legal.
  { name: "phone", re: /(?:\+?1[\s.-]?)?\(?\b\d{3}\)?[\s.-]\d{3}[\s.-]\d{4}\b/ },
  { name: "aws-access-key-id", re: /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/ },
  { name: "github-pat", re: /\b(?:ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9]{36,}\b/ },
  { name: "github-fine-grained-pat", re: /\bgithub_pat_[A-Za-z0-9_]{60,}\b/ },
  { name: "anthropic-api-key", re: /\bsk-ant-[A-Za-z0-9_-]{24,}\b/ },
  { name: "openai-api-key", re: /\bsk-(?!ant-)(?:proj-)?[A-Za-z0-9_-]{32,}\b/ },
  { name: "slack-token", re: /\bxox[abposr]-[A-Za-z0-9-]{10,}\b/ },
  { name: "gitlab-pat", re: /\bglpat-[A-Za-z0-9_-]{20}\b/ },
  { name: "google-api-key", re: /\bAIza[0-9A-Za-z_-]{35}\b/ },
  { name: "stripe-key", re: /\b(?:sk|rk)_(?:live|test)_[A-Za-z0-9]{20,}\b/ },
  { name: "npm-token", re: /\bnpm_[A-Za-z0-9]{36}\b/ },
  { name: "telegram-bot-token", re: /\b\d{8,10}:AA[A-Za-z0-9_-]{33}\b/ },
  { name: "jwt", re: /\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/ },
  { name: "posthog-personal-api-key", re: /\bphx_[A-Za-z0-9_-]{32,}\b/ },
  { name: "private-key", re: /PRIVATE KEY/ },
];

export interface Violation {
  line: number;
  rule: string;
}

/** Every (line, rule) the text breaks; empty means the text may be written. */
export function scrubViolations(text: string): Violation[] {
  const out: Violation[] = [];
  text.split("\n").forEach((line, i) => {
    for (const r of RULES) if (r.re.test(line)) out.push({ line: i + 1, rule: r.name });
  });
  return out;
}

/** Throws (naming rules and line numbers, never the matched text) unless `text` is clean. */
export function assertClean(text: string): void {
  const v = scrubViolations(text);
  if (v.length === 0) return;
  const shown = v.slice(0, 10).map((x) => `line ${x.line}: ${x.rule}`);
  throw new Error(`scrub gate refused ${v.length} match(es): ${shown.join("; ")}`);
}
