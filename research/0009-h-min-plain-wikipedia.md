# 0009: H-lite on plain Wikipedia `recentchange`, held out

- **Question:** how well does the heuristics arm's first slice identify entities on the plain
  Wikipedia stream, measured on data it never saw? Issue #56 (split from #4), decision
  [0010](../docs/decisions/0010-gate3-h-arm.md) "Revisit when: H-min is measured".
- **Feeds:** gate 3 (is there headroom for System 2 over H on identity?), #17 (which answer-key
  reading moves the number), System 2's mapping format (#245).
- **Date:** 2026-09-29. **Status:** plain Wikipedia is reported, never counted (contract B2.1).
  This is not a gate-3 pass or fail.
- **What was measured:** H-lite, `s2w-discover` at `PROFILER_VERSION` 2 (decision
  [0022](../docs/decisions/0022-discover-profiler.md) with its s2w#208 amendment). H-lite is
  H-min without containment (inclusion dependencies), so this number is labelled H-lite, not
  H-min. Containment is #244 and will be scored on the reserved corpus this note never opened.

## The limit to read first

A stream mapping v0 (decision 0021) cannot say that **different** values name one entity. Two
rules with one type label join only when their values are equal. The base answer key lists one
wiki four ways (`wiki` = `enwiki`, `server_name` and `meta.domain` = `en.wikipedia.org`,
`server_url` = `https://en.wikipedia.org`) and one page two ways (`title`, `title_url`). No v0
mapping can put all of a page's or a wiki's mentions in one cluster, so no v0 mapping, from
H-lite, System 2 or B3, can reach recall 1.0 on this key. Every score below is therefore read against
three references, not against 1.0 alone:

- **Ceiling:** the oracle-v0 mapping that `KeySpec::oracle()` derives from the key (one rule per
  mention rule whose path is in its identity; alias paths get no rule). It is a reference, not
  a proven upper bound: on `wiki` H-lite beats it (below). The generated reports' preamble
  calls it "the best v0 mapping for that key"; that wording is wrong and is tracked in #245.
  *Fixed 2026-09-29 (#245): the preamble now says "the oracle-v0 mapping for that key". The
  committed reports under `h-measure/results/` predate the fix and keep the old wording.*
- **Canonical-mention key:** the same key with the alias-only paths (`server_name`,
  `server_url`, `meta.domain`, `title_url`) unscored, so a perfect v0 mapping can score 1.0.
- **Without singleton-only types:** `event` (`meta.id`) and, on these corpora, `log` have one
  mention per entity. A mapping that correctly declines to mint event ids scores recall 0 on
  them (research 0002 §3). Contract B3 counts them; this row shows their weight.

## Method and hygiene

Pre-registered in the [#56 plan](https://github.com/daveremy/stream2worlds/issues/56#issuecomment-5892219272) and karpathy's rulings
([1](https://github.com/daveremy/stream2worlds/issues/56#issuecomment-5892231182), [2](https://github.com/daveremy/stream2worlds/issues/56#issuecomment-5894100984), [3](https://github.com/daveremy/stream2worlds/issues/56#issuecomment-5895163962)). The order is in the git log. Parent order, not timestamps, is the evidence: PR
#227's commits carry one rebased author time.

| Step | Commit |
|---|---|
| Held-out and reserved corpus sha256s committed, before any key or score | 2bf39ba (PR #227; squash-merged to main as 97a6fcc) |
| Answer keys written and pinned (`keys.toml`), after 2bf39ba | e023d69, 947219c (PR #227); format-1 keys for `log_id` 0 in f4775c6 (#231) and e8ac8fd (#236) |
| `freeze` and `score` merged | e8ac8fd (#236), 6d95644 (#237), eeeced7 (#242) |
| H-lite frozen on the development corpus, at N = 10^4 and 2×10^5 events | 5e74d8f, whose parent is eeeced7 |
| Held-out corpora scored, reports committed with this note | the next commit on this branch |

The git log proves that the frozen files were committed before the reports. It cannot prove
when a file was first read: the earliest provable point is the hash commit 2bf39ba, and git
cannot show the spans were unseen before it. The stronger argument is that the frozen mapping cannot depend on
the held-out spans: H-lite is deterministic, and `score` re-runs the freeze from the
development corpus with the scoring build and refuses a frozen file that differs (s2w#238).
The mapping is therefore a function of the development corpus, the profiler code and the
`Config`. The last commit to `crates/s2w-discover` is 32adce5 (#214, decision 0022's s2w#208
amendment, made for the demo box's memory budget). It is an ancestor of 2bf39ba, the hash
commit, and `git log 2bf39ba..eeeced7 -- crates/s2w-discover crates/s2w-model` is empty.

- **Corpora** (`research/h-measure/corpora.toml`): `mediawiki.recentchange`, all wikis,
  replayed from EventStreams history as SSE frames. Development: 200,000 events from
  2026-09-28T00:00Z. `heldout`: 100,000 events from 08:44:42Z. `heldout-2`: 100,000 events from
  17:15:00Z. Each starts 6 h of stream time after the previous one ends. The `reserved` corpus
  (100,000 events from 2026-09-29T02:15Z) was not opened.
- **Freeze:** `cargo xtask h-measure freeze --corpus dev --window N`, release build of
  `origin/main` at eeeced7, production `Config` (in the frozen file). N = 10^4 is the window
  `serve` runs and gives the headline. N = 2×10^5 gives the same eight entity rules with the
  same keys (attributes, relationships and abstained paths differ; relationships are not
  scored), and its reports are identical to the N = 10^4 reports below their header lines. All
  four reports are committed.
- **Score:** `cargo xtask h-measure score --mapping research/h-measure/frozen/h-lite-v2.dev-10000.json
  --corpus heldout --key dev-key-v1.json --key dev-key-v1.canonical-mention.json --key
  dev-key-v1.q1-no-wiki.json --key dev-key-v1.q3-rcid-scored.json --key
  dev-key-v1.q4-separate.json --key dev-key-v1.user-global.json`, and the same for
  `heldout-2`. Full reports: `research/h-measure/results/`. About 80 s and 1.4 GB per corpus.
- **Keys:** the format-1 set (`dev-key-v1*.json`), the only keys scored. The base key's
  `log_id` 0 mentions (AbuseFilter hits, no log row) are excluded: 1,221 on `heldout`, 1,127 on
  `heldout-2`.
- **H-lite is deterministic,** so there are no replicates. The spread is the two held-out spans:
  one number per span, n = 100,000 records each (897,806 and 867,373 key mentions under the base
  key). Two spans on one Monday give a range, not a confidence interval.

**What "held out" means here, stated plainly.** The two spans are held out from this
measurement only. H-lite's thresholds (decision 0022, `PROFILER_VERSION` 2) were set earlier by
the same project, on `page_change` fixtures (a different Wikimedia stream) and on the demo
box's memory budget (s2w#208). The answer key was written by the same project from the published
schema and #17's proposed answers, before any H-lite output on these corpora existed. The
authors know Wikipedia's schema, so author leak exists at the design level even though no
threshold or key was touched after the held-out hashes were committed.

## Result

Base key (`dev-key-v1.json`), mapping frozen at N = 10^4:

| Row | `heldout` P / R / F1 | `heldout-2` P / R / F1 | Entity recovery (`heldout`, `heldout-2`) |
|---|---|---|---|
| **H-lite** | 0.9727 / 0.1664 / **0.2843** | 0.9815 / 0.1722 / **0.2929** | 0.0000, 0.0000 |
| H-lite without singleton-only types | 0.9727 / 0.1889 / 0.3164 | 0.9815 / 0.1956 / 0.3262 | |
| Ceiling (oracle v0) | 1.0000 / 0.4153 / 0.5868 | 1.0000 / 0.3948 / 0.5661 | 0.1995, 0.2067 |
| Ceiling without singleton-only types | 1.0000 / 0.3364 / 0.5034 | 1.0000 / 0.3123 / 0.4760 | |
| Perfect (not reachable by any v0 mapping) | 1 / 1 / 1 | 1 / 1 / 1 | 1, 1 |

- **Identity F1: 0.284 and 0.293** (n = 2 spans, spread 0.009). The mention-weighted
  false-merge rate (1 − P) is 0.027 and 0.019. H-lite is precise and misses most mentions.
- **Against the ceiling:** H-lite reaches about half of the oracle-v0 F1 (0.284 / 0.587 and
  0.293 / 0.566).
- **Entity recovery is 0** on both spans: no key entity with two or more mentions has a
  predicted cluster holding 90% of its mentions at 90% purity. The oracle reaches 0.20 and 0.21.
  Under this key no v0 mapping can recover a page or a wiki entity: a v0 cluster id is a
  function of the values at one rule's key paths, and `title` and `title_url` (or the four wiki
  paths) hold different values. Logs are singletons here, so the recoverable entities are
  users and revisions (the oracle recovers all of them), and H-lite mints neither. So 0.20 and
  0.21 are the most any v0 mapping can reach on this key.

Canonical-mention key (alias paths unscored), both spans:

| Row | P | R | F1 | Entity recovery |
|---|---|---|---|---|
| H-lite | 0.0000 | 0.0000 | undefined | 0.0000 |
| Ceiling (oracle v0) | 1.0000 | 1.0000 | 1.0000 | 1.0000 |

This 0 / 0 row is expected ([ruling](https://github.com/daveremy/stream2worlds/issues/56#issuecomment-5895163962), item 5), and it is not the same finding as the
base key's low recall. H-lite keys `page` at `title_url` (with `meta.uri` sharing its label)
and `wiki` at `server_name` and `meta.domain`. Those are exactly the paths this variant does not
score, so every page and wiki mention H-lite predicts is dropped, and what remains is its
spurious `log_action` clusters (7,921 and 5,091 mentions), which set P to 0. The key-path choice
comes from decision 0022's name-free 1:1 merge, which keeps the class member with the most
aliases and demotes the others to attributes. A mapping that keyed pages on (`wiki`,
`namespace`, `title`) would score page recall 1.0 here. So H-lite's page and wiki clusters are right as
partitions of the alias mentions, and this variant, by construction, gives no credit for
clustering at an alias path.

### Where H-lite loses (base key, `heldout`; `heldout-2` is within 0.001 on every row)

| Type | H-lite P | H-lite R | Ceiling R | Why |
|---|---|---|---|---|
| page | 1.0000 | 0.2500 | 0.2500 | Keyed at `title_url`: the `title_url` half of each page's mentions (recall 0.4999 at that path), in one cluster. Equal to the ceiling. |
| wiki | 0.9976 | 0.2486 | 0.0625 | `server_name` and `meta.domain` hold equal values and share a label, so H-lite joins 2 of the 4 alias paths. The oracle keys only `wiki`, 1 of the 4 paths. H-lite beats the ceiling here. |
| user | undefined | 0 | 1.0 | No rule keys `user`; it appears only as an attribute of two rules. |
| revision | undefined | 0 | 1.0 | No rule keys `revision.new` or `revision.old`. |
| log | undefined | 0 | 1.0 | `log_id` abstained in the profile (`FewGroups` at N = 10^4, `GreyUniqueness` at 2×10^5). Singleton-only here. |
| event | undefined | 0 | 1.0 | `meta.id` is unique per event, so H-lite correctly mints no entity (research 0002 §3). Singleton-only. |

- **Spurious clusters:** 7,921 and 5,091 mentions at `log_action`, a small set of action names
  (36 values in the development corpus, most often `block`, `upload` and `hit`) that H-lite
  keys as an entity. No key type holds them. Four and five
  predicted mentions at `meta.domain` are canary events, which the key does not mention.
- The frozen file records roles only for abstained paths. `user` appears as an attribute of
  the `notify_url` and `parsedcomment` rules; the revision paths appear nowhere in it (not as a
  rule, an attribute or an abstained path). This note does not say which role they received. Finding that out uses the development corpus, not
  these spans (#244's hygiene rule).

### Context collisions (the composite-key sub-metric, unfloored)

Rows where two entities differ only in a context path (definition:
`research/h-measure/README.md`, "Context collisions"). F1, H-lite / ceiling:

| Row | `heldout` groups, entities | `heldout` F1 | `heldout-2` groups, entities | `heldout-2` F1 |
|---|---|---|---|---|
| page @ `wiki` | 185, 424 | 0.3991 / 0.4000 | 78, 172 | 0.3983 / 0.4000 |
| user @ `wiki` | 490, 1,185 | undefined (R 0) / 1.0 | 388, 917 | undefined (R 0) / 1.0 |
| revision @ `wiki` | 0 | undefined | 1, 2 | undefined (R 0) / 1.0 |

`page @ wiki` is the cross-wiki sub-metric: pages with the same namespace and title on
different wikis. H-lite never merges them (P 1.0), because `title_url` carries the wiki's
domain. Its F1 there is within 0.002 of the alias-limited ceiling. `user @ wiki` is undefined because
H-lite has no user type.

### Sensitivity to #17's readings (F1, `heldout` / `heldout-2`)

| Key | H-lite | Ceiling | What changes |
|---|---|---|---|
| base | 0.2843 / 0.2929 | 0.5868 / 0.5661 | |
| q1-no-wiki (page identity without `wiki`) | 0.2840 / 0.2928 | 0.5868 / 0.5661 | Almost nothing. Pages with one namespace and title on two wikis become one gold entity; H-lite keeps them apart (`title_url` carries the domain), which costs 0.0001 of recall on each span (F1 falls 0.0003 and 0.0001). |
| q3-rcid-scored (rcid is an entity) | 0.2600 / 0.2672 | 0.6421 / 0.6268 | H-lite mints no rcid entity, so recall falls. |
| q4-separate (four wiki types) | 0.3898 / 0.4004 | 0.9089 / 0.9054 | The wiki aliases vanish, so both rise (the page alias remains, so the ceiling is 0.91, not 1.0). H-lite's P falls to 0.65 to 0.66 because its `server_name` + `meta.domain` join is now a merge across two types. |
| user-global (user identity without `wiki`) | 0.2843 / 0.2929 | 0.5868 / 0.5661 | Nothing for F1: no user type. |
| canonical-mention | undefined / undefined | 1.0 / 1.0 | See above. |

The recovery row is 0 for H-lite under every key. The q4 reading moves the number most, which
is the alias limit again: the fewer aliases a key asks for, the closer a v0 mapping can get.

## Which row of #4's decision table applies

#4's plan §6 reads H-min's held-out identity F1 against four rows. **F1 is 0.28 to 0.29, below
0.75, and entity recovery is 0, below 0.60, so the row that applies is "entity recovery < 0.60:
H-min itself is too weak to be a meaningful baseline. Fix stage-4 thresholds or alias merging
before anything else."** The ≥ 0.85 row does not fire, so there is no escalation to Dave on
headroom.

Two qualifications:

1. The table was written before the alias limit was known. Under the base key the oracle-v0
   ceiling's recovery is 0.20, so no v0 mapping reaches the 0.60 recovery bar on this key.
   Under the canonical-mention key the bar is reachable (ceiling 1.0) and H-lite scores 0. The
   row applies under both keys, for different reasons.
2. The table's action points at the stages that fail. H-lite loses on types it never proposes
   (`user`, `revision`), on one spurious type (`log_action`), and on its choice of an alias
   path as the key. None is obviously a containment failure, though containment could plausibly
   propose a revision type (`revision.old` and `revision.new` draw on one id space). Whether
   #244 moves the row is measured on the reserved corpus.

What this does not say: H-lite is a lower bound on H (decision 0010). H-full adds containment,
composite keys, carry-over and name embeddings, and could score higher. H-lite sits well below the
oracle-v0 reference (F1 0.28 to 0.29 against 0.57 to 0.59). That is not a measure of gate-3
headroom: the v0 format limits System 2 as well, and H is not complete.

## Design implications

Each is a candidate; karpathy assigns the disposition.

1. **Containment (inclusion dependencies) as s2w-discover stage 5b, `PROFILER_VERSION` 3 =
   H-min proper,** decided from the development-window table and scored on the reserved corpus.
   → deferred: #244. *2026-09-29: `PROFILER_VERSION` 3 went to #250 PR 1 (addendum below);
   #244's change takes 4.* *2026-09-29: 4 went to #250 PR 2; #244's change took 5 and was scored on
   `reserved` (addendum below, dated in UTC).*
2. **The mapping format's alias limit is a finding for System 2's design,** not only for H: no
   v0 producer can join different values that name one entity, and the oracle ceiling is not
   the best v0 mapping (H-lite beats it on `wiki`). → deferred: #245.
3. **H-lite's stage-4 and 1:1-merge behaviour on this stream** (no `user` or `revision` type,
   `log_action` minted, the key placed at an alias path), which #4's table says to fix before
   comparing H with H+S2. → candidate: an issue scoped from the development corpus only, whose
   score uses the reserved corpus (shared with #244, or a second reserved span).
   *2026-09-29: #250. PR 1 fixes `log_action` (a second reserved span, `reserved-2`; addendum
   below); `user` is #250 PR 2; `revision` is #244; the alias key is #245.* *2026-09-29: PR 2
   keys `user` (`PROFILER_VERSION` 4, measured on a third span, `reserved-3`; second addendum
   below). Entity recovery stays below 0.60, so #4's decision-table row does not move.*
4. **Decision 0010's lower-bound reading** is unchanged by this measurement. → adopted: dated
   note on decision 0010 in this PR.

## Addendum 2026-09-29: implication 3, PR 1 (s2w#250, `PROFILER_VERSION` 3)

**Change.** Decision 0022's second 2026-09-29 amendment: a path that passes the entity test with
at most 32 values, whose repeated values each decide which optional paths their events carry, is
a `Category` and keys no type. On `dev` at N = 10^4 it removes the `log_action` type and nothing
else. At N = 2x10^5 `log_action` has 36 values, so the rule does not fire and the mapping is
v2's, rule for rule.

**Hygiene.** A second reserved span, `reserved-2` (10^5 events, 2026-09-29 11:15 to 14:15 UTC),
was captured and pinned (b6c4263) before the rule was written (ded59ae), and opened as held-out
(c2d6894) after the rule and its docs. v3 was frozen on `dev` after the opening (90ddc1e) with
the profiler unchanged since ded59ae; `score` re-derives each freeze, so the frozen files are the
rule's output on `dev`, not a fit. The predictions below were posted in the plan on #250
([comment](https://github.com/daveremy/stream2worlds/issues/250#issuecomment-5898060671),
20:21 UTC) before `reserved-2` was opened (c2d6894, 20:33 UTC). v2 was scored on the same span from a build of `main` @ 7481bbf
with only the `corpora.toml` flip applied. #244's `reserved` stays unopened. Reports:
`research/h-measure/results/h-lite-v{2,3}.dev-{10000,200000}.reserved-2.md`.

**Result** on `reserved-2`, base key `dev-key-v1.json`:

| mapping | P | R | F1 | false-merge | recovery | spurious mentions at `log_action` |
|---|---|---|---|---|---|---|
| v2, N = 10^4 | 0.9840 | 0.1716 | 0.2923 | 0.0160 | 0.0000 | 4,474 |
| **v3, N = 10^4** | **0.9987** | 0.1716 | **0.2929** | 0.0013 | 0.0000 | **0** |
| v2, N = 2x10^5 | 0.9840 | 0.1716 | 0.2923 | 0.0160 | 0.0000 | 4,474 |
| v3, N = 2x10^5 | 0.9840 | 0.1716 | 0.2923 | 0.0160 | 0.0000 | 4,474 |
| ceiling (oracle v0) | 1.0000 | 0.3973 | 0.5687 | 0.0000 | 0.1833 | |

The remaining 0.0013 of false merge is the `wiki` type (P 0.9980 in both versions). The two
2x10^5 reports are identical except for the mapping's name and hash. Canonical-mention key,
N = 10^4: v2 P 0.0000, R 0.0000; v3 predicts no mention at a scored path (P undefined), R 0.0000.

**Pre-registered predictions** (N = 10^4, base key unless stated):

| | prediction | measured | |
|---|---|---|---|
| P1 | recall v3 = recall v2 exactly | 0.1716 = 0.1716 | hit |
| P2 | precision v3 >= v2; `log_action` predicted mentions v2 > 0, v3 = 0 | 0.9987 >= 0.9840; 4,474 and 0 | hit |
| P3 | F1 v3 - v2 in [0.000, +0.020]; the decision-table row does not move | +0.0006; entity recovery still 0 | hit |
| P4 | canonical-mention: v2 P = 0, v3 no predicted mention at a scored path | P 0.0000; P undefined | hit |
| control | N = 2x10^5: rule does not fire, score equals v2's to 4 decimals | identical reports | hit |

**Reading.** This is a precision cleanup. It removes the one spurious type, it does not move
recall or entity recovery, and the row of #4's decision table that applies is unchanged
("entity recovery < 0.60"). The recall losses this report names (`user`, `revision`, the alias
key) remain: #250 PR 2, #244 and #245.

## Addendum 2026-09-29: implication 3, PR 2 (s2w#250, `PROFILER_VERSION` 4)

**Change.** Decision 0022's third 2026-09-29 amendment: a path that stage 4 rejects outright (not
grey, best informative dependent below 80%) is an `Entity` when at least 25% of its repeat groups
span a tenth of the window and a varying path is constant in at least 80% of its groups. On `dev`
at N = 10^4 it keys `user` and `log_params.filter` (a path the key does not score) and changes
nothing else. At N = 2x10^5 it keys `user` and `title`; `title` is `NoDependents` only at that
window (at 10^4 it merges 1:1 into the `title_url` class).

**Hygiene.** `heldout`, `heldout-2` and `reserved-2` had all been opened, so a third span,
`reserved-3` (10^5 events, 2026-09-29 17:15 to 20:15 UTC, the hour band `heldout-2` covers on
another day), was captured and pinned (dc22869, rebased as 09a848d; authored 22:11 UTC) before any
profiler change. The plan and predictions P1-P6 were posted on #250
([comment](https://github.com/daveremy/stream2worlds/issues/250#issuecomment-5900441731), 22:39 UTC).
Before the opening, the committed v3 frozen files were scored in sample on `dev` against the
prototype's v4 freeze to check P2's thresholds; they agreed, so no prediction was amended
([comment](https://github.com/daveremy/stream2worlds/issues/250#issuecomment-5900587850), 22:52 UTC).
The rule (8bce39c) and its docs (7482e71) were committed, then `reserved-3` was opened as held-out
(8068982, 22:55 UTC). v4 was frozen on `dev` after the opening (f701acc); its mappings equal the
prototype's in-sample freezes. No crate changed between the opening and the scores. One refactor followed the scores (abbb53c: each dependent measured once instead of twice); it changes no output: the `dev` profile and both v4 freezes re-derive byte-identical from it. v3 was scored on the same span
from a build of `main` @ 70122b8 with only the `corpora.toml` flip applied. No threshold was chosen
after any held-out score was seen. #244's `reserved` stays unopened. Reports:
`research/h-measure/results/h-lite-v{3,4}.dev-{10000,200000}.reserved-3.md`.

**Result** on `reserved-3`, base key `dev-key-v1.json`:

| mapping | P | R | F1 | false-merge | recovery | `user` P | `user` R |
|---|---|---|---|---|---|---|---|
| v3, N = 10^4 | 0.9988 | 0.1710 | 0.2920 | 0.0012 | 0.0000 | undefined | 0.0000 |
| **v4, N = 10^4** | 0.9912 | **0.2853** | **0.4431** | 0.0088 | **0.0420** | 0.9682 | 1.0000 |
| v3, N = 2x10^5 | 0.9853 | 0.1710 | 0.2914 | 0.0147 | 0.0000 | undefined | 0.0000 |
| v4, N = 2x10^5 | 0.9844 | 0.3425 | 0.5082 | 0.0156 | 0.0420 | 0.9682 | 1.0000 |
| ceiling (oracle v0) | 1.0000 | 0.3997 | 0.5712 | 0.0000 | 0.1917 | | |

`user` precision below 1.0 is the key's user identity, `(wiki, user)`: the mapping keys `user`
alone, so a user active on several wikis is one entity to it and several to the key. Under the
`user-global` key (identity `user` alone) `user` scores P 1.0000, R 1.0000 at both windows, and
the mapping P 0.9991 (10^4). Among the scored paths, v4 differs from v3 at N = 10^4 only in
predicting `data.user` (99,994 mentions); at N = 2x10^5 also `data.title`.

**How `score` counts the 2x10^5 `title` type.** The key's `page` mentions sit at two alias paths,
`title_url` and `title`. v3 and v4 key the `title_url` class; v4 at 2x10^5 also keys `title` as a
separate type. `score` counts each predicted mention against the key's page identity, so `page`
recall doubles (0.25 to 0.50, P 0.9991: equal titles on different wikis merge) although the
mapping never says the two types are one page. That is a second encoding of the page, the #245
alias-key finding, not evidence for this rule; it is also why the `page` row exceeds its type
ceiling (0.25, the best single v0 type).

**Pre-registered predictions** (N = 10^4, base key unless stated):

| | prediction | measured | |
|---|---|---|---|
| P1 | `user` R >= 0.95, `user` P in [0.85, 0.97] | R 1.0000, P 0.9682 | hit (P 0.002 inside the upper bound) |
| P2 | mapping R delta >= +0.08, F1 delta >= +0.10 | +0.1143, +0.1511 | hit |
| P3 | mapping P >= 0.95 and below v3's; no new spurious cluster at a scored path | 0.9912 < 0.9988; only `data.user` changes | hit |
| P4 | entity recovery > 0 and < 0.60 | 0.0420 | hit |
| P5 | `user-global`: `user` P >= 0.99, R >= 0.95 | 1.0000, 1.0000 | hit |
| P6 | canonical-mention: `user` P and R equal the base key's | 0.9682 / 1.0000 both | hit |
| control | N = 2x10^5: `user` R >= 0.95, P in [0.85, 0.97]; `page` R above v3's, `page` P in [0.95, 1.0); F1 delta >= +0.10 | 1.0000, 0.9682; 0.50 > 0.25, 0.9991; +0.2168 | hit |

`varies` (plan section 10 item 1) did not fail: `user` is keyed at both windows.

**Reading.** The rule recovers the `user` type on held-out data at the predicted precision; mapping
F1 rises from 0.29 to 0.44 at the frozen window. Entity recovery rises from 0 to 0.04 and stays
below 0.60, so the row of #4's decision table that applies is unchanged ("entity recovery <
0.60: fix H before comparing H with H+S2"). The remaining losses this report names are `revision`
(#244, now `PROFILER_VERSION` 5) and the alias key (#245).


## Addendum 2026-09-30 (UTC): implication 1, containment (s2w#244, `PROFILER_VERSION` 5)

**Change.** Decision 0022's stage-5b amendment (dated 2026-09-29) adds stage 5b: two `Entity` or `EventId` paths
share one value domain when at least 10% of one path's distinct values also appear at the other
(coverage) and at least 95% of the shared values appear at the other path first, in a strictly
earlier event (carry). The pair then keys one type. On `dev` it accepts exactly `revision.old` →
`revision.new`, at N = 10^4 and at 2x10^5; `revision.new` and `revision.old` are each unique per
event, so no earlier version keyed them. Nothing else changes in the mapping.

**Hygiene.** `reserved` (10^5 events, 2026-09-29 02:15 to 05:15 UTC, pinned in 2bf39ba before
any key or score) was the span this issue named, and neither #56 nor #250 had opened it. Whether
to build was read from the development-window table (disclosed, in sample: v4 → v5 on `dev`,
N = 10^4, R 0.2823 → 0.3674, F1 0.4383 → 0.5349). The rule (eb48f5b) and its docs (59173df)
were committed, and the predictions P1-P6 were posted on #244
([comment](https://github.com/daveremy/stream2worlds/issues/244#issuecomment-5902011361),
01:03 UTC). Two crate commits followed before the opening: a refine pass (2061aa2: each pair
measured once by a merge walk; the `dev` freezes at both windows and the 10^4 profile re-derive
byte-identical) and round-1 review fixes (762493a: a test and comments, no rule change). Then
`reserved` was opened as held-out (709923f, 01:18 UTC). v5 was frozen on `dev` after the opening
(0551300); the in-sample freezes made before the opening were not committed, and each differs
from the committed file only in the line that records `reserved`'s role. No crate changed between the
opening and the scores, and no threshold was chosen after any held-out score was seen.

v4 was scored from a build of `main` @ 455c547 with only the `corpora.toml` flip applied. This
deviates from the posted plan, which named v4's committed freezes: those freezes had recorded
`reserved`'s old role, so `score` refused them ("freeze again under the current pins"); v4 was
re-frozen on `dev` under the current pins
(`frozen/h-lite-v4.dev-{10000,200000}.pins-244.json`), and each file differs from the committed
v4 freeze only in that pin line. Reports:
`research/h-measure/results/{h-min-v5,h-lite-v4}.dev-{10000,200000}.reserved.md`.

*Note 2026-09-30 (s2w#277):* the re-freeze is no longer needed. `score` now accepts a corpus pin
whose role went from `reserved` to `heldout` with the same file, events and sha256, and still
refuses every other change. The original `frozen/h-lite-v4.dev-{10000,200000}.json`, scored on
`reserved` from 455c547 plus that fix, give reports that differ from the committed ones only in the
line naming the mapping file and its sha256. No number above moves.

**Result** on `reserved`, base key `dev-key-v1.json`:

| mapping | P | R | F1 | false-merge | recovery | `revision` P | `revision` R |
|---|---|---|---|---|---|---|---|
| v4, N = 10^4 | 0.9922 | 0.2866 | 0.4447 | 0.0078 | 0.0314 | undefined | 0.0000 |
| **v5, N = 10^4** | 0.9933 | **0.3646** | **0.5334** | 0.0067 | **0.1250** | 1.0000 | 1.0000 |
| v4, N = 2x10^5 | 0.9856 | 0.3440 | 0.5100 | 0.0144 | 0.0314 | undefined | 0.0000 |
| v5, N = 2x10^5 | 0.9873 | 0.4220 | 0.5912 | 0.0127 | 0.1250 | 1.0000 | 1.0000 |
| ceiling (oracle v0) | 1.0000 | 0.3973 | 0.5687 | 0.0000 | 0.1320 | | |

Without the singleton-only types (`event`, `log`), mapping F1 is 0.4898 → 0.5840 at 10^4 and
0.5591 → 0.6448 at 2x10^5 (ceiling 0.4806). Under the canonical-mention key, mapping F1 is
0.3484 → 0.5231 and recovery 0.1290 → 0.5130 at 10^4; at 2x10^5, F1 0.5898 → 0.7182 and recovery
0.5701 → 0.9541. Under `user-global`, F1 is 0.4455 → 0.5342 at 10^4. Among the scored paths, v5
differs from v4 only in predicting `data.revision.new` (36,494 mentions) and `data.revision.old`
(31,399), at both windows.

v5's 2x10^5 mapping F1 (0.5912) exceeds the oracle-v0 ceiling (0.5687) for the reason the PR 2
addendum gives: at that window the mapping also keys `title` as a type separate from the
`title_url` class, and `score` counts both encodings of the page against one identity (`page` R
0.50 against a type ceiling of 0.25). That is the #245 alias-key finding, not evidence for 5b.

**Context collisions.** `revision @ data.wiki`: 2 groups, 4 mentions, P 0.5000, R 1.0000 (v4:
R 0). The mapping keys the revision number alone, so equal revision numbers on two wikis merge;
the key's identity separates them. The merge touches 4 of 67,893 revision mentions, and the
type-level P is at least 0.9999 (it prints as 1.0000).

**Pre-registered predictions** (N = 10^4, base key unless stated):

| | prediction | measured | |
|---|---|---|---|
| P1 | `revision` R >= 0.95 and P >= 0.99 | R 1.0000, P 1.0000 | hit |
| P2 | mapping R delta >= +0.06, F1 delta >= +0.07 | +0.0780, +0.0887 | hit |
| P3 | mapping P >= v4's P − 0.01; false-merge <= 0.02; only `data.revision.new`/`old` change predicted count | 0.9933 (v4 0.9922); 0.0067; only those two | hit |
| P4 | entity recovery in [0.10, 0.30], below 0.60 | 0.1250 | hit |
| P5 | canonical-mention recovery in [0.45, 0.70] (crossing 0.60 not predicted) | 0.5130 | hit |
| P6 | `revision @ data.wiki`: groups > 0 and P < 1 | 2 groups, P 0.5000 | hit |
| control | N = 2x10^5: `revision` R >= 0.95; F1 delta >= +0.07 | 1.0000; +0.0812 | hit |

**Reading.** Stage 5b recovers the `revision` type on held-out data at full recall and
precision; mapping F1 rises from 0.44 to 0.53 at the frozen window, with precision unchanged.
Entity recovery rises from 0.03 to 0.13 and stays below 0.60 under the base key, so the row of #4's
decision table that applies is unchanged ("entity recovery < 0.60: fix H before comparing H with
H+S2"). This is H-min proper as #4 and decision 0010 define it; the remaining named loss is the
alias key (#245).

## Addendum 2026-09-30 (UTC): date-times are a format (s2w#291 PR 2, `PROFILER_VERSION` 7)

**Change.** Decision [0030](../docs/decisions/0030-timestamps-are-a-format.md): a path whose
every value is an RFC 3339 `date-time` gets the role `Timestamp`. It keys no type and is no
stage-5b candidate, but can still be an attribute. Check 12 shifts date-times by one constant
instead of hashing them, as the evaluation contract does.

**Why (#291 item 5).** The demo's date-time types reproduce only on live `page_change` logs
captured by `s2w watch` on 2026-09-27 (21,528 events in one run, 5,232 in a second), never on
the recorded page-change fixture (11,667 events). At the serve window (10^4), v6 keys a type by
the prior-state editor's `first_edit_dt` in events 0-10k and in the second run, and by that and
`revision.rev_dt` in events 10k-20k. A date-time passes the entity test and merges into the
user's class only while it is one-to-one with the user id; over 10^4 events two users share a
second (5 to 6 date-times) and one user's date-time differs between wikis (1 to 3), so it keeps
its own type. On the same windows v7 keys no type by a date-time. Entity rules go 25 → 25,
24 → 23 and 18 → 18, and relationship rules 77 → 77, 93 → 80 and 45 → 45. The date-time's
class is replaced by the prior-state editor's `user_id`, which v6 had merged into it. These are
in-sample reads on data that is not a scored corpus.

**Hygiene.** `reserved-4` (10^5 `recentchange` events, 2026-09-29 23:15 to 2026-09-30 02:15 UTC,
capture.sh's command) was pinned with role `reserved` in cf9c864 (06:02 UTC); only its summary
line, size and sha256 were read. v6 was frozen on `dev` at both windows in a280af2 (06:03), before
any PR 2 profiler change. The predictions were posted on #291
([comment](https://github.com/daveremy/stream2worlds/issues/291#issuecomment-5905284910), 06:13:40)
before the rule commit (a99f948, 06:13:57), with the in-sample `dev` reads disclosed: v7's `dev`
freezes equal v6's except `profiler_version`, and the only `dev` role change is `data.meta.dt`
(`NearUnique` → `Timestamp`, at both windows; re-measured after the opening, unchanged). The
tests (0f224b5), check 12 (35f0bdc) and docs (974d6b8) followed with no profiler change, and
`reserved-4` was opened as held-out in b99c729 (06:25). v7 was frozen on `dev` after the opening;
each v7 file differs from v6's only in `profiler_version` and the line that records
`reserved-4`'s role. v6 was scored from a build of a280af2 plus the opening commit. No threshold
exists to tune, and none changed.

**Result** on `reserved-4`. Every row of every report is the same for v6 and v7; the reports
differ only in the line naming the mapping file and its sha256.

| key, window | P | R | F1 | false-merge | recovery | `user` P | `user` R |
|---|---|---|---|---|---|---|---|
| base, N = 10^4 | 0.9944 | 0.3684 | 0.5376 | 0.0056 | 0.1281 | 0.9761 | 1.0000 |
| base, N = 2x10^5 | 0.9875 | 0.4254 | 0.5946 | 0.0125 | 0.1281 | 0.9761 | 1.0000 |
| user-global, N = 10^4 | 0.9994 | 0.3684 | 0.5383 | 0.0006 | 0.1315 | 1.0000 | 1.0000 |
| user-global, N = 2x10^5 | 0.9916 | 0.4254 | 0.5953 | 0.0084 | 0.1315 | 1.0000 | 1.0000 |
| ceiling (oracle v0), base | 1.0000 | 0.4015 | 0.5730 | 0.0000 | 0.1347 | | |

**Page-change memory** (`backfill_memory`'s `world` child, the recorded page-change fixture cycled
to 1.5x10^5, mapping discovered at 10^4; both builds measured on this base):

| | world events | head world | entities | types | entity rules | relationship rules |
|---|---|---|---|---|---|---|
| v6 (a280af2) | 14,323,096 | 596.7 MiB | 213,153 | 13 | 27 | 116 |
| v7 | 14,323,096 | 599.0 MiB | 213,153 | 13 | 27 | 116 |

The mapping differs by four attribute rules: `first_edit_dt` (the performer's and the revision
editor's) becomes an attribute of the two `origin_rev_id` types as well. They add 2.3 MiB (0.4%)
and no world event.

**Pre-registered predictions:**

| | prediction | measured | |
|---|---|---|---|
| R1 | v7's P, R, F1, false-merge and recovery equal v6's exactly, under every key scored | equal under both keys, both windows | hit |
| R2 | `user` R >= 0.95 (expected 1.0000), `user` P equal to v6's, both windows | R 1.0000, P 0.9761 (v6 0.9761) | hit |
| R3 | `data.log_params.img_timestamp` stays an `Entity` type in v6 and v7 | a type in both, both windows | hit |
| W1 | no stamp-keyed type, 13 types, world events and MiB within ±2% of v6 (posted: 14,323,096 events, 596.8 MiB) | no stamp-keyed type, 13 types, 14,323,096 events (+0%), 599.0 MiB (+0.4%) | hit |

**Reading.** The rule removes the demo's date-time types on live page-change windows and changes
nothing that `recentchange` scores: its only date-time path (`meta.dt`) was never a type. `user`
keeps full recall. MediaWiki's 14-digit `img_timestamp` is still a type; decision 0030 names
that as a limit.

## Addendum 2026-09-30 (UTC): an integer key must come back (s2w#327, `PROFILER_VERSION` 8)

**Change.** Decision 0022's s2w#327 amendment: a second-test key whose every value is an integer
must come back under some follower (fewer than `return_pct`, 30, of its counted changes
superseded under at least one follower with `min_support` of them). It removes the recorded
page-change fixture's `mediainfo/content_size` type. `revision/comment` stays, as the ruling on
#327 accepts; a floor on every kind would also drop `dev`'s `title` at 2x10^5 (the decision has
the numbers).

**Hygiene.** No corpus was left unopened, so this is a no-loss check on spans already opened, not
a fresh held-out test. Leg A's in-sample reads (fixture and `dev` only) found no integer key on
`dev` that passes only the second test. The predictions were posted on #327
([comment](https://github.com/daveremy/stream2worlds/issues/327#issuecomment-5911112660), 12:15:39)
before the rule's first run (8638bb5, 12:21). v8 was then frozen on `dev` at both windows, and
each file differs from v7's only in `profiler_version` and the config text (the new field).

**Result.** On `reserved-4`, every row of every report equals v7's under the base and
`user-global` keys, at both windows; the reports differ only in the line naming the mapping file.
The mappings are the same, so the other spans' scores are v7's too. v8 on every span (base key
unless named):

| span | N | base P | base R | base F1 | `page` R | `user` R | user-global R |
|---|---|---|---|---|---|---|---|
| `heldout` | 10^4 | 0.9881 | 0.3793 | 0.5482 | 0.2500 | 1.0000 | 0.3793 |
| `heldout` | 2x10^5 | 0.9765 | 0.4350 | 0.6019 | 0.5000 | 1.0000 | 0.4350 |
| `heldout-2` | 10^4 | 0.9910 | 0.3606 | 0.5288 | 0.2500 | 1.0000 | 0.3606 |
| `heldout-2` | 2x10^5 | 0.9835 | 0.4182 | 0.5869 | 0.5000 | 1.0000 | 0.4182 |
| `reserved` | 10^4 | 0.9933 | 0.3646 | 0.5334 | 0.2500 | 1.0000 | 0.3646 |
| `reserved` | 2x10^5 | 0.9873 | 0.4220 | 0.5912 | 0.5000 | 1.0000 | 0.4220 |
| `reserved-2` | 10^4 | 0.9912 | 0.3641 | 0.5326 | 0.2500 | 1.0000 | 0.3641 |
| `reserved-2` | 2x10^5 | 0.9847 | 0.4215 | 0.5903 | 0.5000 | 1.0000 | 0.4215 |
| `reserved-3` | 10^4 | 0.9925 | 0.3670 | 0.5358 | 0.2500 | 1.0000 | 0.3670 |
| `reserved-3` | 2x10^5 | 0.9863 | 0.4241 | 0.5932 | 0.5000 | 1.0000 | 0.4241 |
| `reserved-4` | 10^4 | 0.9944 | 0.3684 | 0.5376 | 0.2500 | 1.0000 | 0.3684 |
| `reserved-4` | 2x10^5 | 0.9875 | 0.4254 | 0.5946 | 0.5000 | 1.0000 | 0.4254 |

**Page-change memory** (`backfill_memory`'s `world` child, the recorded fixture cycled to
1.5x10^5, mapping discovered at 10^4; "floor off" is this build with `return_pct` 101, v7's rule):

| | world events | head world | entities | types |
|---|---|---|---|---|
| floor off (v7's rule) | 14,323,096 | 599.2 MiB | 213,153 | 13 |
| v8 | 14,264,996 | 597.7 MiB | 212,633 | 12 |

One pass over the fixture's 11,667 events gives 1,109,478 world events, down from 1,113,978
(-4,500, 0.40%, the size's share).

**Pre-registered predictions:**

| | prediction | measured | |
|---|---|---|---|
| 1 | the discovered-types baseline loses exactly `mediainfo/content_size` | exactly that line | hit |
| 2 | world events on the fixture drop by about 0.4% | -0.40% (4,500 of 1,113,978) | hit |
| 3 | v8's `dev` freezes equal v7's except version and config text | equal, both windows | hit |
| 4 | every score on every opened span equals v7's; no recall loss | `reserved-4` reports equal; the other spans follow from 3 | hit |
| 5 | check 12 still passes | passes | hit |

**Reading.** The rule removes the one integer-valued false type and changes nothing that
`recentchange` scores. Free text is the open half: s2w#333.

## Addendum 2026-09-30: what changes on the obfuscated stream (s2w#370 PR 2)

Replicate r1 obfuscates `dev` under contract B2.2 (rules:
`research/h-measure/obfuscation/recentchange.rules.toml`). Two things make its numbers differ from
this note's by design, not by H-lite:

1. **Plain and obfuscated ceilings differ.** `server_name` and `server_url` are hashed from the
   record's `wiki` value (§B2.2: aliases "hashed the same way"), so on the obfuscated stream they are
   byte-equal to `wiki` and the v0 alias limit does not bite on them. `meta.domain` is too, with one
   exception, the canary rule: canary events carry `meta.domain` and no `wiki`, so on those events
   it hashes its own value in the wiki domain, and `obfuscation/r1.meta.json` counts them under
   `own_values` (karpathy ruling, s2w#370 2026-09-30). Compare an obfuscated score with an
   obfuscated ceiling, never with the plain one.
2. **Folded types have no context-collision rows.** `title`, `revision` and `page_id` fold `wiki`
   into the hash (#17), so one name on two wikis gets two hashes and those types form no collision
   groups. Context collisions are a reported diagnostic, not a §B3 scored metric; identity F1,
   false merges and recovery are unaffected (`h_measure/obfuscate/tests.rs`, test 9).

## Addendum 2026-10-01 (UTC): links (s2w#245 PR 4, `PROFILER_VERSION` 9)

**Change.** Decision 0022's s2w#245 amendment: stage 6 keeps a 1:1 merge's losers as key paths and
links them into the class with the most distinct values, so the mapping is version 2 (decision
[0027](../docs/decisions/0027-stream-mapping-links.md)). This is the first H change the oracle-v0
reference does not bound: a mapping with links reads against the "ceiling with links" row. `serve`
still auto-applies version 1 only, so the links are measured here and not yet served.

**Hygiene.** `reserved-5` was pinned (2b7e4f1) before the profiler change and opened (49aefe1,
role `heldout`) only after v9 was frozen on `dev` at both windows (e43152d). The predictions were
posted on #245
([comment](https://github.com/daveremy/stream2worlds/issues/245#issuecomment-5927730621), about
08:35Z) after an in-sample check on `dev` and before `reserved-5` was read. Its amendments to the
plan's §5 came from that check only. v8 is the saved build `xtask-v8-1e26249` scoring its own
`dev` freezes. The freezes' links: at 10^4, `data.server_url` → `data.meta.domain`, `data.title` →
`data.meta.uri` and `data.comment` → `data.parsedcomment`; at 2x10^5, `server_url` →
`meta.domain` and `comment` and `log_action_comment` → `parsedcomment`, with **no `title` link**.

**Result** (`reserved-5`, 100,000 events, base key `dev-key-v1`; reports in
`h-measure/results/h-min-v{8,9}.dev-{10000,200000}.reserved-5.md`):

| | N | P | R | F1 | recovery | `page` P / R | `wiki` P / R |
|---|---|---|---|---|---|---|---|
| v8 | 10^4 | 0.9924 | 0.3619 | 0.5303 | 0.1369 | 1.0000 / 0.2500 | 0.9987 / 0.2492 |
| v9 | 10^4 | 0.9943 | 0.6781 | 0.8063 | 0.9807 | 0.9994 / 0.9987 | 0.9987 / 0.5607 |
| v8 | 2x10^5 | 0.9865 | 0.4195 | 0.5887 | 0.1369 | 0.9995 / 0.5000 | 0.9987 / 0.2492 |
| v9 | 2x10^5 | 0.9824 | 0.5631 | 0.7159 | 0.1369 | 0.9995 / 0.5000 | 0.9987 / 0.5607 |

`user` P is 0.9675 in every row. Under `user-global`, v9 at 10^4 reads P 0.9992, R 0.6781, F1
0.8079. Under the canonical-mention key, v9 at 10^4 has `page` P 0.9991 and R 1.0000 (v8: R 0)
and `wiki` R 0.

**The ceiling with links, per type** (the PR #377 deferred concern). The row is the same for both
builds and both windows, as it should be (it does not read the mapping): P 0.9993, R 0.9989, F1
0.9991. Per type, from the score's JSON output (`grade.ceiling_links.per_type`):

| type | P | R |
|---|---|---|
| `event`, `log`, `page`, `revision`, `user` | 1.0000 | 1.0000 |
| `wiki` | 0.9985 | 0.9976 |

So the whole gap below 1.0 is `wiki`. Its four key paths (`data.meta.domain`, `data.server_name`,
`data.server_url`, `data.wiki`) are aliases in almost every event; per path, the oracle's recall
is 0.9972 on the first three and 0.9988 on `data.wiki`. Under the canonical-mention key the
ceiling with links is 1.0000 / 1.0000 for every type.

**Pre-registered predictions** (N = 10^4 unless named):

| | prediction | measured | |
|---|---|---|---|
| P1 | `wiki` R in [0.53, 0.60]; `wiki` P ≥ 0.995 | R 0.5607, P 0.9987 | hit |
| P2 | `page` R in [0.93, 1.00]; `page` P in [0.95, 0.99] | R 0.9987; P 0.9994 | **miss** (P above the band) |
| P3 | R rises by [+0.25, +0.36]; P ≥ 0.95; F1 delta ≥ +0.18 | +0.3162; 0.9943; +0.2760 | hit |
| P4 | recovery ≥ 0.60 | 0.9807 | hit |
| P5 | grey band: F1 in [0.75, 0.85), recovery ≥ 0.60; no escalation | F1 0.8063, recovery 0.9807 | hit |
| P6 | canonical-mention: `page` R ≥ 0.90; `page` P < 1.0; `wiki` R = 0 | 1.0000; 0.9991; 0 | hit |
| P7 | ceiling with links: R ≥ 0.99, P ≥ 0.999; every type but `wiki` at 1.0000 | 0.9989, 0.9993; yes | hit |
| P8 | 2x10^5: `page` R = v8's ± 0.01; recovery = v8's ± 0.01; `wiki` R in [0.53, 0.60]; F1 delta in [+0.08, +0.16] | 0.5000 (v8 0.5000); 0.1369 (0.1369); 0.5607; +0.1272 | hit |
| control | v8's ceiling-with-links row equals v9's | equal | hit |

P2 misses on the safe side: the in-sample check read `page` P 0.9808, and `reserved-5` reads
0.9994. The band was set from earlier spans and was not revised. A miss is a finding, not a
reason to retune; no threshold moved after `reserved-5` opened.

**Which row of #4's decision table applies.** At 10^4 (the window `serve` uses), F1 0.8063 and
recovery 0.9807: the 0.75 to 0.85 grey band. F1 is below 0.85, so there is no escalation to Dave.
At 2x10^5 the row is unchanged from v8's ("entity recovery < 0.60"), because that freeze has no
`title` link and recovery stays 0.1369.

**Reading.** Links close most of the alias gap that the v0 format imposed: base-key recall at
10^4 goes from 0.3619 to 0.6781, and the `page` type, written two ways, is recovered almost
entirely. `data.wiki`, one of `wiki`'s four key paths, gets no rule (0 of its 99,995 mentions predicted), so `wiki` stays at R 0.56. The
2x10^5 freeze loses the `title` link, and with it the gain on `page`: the window rule decides
which merges happen, and this note does not resolve that. The links cost 75 MiB on the
page-change memory bound and are not served yet (decision 0022's s2w#245 amendment, s2w#392).
