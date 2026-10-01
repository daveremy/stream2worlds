# h-measure score

A v0 stream mapping cannot say that different values name one entity; a version-2 mapping can, with links (decision 0027). A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the oracle-v0 mapping for that key), its ceiling-with-links row (that mapping plus a rule per alias path, linked into the oracle rule on the same identity, the ceiling for a mapping with links) and the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/frozen/h-min-v9.private-dev-2-8072.json (sha256 0bd923fc923b90552dd5cedfa683ea31a7f1621ec80b97ca5159768143fb0dcf): profiler 9 proposed a mapping from the first 8072 events of private-dev-2 (sha256 d3f91c4dae1fba3ddc86872cc2879d640638fdd6fb059dc91de78fe965ef0766); it abstained on 1 paths. Scored on private-test-2 (sha256 4314ec45a06c44fc77d9d25a9c856c8ddda948ac17f86bce8cd127c1478dccc5), 5524 records.

## Key private-key-v2.json (base, sha256 2f1b2db99692ab1867cb9204092d87e243b10d96c0eec3af6fd532e3f0dba3da)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 1.0000 | 0.0022 | 0.0044 | 0.0000 | 0.0000 |
| mapping, without singleton-only types ["comment"] | 1.0000 | 0.0023 | 0.0045 |  |  |
| ceiling | 1.0000 | 0.8053 | 0.8921 | 0.0000 | 0.8904 |
| ceiling, without singleton-only types ["comment"] | 1.0000 | 0.7988 | 0.8881 |  |  |
| ceiling with links | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| branch | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| comment | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| commit | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| item | 1.0000 | 0.0032 | 0.0064 | 1.0000 | 0.7210 | 0.8379 |
| seat | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| sprint | undefined | 0.0000 | undefined | 1.0000 | 0.1111 | 0.2000 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.branch | 140 | 0 | 0.0000 | 1.0000 |
| data.comment_id | 391 | 0 | 0.0000 | 1.0000 |
| data.commit_sha | 830 | 0 | 0.0000 | 1.0000 |
| data.file_id | 23 | 0 | 0.0000 | 0.0000 |
| data.head_ref | 163 | 0 | 0.0000 | 1.0000 |
| data.head_sha | 227 | 0 | 0.0000 | 1.0000 |
| data.issue | 1295 | 0 | 0.0000 | 0.7755 |
| data.key | 1291 | 0 | 0.0000 | 0.0000 |
| data.merge_commit_sha | 163 | 0 | 0.0000 | 1.0000 |
| data.number | 3270 | 0 | 0.0000 | 0.8686 |
| data.parents.0 | 877 | 0 | 0.0000 | 1.0000 |
| data.parents.1 | 5 | 0 | 0.0000 | 1.0000 |
| data.pr | 168 | 168 | 0.1569 | 0.9990 |
| data.ref_number | 847 | 0 | 0.0000 | 0.9071 |
| data.refs.0.number | 877 | 0 | 0.0000 | 0.8167 |
| data.refs.1.number | 293 | 0 | 0.0000 | 0.9151 |
| data.refs.2.number | 86 | 0 | 0.0000 | 0.9013 |
| data.refs.3.number | 36 | 0 | 0.0000 | 0.8837 |
| data.refs.4.number | 24 | 0 | 0.0000 | 0.9370 |
| data.refs.5.number | 15 | 0 | 0.0000 | 0.9039 |
| data.refs.6.number | 7 | 0 | 0.0000 | 0.9432 |
| data.refs.7.number | 5 | 0 | 0.0000 | 1.0000 |
| data.seat_id | 29 | 0 | 0.0000 | 1.0000 |
| data.sha | 977 | 0 | 0.0000 | 1.0000 |
| data.slot | 23 | 0 | 0.0000 | 0.0000 |
| data.sprint | 23 | 0 | 0.0000 | 0.3333 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| branch @ data.repo | 1 | 2 | 2 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| item @ data.ref_repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.0.repo | 3 | 6 | 153 | 1.0000 | 0.0007 | 0.0013 | 1.0000 | 0.6520 | 0.7894 |
| item @ data.refs.1.repo | 2 | 4 | 64 | 1.0000 | 0.0016 | 0.0031 | 1.0000 | 0.7431 | 0.8526 |
| item @ data.refs.2.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.3.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.4.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.5.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.6.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.7.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.repo | 1 | 2 | 6 | 1.0000 | 0.1667 | 0.2857 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 0 at 0 paths (most: []). Key abstained: {}. Excluded (no_identity): {}. Undecodable records: 0.

Relationships (contract B3, unique typed directed edges):

| row | key edges | predicted | TP | FP | FN | P | R | F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mapping | 5095 | 0 | 0 | 0 | 5095 | undefined | 0.0000 | undefined |
| ceiling | 5095 | 4822 | 4822 | 0 | 273 | 1.0000 | 0.9464 | 0.9725 |
| ceiling with links | 5095 | 5095 | 5095 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 |

