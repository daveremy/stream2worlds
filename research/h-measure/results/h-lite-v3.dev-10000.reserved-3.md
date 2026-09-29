# h-measure score

A v0 stream mapping cannot say that different values name one entity. A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the best v0 mapping for that key) and against the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/frozen/h-lite-v3.dev-10000.json (sha256 014ad5ffa5e67fc203e0822db1168acc6a5188c48e9189e846312c7354abfcaf): profiler 3 proposed a mapping from the first 10000 events of dev (sha256 53d35b1c8631cc506d762a855ea554d4ca833f5c02a6fe80c5ceeb52205ae4dd); it abstained on 16 paths. Scored on reserved-3 (sha256 32593b5564824be07dc47d7b85ba2f378c03b32d0cf19efac03b6c9fbbf84447), 100000 records.

## Key dev-key-v1.json (base, sha256 006c50d4a8067c620a157534cb17f5cdfcf64f58ff275184aafaaefc55a48595)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9988 | 0.1710 | 0.2920 | 0.0012 | 0.0000 |
| mapping, without singleton-only types ["event", "log"] | 0.9988 | 0.1939 | 0.3247 |  |  |
| ceiling | 1.0000 | 0.3997 | 0.5712 | 0.0000 | 0.1917 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3194 | 0.4842 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.2500 | 0.4000 | 1.0000 | 0.2500 | 0.4000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9983 | 0.2489 | 0.3985 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3230 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4978 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 38418 | 0 | 0.0000 | 1.0000 |
| data.revision.old | 32972 | 0 | 0.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4978 | 0.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 0.0000 |
| data.title | 99995 | 0 | 0.0000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.4999 | 0.0000 |
| data.user | 99994 | 0 | 0.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 116 | 271 | 1442 | 1.0000 | 0.2467 | 0.3958 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 4 | 8 | 8 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 325 | 913 | 17694 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5 at 1 paths (most: [("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 900}. Undecodable records: 0.

## Key dev-key-v1.canonical-mention.json (canonical-mention, sha256 82fa6b0118fb35d3c6f8f446fe78776d84d87a8e8bfe960b0bc9fb6aca1ff4b4)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | undefined | 0.0000 | undefined | undefined | 0.0000 |
| mapping, without singleton-only types ["event", "log"] | undefined | 0.0000 | undefined |  |  |
| ceiling | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 1.0000 | 1.0000 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| wiki | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3230 | 0 | 0.0000 | 1.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 38418 | 0 | 0.0000 | 1.0000 |
| data.revision.old | 32972 | 0 | 0.0000 | 1.0000 |
| data.title | 99995 | 0 | 0.0000 | 1.0000 |
| data.user | 99994 | 0 | 0.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 1.0000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 116 | 271 | 721 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| revision @ data.wiki | 4 | 8 | 8 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 325 | 913 | 17694 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 0 at 0 paths (most: []). Key abstained: {}. Excluded (no_identity): {"data.log_id": 900}. Undecodable records: 0.

## Key dev-key-v1.user-global.json (user-global, sha256 8454ec7e6ec60d21e8b714fcc8c74cfc6253e2d63d9efd541a1dcda8ebc473df)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9988 | 0.1710 | 0.2920 | 0.0012 | 0.0000 |
| mapping, without singleton-only types ["event", "log"] | 0.9988 | 0.1939 | 0.3247 |  |  |
| ceiling | 1.0000 | 0.3997 | 0.5712 | 0.0000 | 0.1871 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3194 | 0.4842 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.2500 | 0.4000 | 1.0000 | 0.2500 | 0.4000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9983 | 0.2489 | 0.3985 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3230 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4978 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 38418 | 0 | 0.0000 | 1.0000 |
| data.revision.old | 32972 | 0 | 0.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4978 | 0.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 0.0000 |
| data.title | 99995 | 0 | 0.0000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.4999 | 0.0000 |
| data.user | 99994 | 0 | 0.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 116 | 271 | 1442 | 1.0000 | 0.2467 | 0.3958 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 4 | 8 | 8 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5 at 1 paths (most: [("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 900}. Undecodable records: 0.


