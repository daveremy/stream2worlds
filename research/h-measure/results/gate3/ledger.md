# Gate 3 ledger (s2w#374 PR 2)

Built from `research/h-measure/committed/*.json` and their transcripts. No row here is a score;
PR 3 scores these files. Dollars are the `prices.toml` table times reported tokens (`spend.usd`,
the probe included); `CLI $` is the CLI's own `total_cost_usd` sum, a cross-check only.
Latency is the sum of every call's `latency_ms`, the probe included. Model `claude-sonnet-5-5`.

| arm | stream | corpus | K | outcome | attempts | calls | input | cache read | cache write | output | $ | CLI $ | latency s | fits (k, events, tokens) | no-match first / repair calls / after | reported model |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| b3 | obf | `obf-r2-dev` | r2 | mapping | 1 | 3 | 6 | 3140 | 77379 | 2172 | 0.3319 | 0.3319 | 26.2 | T 35486, B 54873 bytes: (198, 51, 42808); (247, 41, 35154) | no / 0 / no | claude-sonnet-5-5 |
| b3 | obf | `obf-r3-dev` | r3 | mapping | 1 | 4 | 8 | 5384 | 111350 | 3001 | 0.4765 | 0.4765 | 33.1 | T 34855, B 54276 bytes: (198, 51, 42426); (249, 41, 35415) | yes / 1 / no | claude-sonnet-5-5 |
| b3 | obf | `obf-r4-dev` | r4 | mapping | 1 | 3 | 6 | 3225 | 76516 | 2194 | 0.3287 | 0.3287 | 26.8 | T 35035, B 54862 bytes: (198, 51, 42323); (247, 41, 34770) | no / 0 / no | claude-sonnet-5-5 |
| b3 | obf | `obf-r5-dev` | r5 | mapping | 1 | 3 | 6 | 3140 | 77293 | 1988 | 0.3297 | 0.3297 | 25.0 | T 35362, B 54700 bytes: (198, 51, 42727); (247, 41, 35149) | no / 0 / no | claude-sonnet-5-5 |
| b3 | obf | `obf-r6-dev` | r6 | mapping | 1 | 3 | 6 | 3225 | 76386 | 2011 | 0.3263 | 0.3263 | 29.0 | T 34918, B 54651 bytes: (198, 51, 42241); (247, 41, 34722) | no / 0 / no | claude-sonnet-5-5 |
| b3 | private | `private-dev-2` | r2 | mapping | 1 | 4 | 8 | 5384 | 45845 | 5958 | 0.2441 | 0.2441 | 50.8 | T 14844, B 24275 bytes: (156, 52, 18403); (200, 41, 15850); (221, 37, 14330) | no / 0 / no | claude-sonnet-5-5 |
| b3 | private | `private-dev-2` | r3 | mapping | 1 | 4 | 8 | 5384 | 45845 | 6076 | 0.2452 | 0.2452 | 50.4 | T 14844, B 24275 bytes: (156, 52, 18403); (200, 41, 15850); (221, 37, 14330) | no / 0 / no | claude-sonnet-5-5 |
| b3 | private | `private-dev-2` | r4 | mapping | 1 | 3 | 6 | 3140 | 33123 | 4378 | 0.1769 | 0.1769 | 36.0 | T 14929, B 24275 bytes: (156, 52, 18318); (198, 41, 15388) | no / 0 / no | claude-sonnet-5-5 |
| b3 | private | `private-dev-2` | r5 | mapping | 1 | 3 | 6 | 3225 | 32781 | 4110 | 0.1729 | 0.1729 | 36.9 | T 14929, B 24275 bytes: (156, 52, 18403); (199, 41, 14955) | no / 0 / no | claude-sonnet-5-5 |
| b3 | private | `private-dev-2` | r6 | mapping | 1 | 3 | 6 | 3225 | 32781 | 4339 | 0.1752 | 0.1752 | 37.0 | T 14929, B 24275 bytes: (156, 52, 18403); (199, 41, 14955) | no / 0 / no | claude-sonnet-5-5 |
| h-s2 | obf | `obf-r2-dev` | r2 | mapping | 1 | 2 | 4 | 1066 | 37070 | 1830 | 0.1668 | 0.1668 | 18.8 | n/a | no / 0 / no | claude-sonnet-5-5 |
| h-s2 | obf | `obf-r3-dev` | r3 | mapping | 1 | 2 | 4 | 2605 | 34809 | 1770 | 0.1575 | 0.1575 | 16.4 | n/a | no / 0 / no | claude-sonnet-5-5 |
| h-s2 | obf | `obf-r4-dev` | r4 | mapping | 1 | 2 | 4 | 1066 | 36619 | 1734 | 0.1640 | 0.1640 | 19.6 | n/a | no / 0 / no | claude-sonnet-5-5 |
| h-s2 | obf | `obf-r5-dev` | r5 | mapping | 1 | 2 | 4 | 2605 | 35316 | 1813 | 0.1599 | 0.1599 | 17.8 | n/a | no / 0 / no | claude-sonnet-5-5 |
| h-s2 | obf | `obf-r6-dev` | r6 | mapping | 1 | 2 | 4 | 1066 | 36502 | 2050 | 0.1667 | 0.1667 | 17.5 | n/a | no / 0 / no | claude-sonnet-5-5 |
| h-s2 | private | `private-dev-2` | r2 | mapping | 1 | 2 | 4 | 2605 | 14798 | 2167 | 0.0814 | 0.0814 | 20.5 | n/a | no / 0 / no | claude-sonnet-5-5 |
| h-s2 | private | `private-dev-2` | r3 | mapping | 1 | 2 | 4 | 2605 | 14798 | 2401 | 0.0837 | 0.0837 | 21.1 | n/a | no / 0 / no | claude-sonnet-5-5 |
| h-s2 | private | `private-dev-2` | r4 | mapping | 1 | 2 | 4 | 1066 | 16513 | 2257 | 0.0888 | 0.0888 | 21.1 | n/a | no / 0 / no | claude-sonnet-5-5 |
| h-s2 | private | `private-dev-2` | r5 | mapping | 1 | 3 | 6 | 3225 | 31223 | 3952 | 0.1651 | 0.1651 | 34.0 | n/a | yes / 1 / no | claude-sonnet-5-5 |
| h-s2 | private | `private-dev-2` | r6 | mapping | 1 | 2 | 4 | 1066 | 16513 | 2196 | 0.0882 | 0.0882 | 19.4 | n/a | no / 0 / no | claude-sonnet-5-5 |

