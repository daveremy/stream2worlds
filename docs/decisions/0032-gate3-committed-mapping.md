# 0032: System 2 commits one mapping per replicate for gate 3

Date: 2026-09-30 · Status: accepted · Gate 3 · Issue #373 · Builds on [0029](0029-dashboard-manifest-v0.md), [0027](0027-stream-mapping-links.md)

## Decision

**For gate 3, System 2 proposes one executable `StreamMapping` per stream per replicate, under a
$5 hard stop, and the result is committed to git before any answer key is read.** The
proposer is `MappingProposer` in `s2w-system2`. It serves both arms of the comparison
(`docs/evaluation-contract.md` §B1) with one reply format, one repair prompt and one retry
policy:

- **"H plus System 2"** (`propose`) reads a `MappingInput` (`s2w-model`): the heuristic
  profiler's result over the development window, that is its mapping and identity (or `null`
  when it abstained), its decoded paths, its event-type path, per-path statistics, and a
  sample of decoded events.
- **B3** (`propose_raw`) reads a `RawMappingInput`: raw events of the same window, sampled by
  a frozen rule, and nothing else.

The reply must decode as a `StreamMapping` and pass `validate()`. The prompts ask for version 2
(links allowed, decision 0027). A version-1 reply that validates is accepted too, because it is
a version-2 mapping with no links.

This decision is delivered in three PRs. PR 1 (this record) ships the proposer, the prompts,
the inputs, `ReplyFormat::ClaudeJson` and recording format 2. PR 2 ships the driver, the price
table, the ledger and hard stop, the clean-session runner and `score` acceptance. PR 3 ships the
B3 sampler and a live dry run. The sections below marked "PR 2" are decided here and built
there.

## Prompts

Four committed files under `crates/s2w-system2/prompts/`, not three:

| File | Role |
|---|---|
| `mapping.txt` | The "H plus System 2" first prompt |
| `mapping-raw.txt` | The B3 first prompt |
| `mapping-format.txt` | The reply rules, filled into both first prompts at `{{FORMAT}}` |
| `mapping-repair.txt` | Appended to either first prompt for the repair call |

The format rules sit in one shared file so both arms are held to byte-identical output rules
(§B1 fairness). The two first prompts differ only in what they say the data is. Each arm's
`prompt_files_hash` hashes its first prompt, the format file and the repair file. It folds into
a committed run's `input_hash`, so an edit to any of the three is a new input. It is not the
replay key. The replay key is each call's `prompt_hash`, the hash of the filled prompt as sent.

Untrusted input reaches a prompt only through the one JSON line encoder (decision 0029). The
reply and the fault in a repair prompt go through it too.

## Retry policy (frozen, shared by both arms)

1. One attempt is at most two calls: the first prompt, then one repair call that carries the
   reply and its fault (decode or validator error) back. A repaired reply that still fails is
   `invalid: <fault>`, and that is the replicate's result. There is no second attempt.
2. A provider failure (timeout, exit status, output over cap, malformed envelope, a replayed
   failure) is never repaired. It ends the attempt, and one further attempt starts again from
   the first prompt. A provider failure in the second attempt is `provider: <error>`. So a
   replicate makes at most four calls per arm (`MAX_ATTEMPTS` = 2).
3. A `CallGate` is asked before every call, with the prompt and every call made so far
   (their tokens and cost included). `Err(reason)` stops the proposal before the call, and
   `reason` is the result. The budget stop (PR 2) is a gate, so it overrides both rules above.
4. The only execution assistance is the repair call's fault text, the same for both arms.

A failed proposal is a result, not a retry trigger for the driver: `mapping: null` grades as the
empty mapping, the way an abstention is graded today.

## Recording format 2

Every call is a `CallRecord`: `attempt`, `call` (1 first, 2 repair), `prompt_hash`, exactly one
of `reply` and `error`, `model` as the provider reports it, `input_tokens`, `output_tokens`,
`cache_read_tokens`, `cache_write_tokens`, `cost_usd`, `latency_ms` and `started_at_ms` (Unix
milliseconds, from an injectable clock). A recording is `{"format": 2, "calls": [...]}`.