| key edge type | key edges | aligned predicted type | TP | FP | FN | P | R | F1 | ceiling R |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| commit-names | 899 | (unaligned) | 0 | 0 | 899 | undefined | 0.0000 | undefined | 1.0000 |
| cross-references | 847 | (unaligned) | 0 | 0 | 847 | undefined | 0.0000 | undefined | 1.0000 |
| has-comment | 391 | (unaligned) | 0 | 0 | 391 | undefined | 0.0000 | undefined | 1.0000 |
| head-branch | 163 | (unaligned) | 0 | 0 | 163 | undefined | 0.0000 | undefined | 1.0000 |
| head-commit | 164 | (unaligned) | 0 | 0 | 164 | undefined | 0.0000 | undefined | 1.0000 |
| leg-branch | 59 | (unaligned) | 0 | 0 | 59 | undefined | 0.0000 | undefined | 0.0000 |
| leg-commit | 95 | (unaligned) | 0 | 0 | 95 | undefined | 0.0000 | undefined | 0.0000 |
| leg-pr | 119 | (unaligned) | 0 | 0 | 119 | undefined | 0.0000 | undefined | 0.0000 |
| merged-as | 163 | (unaligned) | 0 | 0 | 163 | undefined | 0.0000 | undefined | 1.0000 |
| names | 444 | (unaligned) | 0 | 0 | 444 | undefined | 0.0000 | undefined | 1.0000 |
| parent | 882 | (unaligned) | 0 | 0 | 882 | undefined | 0.0000 | undefined | 1.0000 |
| references | 830 | (unaligned) | 0 | 0 | 830 | undefined | 0.0000 | undefined | 1.0000 |
| reviews-branch | 17 | (unaligned) | 0 | 0 | 17 | undefined | 0.0000 | undefined | 1.0000 |
| reviews-commit | 20 | (unaligned) | 0 | 0 | 20 | undefined | 0.0000 | undefined | 1.0000 |
| reviews-item | 2 | (unaligned) | 0 | 0 | 2 | undefined | 0.0000 | undefined | 1.0000 |

Unaligned predicted edge types (all false): none. Predicted edges with a no-majority endpoint: 0. Dropped (unscored endpoint): 0. Unobservable key rows: 0 (predicted edges dropped on them: 0). Key edge types with no edge in this corpus: none.

## Key private-key-v2.context-scored.json (context-scored, sha256 e02500f7164797e6d3841f6210c2d1aac4d749fec02d7bda66bb2b018a35fcbe)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 1.0000 | 0.0011 | 0.0022 | 0.0000 | 0.0000 |
| mapping, without singleton-only types ["comment"] | 1.0000 | 0.0011 | 0.0022 |  |  |
| ceiling | 1.0000 | 0.9017 | 0.9483 | 0.0000 | 0.8906 |
| ceiling, without singleton-only types ["comment"] | 1.0000 | 0.9001 | 0.9474 |  |  |
| ceiling with links | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| actor | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| branch | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| comment | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| commit | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| item | 1.0000 | 0.0032 | 0.0064 | 1.0000 | 0.7210 | 0.8379 |
| repo | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| seat | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| sprint | undefined | 0.0000 | undefined | 1.0000 | 0.1111 | 0.2000 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.actor | 3270 | 0 | 0.0000 | 1.0000 |
| data.author | 877 | 0 | 0.0000 | 1.0000 |
| data.branch | 140 | 0 | 0.0000 | 1.0000 |
| data.comment_id | 391 | 0 | 0.0000 | 1.0000 |
| data.commit_sha | 830 | 0 | 0.0000 | 1.0000 |
| data.file_id | 23 | 0 | 0.0000 | 0.0000 |
| data.head_ref | 163 | 0 | 0.0000 | 1.0000 |
| data.head_sha | 227 | 0 | 0.0000 | 1.0000 |
| data.issue | 1295 | 0 | 0.0000 | 0.7755 |
| data.key | 1291 | 0 | 0.0000 | 0.0000 |
| data.merge_commit_sha | 163 | 0 | 0.0000 | 1.0000 |
| data.number | 3270 | 0 | 0.0000 | 0.8686 |
| data.parents.0 | 877 | 0 | 0.0000 | 1.0000 |
| data.parents.1 | 5 | 0 | 0.0000 | 1.0000 |
| data.pr | 168 | 168 | 0.1569 | 0.9990 |
| data.ref_number | 847 | 0 | 0.0000 | 0.9071 |
| data.ref_repo | 847 | 0 | 0.0000 | 1.0000 |
| data.refs.0.number | 877 | 0 | 0.0000 | 0.8167 |
| data.refs.0.repo | 877 | 0 | 0.0000 | 1.0000 |
| data.refs.1.number | 293 | 0 | 0.0000 | 0.9151 |
| data.refs.1.repo | 293 | 0 | 0.0000 | 1.0000 |
| data.refs.2.number | 86 | 0 | 0.0000 | 0.9013 |
| data.refs.2.repo | 86 | 0 | 0.0000 | 1.0000 |
| data.refs.3.number | 36 | 0 | 0.0000 | 0.8837 |
| data.refs.3.repo | 36 | 0 | 0.0000 | 1.0000 |
| data.refs.4.number | 24 | 0 | 0.0000 | 0.9370 |
| data.refs.4.repo | 24 | 0 | 0.0000 | 1.0000 |
| data.refs.5.number | 15 | 0 | 0.0000 | 0.9039 |
| data.refs.5.repo | 15 | 0 | 0.0000 | 1.0000 |
| data.refs.6.number | 7 | 0 | 0.0000 | 0.9432 |
| data.refs.6.repo | 7 | 0 | 0.0000 | 1.0000 |
| data.refs.7.number | 5 | 0 | 0.0000 | 1.0000 |
| data.refs.7.repo | 5 | 0 | 0.0000 | 1.0000 |
| data.repo | 5524 | 0 | 0.0000 | 1.0000 |
| data.seat_id | 29 | 0 | 0.0000 | 1.0000 |
| data.sha | 977 | 0 | 0.0000 | 1.0000 |
| data.slot | 23 | 0 | 0.0000 | 0.0000 |
| data.sprint | 23 | 0 | 0.0000 | 0.3333 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| branch @ data.repo | 1 | 2 | 2 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| item @ data.ref_repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.0.repo | 3 | 6 | 153 | 1.0000 | 0.0007 | 0.0013 | 1.0000 | 0.6520 | 0.7894 |
| item @ data.refs.1.repo | 2 | 4 | 64 | 1.0000 | 0.0016 | 0.0031 | 1.0000 | 0.7431 | 0.8526 |
| item @ data.refs.2.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.3.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.4.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.5.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.6.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.refs.7.repo | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| item @ data.repo | 1 | 2 | 6 | 1.0000 | 0.1667 | 0.2857 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 0 at 0 paths (most: []). Key abstained: {}. Excluded (no_identity): {}. Undecodable records: 0.