## Spend

| arm | stream | replicates | $ total | $ max | over $5 |
|---|---|---|---|---|---|
| h-s2 | obf | 5 | 0.8150 | 0.1668 | 0 |
| h-s2 | private | 5 | 0.5073 | 0.1651 | 0 |
| b3 | obf | 5 | 1.7930 | 0.4765 | 0 |
| b3 | private | 5 | 1.0142 | 0.2452 | 0 |

Committed total: **$4.1295** across 20 files (hard ceiling $100; the tool's per-file gate is $5).

## No-match check (decision 0032, dated note 2026-10-01)

Pre-repair no-match rate: files whose first decoded mapping matched no sampled record, over files where the check ran (`no_match.first` set).
Post-repair zero rate: files whose final mapping matches no sampled record, over files that ended with a mapping (`no_match.after` set).

| arm | stream | pre-repair no-match | repair calls | post-repair zero |
|---|---|---|---|---|
| h-s2 | obf | 0 of 5 | 0 | 0 of 5 |
| h-s2 | private | 1 of 5 | 1 | 0 of 5 |
| b3 | obf | 1 of 5 | 1 | 0 of 5 |
| b3 | private | 0 of 5 | 0 | 0 of 5 |

## Failed probes (no file written)

Six clean-session probes answered something other than `none`, so those runs wrote no file. Each one was re-run once under the same replicate number, as ruling Q7 allows (up to 3 runs). Every re-run passed its probe. No stream reached 3 failures, so no stream is unmeasurable. Every failed reply named only the context that the CLI attaches to each prompt: a `userEmail` reminder and a `gitStatus` reminder for the empty scratch directory. No failed reply named the corpus, an instruction file or memory. Passing probes saw the same attachments and did not mention them, so a `none` reply shows the model reported nothing. It does not show the session was empty (follow-up: s2w#431).

| arm | corpus | K | probe charge $ |
|---|---|---|---|
| h-s2 | `obf-r2-dev` | r2 | 0.0143 |
| b3 | `obf-r3-dev` | r3 | 0.0160 |
| b3 | `obf-r5-dev` | r5 | 0.0129 |
| b3 | `private-dev-2` | r3 | 0.0150 |
| b3 | `private-dev-2` | r4 | 0.0139 |
| h-s2 | `private-dev-2` | r6 | 0.0138 |

Failed-probe spend: $0.0859. This spend is in no committed file. **Run total: $4.2154** ($4.1295 committed + $0.0859 failed probes). The plan estimated $10–30.

The runner also made 10 earlier attempts that the tool refused before any call. Those attempts spent $0. The shared OAuth access token had less than the 90 minutes a run needs, so the tool refused rather than refresh the token inside the scratch copy (lifeos#1252). The runs started after a normal session refreshed the token. No run printed the credentials-refresh warning.

## Notes

- **Private replicates differ only by model sampling.** The h-s2 sample is the newest 60 events of the window, so all five private h-s2 prompts are identical. Each b3 sample is sized from its h-s2 file's T. Where two h-s2 files report the same T (r2/r3 and r4–r6), the b3 fits match. The obfuscated replicates also differ by obfuscation key.
- **Reported model:** every call in every file reported `claude-sonnet-5-5`, the configured snapshot. No replicate is unmeasurable under §A8.
- **No failures:** all 20 files hold a mapping. Each took 1 attempt. No file has a `budget`, `budget-fit`, `invalid` or provider failure.