`ReplayProvider` reads format 1 and format 2. Format 1 keeps its behaviour: one reply per
prompt, answered as often as it is asked, and a duplicate hash is refused. Format 2 is strict:
the n-th ask of a prompt gets the n-th row recorded for it, a failure row replays as a failure
that displays the same (its captured stdout is not kept), and an ask past the last row is
`NotRecorded`. That makes a recorded run with a
provider failure and a retry replay call for call. `ReplayProvider::to_json` still writes
format 1 and refuses (`Lossy`) when it cannot hold what the provider holds: a format-2
provider, a failure, several rows for one prompt, or a reply's cache tokens, cost or model.

## Reply format: `ClaudeJson`

`ExecProvider::with_format(ReplyFormat::ClaudeJson)` parses the headless Claude CLI's
`--output-format json` envelope. `is_error` true is a failure. Otherwise `result`, the four
`usage` token counts and `total_cost_usd` are required, and `modelUsage` keys give the reported
model. A missing field or non-JSON stdout is `ProviderError::Envelope`, never a reply with null
tokens: the $5 ledger cannot run on unknown counts. This supersedes 0029's "Tokens are null on
this path" for runs that use `ClaudeJson`. `ReplyFormat::Text` (the default) is unchanged.

## The committed file (PR 2)

`research/h-measure/committed/<arm>.<corpus>.r<k>.json`:
`{format, arm, corpus, corpus_sha256, window, replicate, provider, model, prompt_hash, input_hash,
pins, attempts, mapping | null, failure | null, spend, transcript_sha256}`, beside
`<out>.transcript.json` (recording format 2). `model` is the snapshot id as configured, never
from a reply; the reported model is in each call record. The file is committed to git before
`score` runs, and the git log is the order proof. `score --mapping <committed file>` rebuilds the
input, replays the transcript and refuses unless the proposer reproduces the same mapping or the
same failure, so a hand-edited mapping never scores.

## Price table and hard stop (PR 2)

`research/h-measure/prices.toml`, beside `corpora.toml`:
`[model."<snapshot id>"] input, output, cache_write, cache_read` in USD per million tokens, with
`source_url` and `copied_on`. Dollars = this table × reported tokens. The CLI's
`total_cost_usd` is recorded as a cross-check and never used for the cap. Before every call:
`spent + price(prompt bytes / 3 input tokens, 16,384 output tokens) > $5` stops the replicate
with `failure: "budget"`. An unknown snapshot id is refused at load.

## Clean session (PR 2)