Relationships (contract B3, unique typed directed edges):

| row | key edges | predicted | TP | FP | FN | P | R | F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mapping | 5095 | 0 | 0 | 0 | 5095 | undefined | 0.0000 | undefined |
| ceiling | 5095 | 4822 | 4822 | 0 | 273 | 1.0000 | 0.9464 | 0.9725 |
| ceiling with links | 5095 | 5095 | 5095 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 |

| key edge type | key edges | aligned predicted type | TP | FP | FN | P | R | F1 | ceiling R |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| commit-names | 899 | (unaligned) | 0 | 0 | 899 | undefined | 0.0000 | undefined | 1.0000 |
| cross-references | 847 | (unaligned) | 0 | 0 | 847 | undefined | 0.0000 | undefined | 1.0000 |
| has-comment | 391 | (unaligned) | 0 | 0 | 391 | undefined | 0.0000 | undefined | 1.0000 |
| head-branch | 163 | (unaligned) | 0 | 0 | 163 | undefined | 0.0000 | undefined | 1.0000 |
| head-commit | 164 | (unaligned) | 0 | 0 | 164 | undefined | 0.0000 | undefined | 1.0000 |
| leg-branch | 59 | (unaligned) | 0 | 0 | 59 | undefined | 0.0000 | undefined | 0.0000 |
| leg-commit | 95 | (unaligned) | 0 | 0 | 95 | undefined | 0.0000 | undefined | 0.0000 |
| leg-pr | 119 | (unaligned) | 0 | 0 | 119 | undefined | 0.0000 | undefined | 0.0000 |
| merged-as | 163 | (unaligned) | 0 | 0 | 163 | undefined | 0.0000 | undefined | 1.0000 |
| names | 444 | (unaligned) | 0 | 0 | 444 | undefined | 0.0000 | undefined | 1.0000 |
| parent | 882 | (unaligned) | 0 | 0 | 882 | undefined | 0.0000 | undefined | 1.0000 |
| references | 830 | (unaligned) | 0 | 0 | 830 | undefined | 0.0000 | undefined | 1.0000 |
| reviews-branch | 17 | (unaligned) | 0 | 0 | 17 | undefined | 0.0000 | undefined | 1.0000 |
| reviews-commit | 20 | (unaligned) | 0 | 0 | 20 | undefined | 0.0000 | undefined | 1.0000 |
| reviews-item | 2 | (unaligned) | 0 | 0 | 2 | undefined | 0.0000 | undefined | 1.0000 |

Unaligned predicted edge types (all false): none. Predicted edges with a no-majority endpoint: 0. Dropped (unscored endpoint): 0. Unobservable key rows: 0 (predicted edges dropped on them: 0). Key edge types with no edge in this corpus: none.


