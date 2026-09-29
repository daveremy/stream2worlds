# 0010: Gate 3 arm H — what it contains, and Rebmann 2022's disposition

Date: 2026-09-27 · Status: accepted · Gate 3 · Issue #4 · Research [0002](../../research/0002-structure-without-llm.md), [0006](../../research/0006-scaling.md)

## Decision

**Arm H is the full seven-stage design in research 0002 §6** (flatten → profile → event-type
field → role signatures → relate identifiers → assemble types → emit), including local name
embeddings, exact 64-bit hash sets for containment up to the cap and the HLL++/Bloom tier past it
(0006 §8). This matches the evaluation contract's own definition of H (B1: "heuristics alone —
System 1 rules and local embeddings") — this record does not redefine that phrase, it fixes what
"System 1 rules" means in Rust.

**Rebmann, Rehse and van der Aa (BPM 2022) is a component of H, not a fourth arm, and not merely
a citation.** Two independent reasons:

1. Contract B1 fixes the arms at H, H+S2 and B3. Adding a fourth arm needs a contract amendment;
   this record does not propose one.
2. The method has two halves. Its case-object candidates come from a BERT tagger reading
   attribute *names* — that half has no input on the obfuscated stream (`f1…fN`, contract B2.2)
   and is exactly what obfuscation is designed to disable. Its other half — the entity-id
   signature of research 0002 §3, and the cardinality/alias rule of §4, with the ≥0.9
   uniqueness threshold attributed to Rebmann's public code in §5's table — is already what H
   contains.

**What H takes from Rebmann 2022:** the ≥0.9 uniqueness threshold as a starting value for H's
role-classification tuning, and its cardinality rule for H's relate stage. **What H does not
take:** the name-based tagger. **Caveat carried forward from research 0002:** Rebmann et al.'s
paper body is UNVERIFIED there — only its public code was read. This record repeats that caveat
rather than upgrading it to confirmed.

**Rebmann's plain-vs-obfuscated split is itself informative, per the issue and Dave's comment,**
and is not measured by this record — measuring it (H's role classification, with and without a
name tagger, on the plain stream only, reported not counted) is follow-up work, not part of
closing #4. See "Revisit when" and issue #56.

**H-min** (0002 §6 stages 1-4 plus containment/alias in stage 5 only; no composite-key
refinement, no carry-over, no name embeddings) is the first slice of H to be measured, not the
arm itself — a strict subset, and, on the plain stream, a **lower bound** on H's eventual score
(not an upper bound: H-min omits signals that only add identity evidence). H-min is scoped and
built in issue #56, a follow-up to this record, not this one.

## Architecture note (informational, not amended here)

Building H needs a corpus-level profiler: a batch process over 10^5-10^6 events producing one
mapping per corpus, not a per-event verdict. It is not a specialization of #51's `Engine` trait
(total, per-event, stateless across events) — H is a producer of the mapping a future
mapping-driven executor would consume, not a consumer of already-known claims. It belongs in a
new pure crate, sibling to `s2w-core` (deps: `s2w-model` only, plus what JSON flattening needs),
not inside `s2w-system1` (an adapter crate with no code yet, being built this sprint by #51 for
a different trait) and not as a script outside `crates/`. The workspace-layers amendment, the new
crate, its executor, scorer and measurement driver are scoped and built in issue #56 —
adding a workspace member is a decision in its own right, made when that issue lands, not
reserved here.

## Why

Gate 3 is only meaningful if H is the strongest non-LLM method available, or System 2's win is
against a straw man. Research 0002 Verdict 4 warns H may already score above 0.90 identity F1 on
plain Wikipedia, which would make the contract's 0.10 margin (B4.1) unreachable — so H needs a
fixed, complete definition before anyone spends design effort on System 2's side of the
comparison, and Rebmann's real contribution (the uniqueness and cardinality rules) needs to be
inside that definition rather than debated separately every time it comes up.

## Alternatives considered

- **Rebmann as a fourth arm.** Rejected: contradicts contract B1's fixed arm set. Its novel
  contribution — the BERT name tagger — only has input where field names are readable (the plain
  and private streams, contract B2.1/B2.3; research 0002 §"Where local embeddings fit"); on those
  streams its signal occupies the slot H's own local name embeddings already fill, so a fourth
  arm would duplicate B1's H rather than add a distinct method. On the obfuscated stream (B2.2)
  it has no input at all.
- **Rebmann as citation only.** Rejected: its uniqueness threshold and cardinality rule are
  concrete inputs H's tuning can start from, not background reading — the "0.9 rule" is more use
  than a footnote.

## Revisit when

- ~~Issue #17 (identifier-domain and obfuscation-hashing questions) lands~~ (answered 2026-09-29, contract amendment) — several of H-min's
  measurement conventions (not this record) are provisional on its answers.
- The obfuscated stream and its obfuscator (contract B2.2) exist, so H-min's plain-stream number
  can be compared against an obfuscated one instead of standing alone.
- H-min is measured (issue #56) and either confirms or revises the "lower bound" reading
  above.
  *2026-09-29: measured as H-lite (H-min without containment, `PROFILER_VERSION` 2; research
  [0009](../../research/0009-h-min-plain-wikipedia.md)). Held-out identity F1 0.284 and 0.293
  on two spans, entity recovery 0. The lower-bound reading stands; containment, the stage that
  makes H-lite H-min, is #244.*