Provider 1 is the headless Claude CLI pinned to a dated snapshot, tool-less (`--tools ""`), with
`--strict-mcp-config` and no MCP config, `--no-session-persistence`, `--output-format json`, the
prompt on stdin. `ExecProvider` already clears the environment and runs in an empty directory.
The driver adds a scratch `HOME` that holds only the credentials file (no user instructions, no
memory, no MCP servers) and removes it after the run. The first call of a run is a probe that
asks the model to list every instruction, memory or file it can see. The run refuses to go on
unless the reply is `none`, and the probe is in the ledger. Dave approved this transport for
provider 1 on 2026-09-30 (s2w#373). PR 2 adds the dated contract note.

## Surfaces (decision 0017)

A committed run file is a research artifact, like an h-measure freeze. It has no view and no MCP
surface.

## Dated note 2026-09-30: what PR 2 built (s2w#373)

`cargo xtask gate3 commit` (`xtask/src/h_measure/gate3/`) implements the three PR 2 sections
above. It differs from them in these ways:

- **File shape.** The committed file carries `kind: "s2w-gate3-committed"` (how `score` tells it
  from a frozen mapping) and nests the corpus, its sha256, the window and every pin in
  `heuristic`, which is exactly what `h-measure freeze` writes for the same corpus and window,
  so `score` checks the H part as it checks a freeze. `prompt_hash` is `prompt_files_hash`. It
  adds `price` (the `prices.toml` row it was charged by), `probe` (the probe's call record),
  and `sample_events` / `sample_string_chars` (60 and 200). `score` replays the probe and the
  transcript through the same function and budget gate as the live run, and also refuses a
  replay whose spend differs from the recorded `spend`.
- **Budget failure text.** The failure is `budget: spent $X, next call estimated $Y, cap $5.00`,
  with fixed decimals so a replay writes it byte for byte. The estimate is more conservative
  than the section above: one input token per 2 prompt bytes (JSON with ids and hashes
  tokenizes near that) plus 8,192 tokens for the CLI's built-in system prompt, all charged at
  the higher of the input and cache-write rates, because the CLI may cache-write the prompt.
  The session sets `CLAUDE_CODE_MAX_OUTPUT_TOKENS` to 16,384, so the output part is a bound. A
  call that reported no tokens is charged its estimate.
- **CLI cap.** `--max-budget-usd` is fixed at 5, the whole cap. `ExecProvider`'s argv is fixed per
  provider, so the CLI cannot be given the per-call remainder; the gate enforces that.
- **Credentials.** The scratch `HOME` gets a copy of the operator's credentials file only when
  its access token stays valid for at least 90 more minutes (five calls at the 15-minute call
  timeout, plus 15 minutes). The CLI then never refreshes it:
  OAuth refresh tokens are single-use, and a refresh inside the copy would spend the token the
  operator's own sessions hold (lifeos#1252). A copy the CLI rewrote anyway is reported loudly
  after the run and kept beside the operator's file, since it then holds the live refresh token.
- **Probe wording.** The probe asks for anything besides the model's built-in system prompt and
  the probe itself, because the CLI always sends a built-in system prompt.
- **Call timeout.** Each call may run 15 minutes (`ExecLimits` default is 3), because a mapping
  reply may use 16k output tokens.
- **Probe failure.** A failed probe writes no file: the session is not proven clean, so there is
  no replicate. The error names what the probe was charged.

## Dated note 2026-10-01: what PR 3 built (s2w#373)

`cargo xtask gate3 commit --arm b3` (`xtask/src/h_measure/gate3/b3.rs`) is the raw-sample
baseline of contract §B1: the same model, task, prompt format and System 2 path
(`MappingProposer::propose_raw` shares `propose`'s attempt and repair loop) as h-s2, given raw
events of the same development window instead of H's result.

- **The budget.** A b3 replicate is sized by the h-s2 replicate with the same corpus, window,
  replicate, model and price (`--h-s2 FILE`). Before any call, b3 replays that file exactly as
  `score` does, so a stale or hand-written h-s2 file never sets a budget. T is the prompt tokens
  of the h-s2 transcript's first call that reported tokens: input plus cache read plus cache
  write. The CLI caches the prompt, so `input_tokens` alone is a handful; the sum is what the
  model read. It includes the CLI's built-in system prompt, which b3's calls carry too. An
  h-s2 replicate whose calls reported no tokens has no budget, and b3 refuses it.
- **The sampler (frozen).** The events are each frame's `data` string as the stream carried
  it: no profile, no H, no truncation. The sample is every k-th event of the window from the
  first. k is the smallest value whose b3 first prompt is at most B bytes, where B is the
  h-s2 first prompt's length rebuilt from the committed heuristic. Bytes against bytes is the
  plan's "bytes/3" rule with the 3 cancelled on both sides, and it needs no estimate of the
  CLI's system-prompt overhead.
- **The check and refit.** After a fit's proposal, its first call that reported tokens is
  counted the same way as T. Above 105% of T, the fit is spent (its calls stay in the
  transcript and the ledger) and the next fit is the smallest k above it that fits B. There is no fixed fit count:
  each fit's calls pass a budget gate seeded with everything spent before them, so the $5 cap
  bounds the refits. When not even one event fits B, or an over-budget fit was already a
  one-event sample (every larger k samples the same first event), the replicate is committed
  with `failure: "budget-fit: ..."`.
- **Two samples, one window.** h-s2's sample is the 60 newest events of the window, after H;
  b3's is every k-th event from the first. The difference is planned: both are within the
  window, and each arm gets the sample its input form calls for.
- **File shape.** A b3 committed file carries `heuristic` (the same freeze, for its pins and its
  B) and adds `budget`: the h-s2 file's sha256, T, B, and every fit (`k`, events, prompt bytes,
  calls, first-prompt tokens). Its `input_hash` hashes the h-s2 input, the last raw input sent
  and `prompt_files_hash`. `score` replays every fit and refuses unless the fits, the input
  hash, the result, the attempts and the spend all match. h-s2 files are unchanged (FORMAT 1).
- **Private-corpus probe test.** Deferred from s2w#371: a stand-in private corpus (the
  synthetic private fixture, pinned as a development corpus) and a fake `claude` that answers
  the probe `none` only when nothing it can observe (stdin, argv, environment, working
  directory, `HOME` and their parents) names the corpus directory, its name or any of its
  events. The committed probe reply is `none`, and the same observer given the corpus
  directory in its environment reports it.

DRYRUN_PLACEHOLDER
