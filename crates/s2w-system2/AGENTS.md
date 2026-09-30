# s2w-system2

System 2: asynchronous, budget-capped model passes over world snapshots.

## Allowed dependencies

- `s2w-model`, `serde`, `serde_json`, `thiserror`; provider clients chosen in decision records

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Never sits in the stream. Proposals never act on the stream from inside this crate;
  auto-apply is a policy decision recorded as a decision row. Outward effects (exports,
  alerts) always need approval through the export manifest (decision 0019).
- Stream text is untrusted: model workers hold no action credentials; proposals use a constrained, validated format.
- Every model output is persisted; replay never re-runs an LLM.
- Every proposal and decision is persisted as an append-only record (decision 0019).
- Never depends on the core or on another adapter.
- No domain knowledge in this crate; see decision 0018.
- Providers sit behind the `Provider` trait. The exec provider runs the operator's command
  with no shell: argv as given, the prompt on stdin only, a cleared environment plus the
  variables the operator passes, an empty working directory, and caps on wall time, stdout
  and stderr. Keeping the model CLI tool-less is the operator's job, not this crate's.
- Prompts are committed data under `prompts/`, not string constants; check 9 reads them, and
  the per-line vocabulary opt-out does not apply there. Untrusted input reaches a prompt only
  through the one JSON line encoder.
- One proposal attempt makes at most two model calls: the first, and one repair when the reply
  fails to decode or validate.
