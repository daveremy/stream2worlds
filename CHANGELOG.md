# Changelog

How Stream2Worlds got built, one entry per sprint, newest first. Git history records every commit; this file records the arc: what became possible, what we learned, and where the plan changed direction.

Each entry has the same four parts:

- **Shipped:** what now works or is decided, and why it matters.
- **Learned:** what a measurement, review or research note taught us.
- **Changed course:** decisions that overturned an earlier plan, with the reason.
- **Next:** what the following sprint picks up.

A sprint without a merge still gets an entry. What it learned is often the most useful part.

---

## Read-only MCP over an on-disk world — #115 (2026-09-28)

**Shipped:** `s2w mcp --log-dir PATH [--world NAME]` opens the event and verdict SQLite
databases without either writer lock, reconstructs a bounded one-shot timeline from durably
stored verdicts, and serves it through the existing five MCP tools while another process can
keep writing.

**Learned:** the verdict cursor must be captured once to give replay an explicit upper bound,
and multiple stored versions from one engine must collapse to the first-written row or their
claims are served twice.

**Changed course:** read-only replay does not run engines and does not load manifest or
membership metadata. Without an engine registry it serves the first stored verdict for every
historic engine, including retired ones.

**Next:** add live snapshot refresh ([#128](https://github.com/daveremy/stream2worlds/issues/128))
and read-only manifest/membership loading ([#129](https://github.com/daveremy/stream2worlds/issues/129));
decide whether `mcp --log-dir` should gain an engine-policy configuration surface.

---

## Domain as data — decisions 0017/0018, readable view, research 0008 (2026-09-28)

**Shipped:** [Decision 0017](docs/decisions/0017-view-and-agents-first-class.md) makes
visualization and agent use first-class: a view spec is authored by System 2 and stored as a
log event, shape comes before domain, and every gate reports a lifetime baseline plus surprise
against it. [Decision 0018](docs/decisions/0018-no-compiled-domain-code.md) retires compiled
domain code entirely — no crate, type, flag or branch may name or key on a domain; entity
types, labels and links are discovered data, enforced by a domain-vocabulary scan and an
obfuscation replay. The live web view ([#114](https://github.com/daveremy/stream2worlds/issues/114))
now reads readable from world data alone: a derived per-entity-type label (highest
coverage×distinctness string attribute, id-like values rejected), deterministic colors with a
legend, degree-based sizing, and "Active now"/"Hubs" panels. `serve`/`mcp --json` gained the
same structured stdout/stderr progress `watch --json` already had ([#110](https://github.com/daveremy/stream2worlds/issues/110)
part 1 — MCP itself is tracked separately as #115). [#123](https://github.com/daveremy/stream2worlds/issues/123)
closed the review gap on #114: a rename-invariance test proving the label pick survives an
attribute rename (0018's own acceptance test), derivations computed once per snapshot instead
of once per SSE message, and three label-scoring fixes (score-before-whitespace tie-break, a
digit/dash guard on the id-like heuristic, hub ranking using the larger of recorded and
visible degree).

**Learned:** [Research 0008](research/0008-view-spec-and-surprise.md) (sagan) grounds 0017's shape: a
System 2-authored view spec, shape-before-domain, a lifetime baseline with surprise scoring,
and a readiness rule frozen to the H+S2 arm — the obfuscated view form is reported, not graded
as a pass/fail.

**Changed course:** `WikimediaPageChangeEngine` and the Wikimedia-bound embeddings schema are
retired per 0018; a new stream shows raw identifiers and inferred shapes until the H heuristics
land, which is the honest state of the product rather than a regression to hide.

**Next:** the H heuristics (research 0002 §6's seven domain-free stages) that make a new
stream typed without any domain code.

---

## No compiled domain code — #119 (2026-09-28)

**Shipped:** retired the Wikimedia-bound compiled engines (`WikimediaPageChangeEngine`, the
local-embeddings engine and its `model2vec-rs` dependency), the `Wikimedia` SSE dialect, and
the `--wiki` flag; the `wikipedia` preset is now URL+settings data over the generic `sse`
transport, with no domain-named dialect behind it (decision 0018). Two new `cargo xtask check`
fitness functions enforce the rule going forward: a vocabulary scan (`xtask/src/vocabulary.rs`)
that denylists domain terms across every crate's `src/` tree and the web view's TypeScript, and
an obfuscation-replay check (`xtask/src/obfuscation.rs`) that folds the golden event log twice —
once plain, once with every claim identifier/attribute-key/string value renamed and hashed —
and fails if the two folded worlds differ, catching code that reads a specific name or value
instead of just shape. Also retired the now-dormant version-history append-only check
(`xtask/src/version_history.rs`), a vestige of the deleted embeddings engine.

**Learned:** the token-boundary design for the vocabulary scan (a denylist entry matches a
contiguous run of tokens, so `wiki_id` and `wikiId` both hit a `wiki` entry) is what makes one
entry catch every spelling convention, at the cost of needing an explicit `// vocabulary: allow`
escape hatch for legitimate data (e.g. the `wikipedia` preset's own name and URL).

**Changed course:** domain-pack crates (this issue's original design) were superseded same-day
by Dave's ruling that no compiled code for a domain should exist at all — see decision 0018 and
the issue's superseding comment.

**Next:** a generic, data-driven field filter (`--filter <json-path>=<value>`) to replace what
`--wiki` provided, plus obfuscation-replay coverage for the System 1 bridge/engines layer —
[#131](https://github.com/daveremy/stream2worlds/issues/131).