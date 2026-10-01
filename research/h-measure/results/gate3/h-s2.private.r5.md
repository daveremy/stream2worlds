# h-measure score

A v0 stream mapping cannot say that different values name one entity; a version-2 mapping can, with links (decision 0027). A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the oracle-v0 mapping for that key), its ceiling-with-links row (that mapping plus a rule per alias path, linked into the oracle rule on the same identity, the ceiling for a mapping with links) and the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/committed/h-s2.private-dev-2.r5.json (sha256 a6388c384a9cb8d0c312cfb529eb1d625fb80c8260189037d90b2d595cfa4b13): profiler 9 proposed a mapping from the first 8072 events of private-dev-2 (sha256 d3f91c4dae1fba3ddc86872cc2879d640638fdd6fb059dc91de78fe965ef0766); it abstained on 1 paths. Scored on private-test-2 (sha256 4314ec45a06c44fc77d9d25a9c856c8ddda948ac17f86bce8cd127c1478dccc5), 5524 records.

System 2 (arm h-s2, replicate 5, model claude-sonnet-5-5) started from the heuristic above and proposed a mapping, $0.1651 by the price table; its first valid mapping matched no sampled record and was repaired once; the committed mapping matches. Its output is what is graded below.

## Key private-key-v2.json (base, sha256 2f1b2db99692ab1867cb9204092d87e243b10d96c0eec3af6fd532e3f0dba3da)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 1.0000 | 0.5346 | 0.6968 | 0.0000 | 0.5563 |
| mapping, without singleton-only types ["comment"] | 1.0000 | 0.5525 | 0.7118 |  |  |
| ceiling | 1.0000 | 0.8053 | 0.8921 | 0.0000 | 0.8904 |
| ceiling, without singleton-only types ["comment"] | 1.0000 | 0.7988 | 0.8881 |  |  |
| ceiling with links | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| branch | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| comment | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| commit | 1.0000 | 0.8843 | 0.9386 | 1.0000 | 1.0000 | 1.0000 |
| item | 1.0000 | 0.4516 | 0.6222 | 1.0000 | 0.7210 | 0.8379 |
| seat | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| sprint | undefined | 0.0000 | undefined | 1.0000 | 0.1111 | 0.2000 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.branch | 140 | 0 | 0.0000 | 1.0000 |
| data.comment_id | 391 | 0 | 0.0000 | 1.0000 |
| data.commit_sha | 830 | 830 | 0.9616 | 1.0000 |
| data.file_id | 23 | 0 | 0.0000 | 0.0000 |
| data.head_ref | 163 | 0 | 0.0000 | 1.0000 |
| data.head_sha | 227 | 0 | 0.0000 | 1.0000 |
| data.issue | 1295 | 1295 | 0.6808 | 0.7755 |
| data.key | 1291 | 0 | 0.0000 | 0.0000 |
| data.merge_commit_sha | 163 | 163 | 0.9808 | 1.0000 |
| data.number | 3270 | 3270 | 0.6683 | 0.8686 |
| data.parents.0 | 877 | 877 | 0.9887 | 1.0000 |
| data.parents.1 | 5 | 0 | 0.0000 | 1.0000 |
| data.pr | 168 | 168 | 0.1569 | 0.9990 |
| data.ref_number | 847 | 0 | 0.0000 | 0.9071 |
| data.refs.0.number | 877 | 877 | 0.7022 | 0.8167 |
| data.refs.1.number | 293 | 0 | 0.0000 | 0.9151 |
| data.refs.2.number | 86 | 0 | 0.0000 | 0.9013 |
| data.refs.3.number | 36 | 0 | 0.0000 | 0.8837 |
| data.refs.4.number | 24 | 0 | 0.0000 | 0.9370 |
| data.refs.5.number | 15 | 0 | 0.0000 | 0.9039 |
| data.refs.6.number | 7 | 0 | 0.0000 | 0.9432 |
| data.refs.7.number | 5 | 0 | 0.0000 | 1.0000 |
| data.seat_id | 29 | 29 | 1.0000 | 1.0000 |
| data.sha | 977 | 977 | 0.9189 | 1.0000 |
| data.slot | 23 | 0 | 0.0000 | 0.0000 |
| data.sprint | 23 | 0 | 0.0000 | 0.3333 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| branch @ data.repo | 1 | 2 | 2 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| item @ data.ref_repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.0.repo | 3 | 6 | 153 | 1.0000 | 0.4574 | 0.6277 | 1.0000 | 0.6520 | 0.7894 |
| item @ data.refs.1.repo | 2 | 4 | 64 | 1.0000 | 0.3678 | 0.5378 | 1.0000 | 0.7431 | 0.8526 |
| item @ data.refs.2.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.3.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.4.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.5.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.6.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.7.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.repo | 1 | 2 | 6 | 1.0000 | 0.7000 | 0.8235 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 0 at 0 paths (most: []). Key abstained: {}. Excluded (no_identity): {}. Undecodable records: 0.

Relationships (contract B3, unique typed directed edges):

| row | key edges | predicted | TP | FP | FN | P | R | F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mapping | 5095 | 2126 | 1750 | 376 | 3345 | 0.8231 | 0.3435 | 0.4847 |
| ceiling | 5095 | 4822 | 4822 | 0 | 273 | 1.0000 | 0.9464 | 0.9725 |
| ceiling with links | 5095 | 5095 | 5095 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 |

| key edge type | key edges | aligned predicted type | TP | FP | FN | P | R | F1 | ceiling R |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| commit-names | 899 | commit → issue, references issue | 752 | 213 | 147 | 0.7793 | 0.8365 | 0.8069 | 1.0000 |
| cross-references | 847 | (unaligned) | 0 | 0 | 847 | undefined | 0.0000 | undefined | 1.0000 |
| has-comment | 391 | (unaligned) | 0 | 0 | 391 | undefined | 0.0000 | undefined | 1.0000 |
| head-branch | 163 | (unaligned) | 0 | 0 | 163 | undefined | 0.0000 | undefined | 1.0000 |
| head-commit | 164 | (unaligned) | 0 | 0 | 164 | undefined | 0.0000 | undefined | 1.0000 |
| leg-branch | 59 | (unaligned) | 0 | 0 | 59 | undefined | 0.0000 | undefined | 0.0000 |
| leg-commit | 95 | (unaligned) | 0 | 0 | 95 | undefined | 0.0000 | undefined | 0.0000 |
| leg-pr | 119 | issue → pull_request, has pull request | 119 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 | 0.0000 |
| merged-as | 163 | (unaligned) | 0 | 0 | 163 | undefined | 0.0000 | undefined | 1.0000 |
| names | 444 | (unaligned) | 0 | 0 | 444 | undefined | 0.0000 | undefined | 1.0000 |
| parent | 882 | commit → commit, has parent | 877 | 0 | 5 | 1.0000 | 0.9943 | 0.9972 | 1.0000 |
| references | 830 | (unaligned) | 0 | 0 | 830 | undefined | 0.0000 | undefined | 1.0000 |
| reviews-branch | 17 | (unaligned) | 0 | 0 | 17 | undefined | 0.0000 | undefined | 1.0000 |
| reviews-commit | 20 | (unaligned) | 0 | 0 | 20 | undefined | 0.0000 | undefined | 1.0000 |
| reviews-item | 2 | agent_seat → issue, works on | 2 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |

Unaligned predicted edge types (all false): commit → issue, merges (163). Predicted edges with a no-majority endpoint: 0. Dropped (unscored endpoint): 2442. Unobservable key rows: 0 (predicted edges dropped on them: 0). Key edge types with no edge in this corpus: none.


