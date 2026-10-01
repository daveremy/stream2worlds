# h-measure score

A v0 stream mapping cannot say that different values name one entity; a version-2 mapping can, with links (decision 0027). A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the oracle-v0 mapping for that key), its ceiling-with-links row (that mapping plus a rule per alias path, linked into the oracle rule on the same identity, the ceiling for a mapping with links) and the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/frozen/h-min-v9.dev-200000.pins-388.json (sha256 e7c9eef750c97a510d7b38476e233f57d0ca93c4b8696c896cdc77fa72a18acc): profiler 9 proposed a mapping from the first 200000 events of dev (sha256 53d35b1c8631cc506d762a855ea554d4ca833f5c02a6fe80c5ceeb52205ae4dd); it abstained on 18 paths. Scored on reserved-6 (sha256 2df0593c1e3121808a0e5df237f6fb3efdb9cd99170ed584a6bf0679f63619f4), 100000 records.

## Key dev-key-v3.json (base, sha256 39d4949621c0e6558a728f06ceb1a5b00e9696cc96fc6b885953dfc75a1be6df)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9806 | 0.5716 | 0.7222 | 0.0194 | 0.1963 |
| mapping, without singleton-only types ["event", "log"] | 0.9806 | 0.6470 | 0.7796 |  |  |
| ceiling | 1.0000 | 0.4069 | 0.5785 | 0.0000 | 0.2060 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3287 | 0.4948 |  |  |
| ceiling with links | 0.9988 | 0.9989 | 0.9989 | 0.0012 | 0.9998 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9994 | 0.5000 | 0.6665 | 1.0000 | 0.2500 | 0.4000 |
| revision | 0.9999 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9668 | 1.0000 | 0.9831 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9978 | 0.5598 | 0.7172 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3135 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.7464 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 43445 | 43445 | 1.0000 | 1.0000 |
| data.revision.old | 38647 | 38647 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.7464 | 0.0000 |
| data.server_url | 99995 | 99995 | 0.7464 | 0.0000 |
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.5000 | 0.0000 |
| data.user | 99995 | 99995 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 65 | 164 | 464 | 0.7509 | 0.4934 | 0.5955 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 7 | 14 | 14 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 350 | 871 | 11626 | 0.7143 | 1.0000 | 0.8333 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 9305 at 3 paths (most: [("data.log_action", 4650), ("data.log_action_comment", 4650), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 1515}. Undecodable records: 0.

Relationships (contract B3, unique typed directed edges):

| row | key edges | predicted | TP | FP | FN | P | R | F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mapping | 178265 | 148028 | 41906 | 106122 | 136359 | 0.2831 | 0.2351 | 0.2569 |
| ceiling | 178265 | 178265 | 178265 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 |
| ceiling with links | 178265 | 178265 | 178265 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 |

| key edge type | key edges | aligned predicted type | TP | FP | FN | P | R | F1 | ceiling R |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| by | 43445 | (unaligned) | 0 | 0 | 43445 | undefined | 0.0000 | undefined | 1.0000 |
| edits | 43445 | (unaligned) | 0 | 0 | 43445 | undefined | 0.0000 | undefined | 1.0000 |
| follows | 38647 | (unaligned) | 0 | 0 | 38647 | undefined | 0.0000 | undefined | 1.0000 |
| logged-by | 3135 | (unaligned) | 0 | 0 | 3135 | undefined | 0.0000 | undefined | 1.0000 |
| page-on | 42139 | data/title_url+meta/uri → data/server_name+data/server_url+meta/domain, n:1 | 41906 | 245 | 233 | 0.9942 | 0.9945 | 0.9943 | 1.0000 |
| targets | 3135 | (unaligned) | 0 | 0 | 3135 | undefined | 0.0000 | undefined | 1.0000 |
| user-on | 4319 | (unaligned) | 0 | 0 | 4319 | undefined | 0.0000 | undefined | 1.0000 |

Unaligned predicted edge types (all false): data/comment+data/log_action_comment+data/parsedcomment → data/log_action, n:1 (899); data/comment+data/log_action_comment+data/parsedcomment → data/title_url+meta/uri, n:m (5423); data/comment+data/log_action_comment+data/parsedcomment → revision/new+revision/old, n:m (2513); data/log_action → data/server_name+data/server_url+meta/domain, n:m (292); data/log_action → data/title_url+meta/uri, n:m (3648); data/server_name+data/server_url+meta/domain → data/comment+data/log_action_comment+data/parsedcomment, n:m (1035); data/server_name+data/server_url+meta/domain → data/user, n:m (4380); data/server_name+data/server_url+meta/domain → revision/new+revision/old, n:m (43445); data/user → data/title, n:m (44242). Predicted edges with a no-majority endpoint: 15872. Dropped (unscored endpoint): 410674. Unobservable key rows: 0 (predicted edges dropped on them: 0). Key edge types with no edge in this corpus: none.

## Key dev-key-v3.canonical-mention.json (canonical-mention, sha256 8376b36c3e31c6b4e1f7689afcffebb63604ad35933981ac999e4e44208afda3)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9563 | 0.5814 | 0.7231 | 0.0437 | 0.9596 |
| mapping, without singleton-only types ["event", "log"] | 0.9563 | 0.7383 | 0.8333 |  |  |
| ceiling | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 1.0000 | 1.0000 |  |  |
| ceiling with links | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9988 | 1.0000 | 0.9994 | 1.0000 | 1.0000 | 1.0000 |
| revision | 0.9999 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9668 | 1.0000 | 0.9831 | 1.0000 | 1.0000 | 1.0000 |
| wiki | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3135 | 0 | 0.0000 | 1.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 43445 | 43445 | 1.0000 | 1.0000 |
| data.revision.old | 38647 | 38647 | 1.0000 | 1.0000 |
| data.title | 99995 | 99995 | 1.0000 | 1.0000 |
| data.user | 99995 | 99995 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 1.0000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 65 | 164 | 232 | 0.5018 | 1.0000 | 0.6683 | 1.0000 | 1.0000 | 1.0000 |
| revision @ data.wiki | 7 | 14 | 14 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 350 | 871 | 11626 | 0.7143 | 1.0000 | 0.8333 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 9300 at 2 paths (most: [("data.log_action", 4650), ("data.log_action_comment", 4650)]). Key abstained: {}. Excluded (no_identity): {"data.log_id": 1515}. Undecodable records: 0.

Relationships (contract B3, unique typed directed edges):

| row | key edges | predicted | TP | FP | FN | P | R | F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mapping | 178265 | 47654 | 0 | 47654 | 178265 | 0.0000 | 0.0000 | undefined |
| ceiling | 178265 | 178265 | 178265 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 |
| ceiling with links | 178265 | 178265 | 178265 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 |

| key edge type | key edges | aligned predicted type | TP | FP | FN | P | R | F1 | ceiling R |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| by | 43445 | (unaligned) | 0 | 0 | 43445 | undefined | 0.0000 | undefined | 1.0000 |
| edits | 43445 | (unaligned) | 0 | 0 | 43445 | undefined | 0.0000 | undefined | 1.0000 |
| follows | 38647 | (unaligned) | 0 | 0 | 38647 | undefined | 0.0000 | undefined | 1.0000 |
| logged-by | 3135 | (unaligned) | 0 | 0 | 3135 | undefined | 0.0000 | undefined | 1.0000 |
| page-on | 42139 | (unaligned) | 0 | 0 | 42139 | undefined | 0.0000 | undefined | 1.0000 |
| targets | 3135 | (unaligned) | 0 | 0 | 3135 | undefined | 0.0000 | undefined | 1.0000 |
| user-on | 4319 | (unaligned) | 0 | 0 | 4319 | undefined | 0.0000 | undefined | 1.0000 |

Unaligned predicted edge types (all false): data/comment+data/log_action_comment+data/parsedcomment → data/log_action, n:1 (899); data/comment+data/log_action_comment+data/parsedcomment → revision/new+revision/old, n:m (2513); data/user → data/title, n:m (44242). Predicted edges with a no-majority endpoint: 4722. Dropped (unscored endpoint): 511048. Unobservable key rows: 0 (predicted edges dropped on them: 0). Key edge types with no edge in this corpus: none.

## Key dev-key-v2.user-global.json (user-global, sha256 ffa2f158dcbc8a4203344b6689271830ba785b29e8371dd77758b8c47063018a)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9854 | 0.5716 | 0.7235 | 0.0146 | 0.2022 |
| mapping, without singleton-only types ["event", "log"] | 0.9854 | 0.6470 | 0.7811 |  |  |
| ceiling | 1.0000 | 0.4069 | 0.5785 | 0.0000 | 0.2022 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3287 | 0.4948 |  |  |
| ceiling with links | 0.9988 | 0.9989 | 0.9989 | 0.0012 | 0.9998 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9994 | 0.5000 | 0.6665 | 1.0000 | 0.2500 | 0.4000 |
| revision | 0.9999 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9978 | 0.5598 | 0.7172 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3135 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.7464 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 43445 | 43445 | 1.0000 | 1.0000 |
| data.revision.old | 38647 | 38647 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.7464 | 0.0000 |
| data.server_url | 99995 | 99995 | 0.7464 | 0.0000 |
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.5000 | 0.0000 |
| data.user | 99995 | 99995 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 65 | 164 | 464 | 0.7509 | 0.4934 | 0.5955 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 7 | 14 | 14 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 9305 at 3 paths (most: [("data.log_action", 4650), ("data.log_action_comment", 4650), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 1515}. Undecodable records: 0.

No relationships declared by this key (format 2 or earlier).

## Key dev-key-v1.json (base, sha256 006c50d4a8067c620a157534cb17f5cdfcf64f58ff275184aafaaefc55a48595)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9806 | 0.5716 | 0.7222 | 0.0194 | 0.1963 |
| mapping, without singleton-only types ["event", "log"] | 0.9806 | 0.6470 | 0.7796 |  |  |
| ceiling | 1.0000 | 0.4069 | 0.5785 | 0.0000 | 0.2060 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3287 | 0.4948 |  |  |
| ceiling with links | 0.9988 | 0.9989 | 0.9989 | 0.0012 | 0.9998 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9994 | 0.5000 | 0.6665 | 1.0000 | 0.2500 | 0.4000 |
| revision | 0.9999 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9668 | 1.0000 | 0.9831 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9978 | 0.5598 | 0.7172 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3135 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.7464 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 43445 | 43445 | 1.0000 | 1.0000 |
| data.revision.old | 38647 | 38647 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.7464 | 0.0000 |
| data.server_url | 99995 | 99995 | 0.7464 | 0.0000 |
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.5000 | 0.0000 |
| data.user | 99995 | 99995 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 65 | 164 | 464 | 0.7509 | 0.4934 | 0.5955 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 7 | 14 | 14 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 350 | 871 | 11626 | 0.7143 | 1.0000 | 0.8333 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 9305 at 3 paths (most: [("data.log_action", 4650), ("data.log_action_comment", 4650), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 1515}. Undecodable records: 0.

No relationships declared by this key (format 2 or earlier).


