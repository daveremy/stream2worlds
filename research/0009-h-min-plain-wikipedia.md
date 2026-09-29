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
   #244's change takes 4.*
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

