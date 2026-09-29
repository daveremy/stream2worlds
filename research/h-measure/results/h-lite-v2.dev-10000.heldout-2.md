# h-measure score

A v0 stream mapping cannot say that different values name one entity. A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the best v0 mapping for that key) and against the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/frozen/h-lite-v2.dev-10000.json (sha256 3caa0bc2cd67daf1edda83afa44623b6472c93eadc53c9d015f9667f267f6963): profiler 2 proposed a mapping from the first 10000 events of dev (sha256 53d35b1c8631cc506d762a855ea554d4ca833f5c02a6fe80c5ceeb52205ae4dd); it abstained on 16 paths. Scored on heldout-2 (sha256 4432c2a8533e0080a2a97344a2aa5ed1273aafc951faf673dcdb36b65800bc22), 100000 records.

## Key dev-key-v1.json (base, sha256 006c50d4a8067c620a157534cb17f5cdfcf64f58ff275184aafaaefc55a48595)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9815 | 0.1722 | 0.2929 | 0.0185 | 0.0000 |
| mapping, without singleton-only types ["event", "log"] | 0.9815 | 0.1956 | 0.3262 |  |  |
| ceiling | 1.0000 | 0.3948 | 0.5661 | 0.0000 | 0.2067 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3123 | 0.4760 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.2500 | 0.4000 | 1.0000 | 0.2500 | 0.4000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9973 | 0.2484 | 0.3977 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3964 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4967 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 34634 | 0 | 0.0000 | 1.0000 |
| data.revision.old | 28811 | 0 | 0.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4967 | 0.0000 |
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
| page @ data.wiki | 78 | 172 | 3146 | 1.0000 | 0.2487 | 0.3983 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 1 | 2 | 3 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 388 | 917 | 17117 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5096 at 2 paths (most: [("data.log_action", 5091), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 1127}. Undecodable records: 0.

## Key dev-key-v1.canonical-mention.json (canonical-mention, sha256 82fa6b0118fb35d3c6f8f446fe78776d84d87a8e8bfe960b0bc9fb6aca1ff4b4)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.0000 | 0.0000 | undefined | 1.0000 | 0.0000 |
| mapping, without singleton-only types ["event", "log"] | 0.0000 | 0.0000 | undefined |  |  |
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
| data.log_id | 3964 | 0 | 0.0000 | 1.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 34634 | 0 | 0.0000 | 1.0000 |
| data.revision.old | 28811 | 0 | 0.0000 | 1.0000 |
| data.title | 99995 | 0 | 0.0000 | 1.0000 |
| data.user | 99994 | 0 | 0.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 1.0000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 78 | 172 | 1573 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| revision @ data.wiki | 1 | 2 | 3 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 388 | 917 | 17117 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5091 at 1 paths (most: [("data.log_action", 5091)]). Key abstained: {}. Excluded (no_identity): {"data.log_id": 1127}. Undecodable records: 0.

## Key dev-key-v1.q1-no-wiki.json (q1-no-wiki, sha256 5cb8a5658054920c10f220a38dc7338ccf44c2f5a466d0993bb0ee5215542514)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9815 | 0.1721 | 0.2928 | 0.0185 | 0.0000 |
| mapping, without singleton-only types ["event", "log"] | 0.9815 | 0.1955 | 0.3261 |  |  |
| ceiling | 1.0000 | 0.3948 | 0.5661 | 0.0000 | 0.2071 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3123 | 0.4760 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.2496 | 0.3995 | 1.0000 | 0.2500 | 0.4000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9973 | 0.2484 | 0.3977 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3964 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4967 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 34634 | 0 | 0.0000 | 1.0000 |
| data.revision.old | 28811 | 0 | 0.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4967 | 0.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 0.0000 |
| data.title | 99995 | 0 | 0.0000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.4992 | 0.0000 |
| data.user | 99994 | 0 | 0.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| revision @ data.wiki | 1 | 2 | 3 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 388 | 917 | 17117 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5096 at 2 paths (most: [("data.log_action", 5091), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 1127}. Undecodable records: 0.

## Key dev-key-v1.q3-rcid-scored.json (q3-rcid-scored, sha256 bd1ae190f4dc115d88f8755587dbacae85466ff998d3e4712c9bdf13bdd344c2)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9815 | 0.1546 | 0.2672 | 0.0185 | 0.0000 |
| mapping, without singleton-only types ["event", "log", "rc"] | 0.9815 | 0.1956 | 0.3262 |  |  |
| ceiling | 1.0000 | 0.4564 | 0.6268 | 0.0000 | 0.2067 |
| ceiling, without singleton-only types ["event", "log", "rc"] | 1.0000 | 0.3123 | 0.4760 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.2500 | 0.4000 | 1.0000 | 0.2500 | 0.4000 |
| rc | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9973 | 0.2484 | 0.3977 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.id | 98409 | 0 | 0.0000 | 1.0000 |
| data.log_id | 3964 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4967 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 34634 | 0 | 0.0000 | 1.0000 |
| data.revision.old | 28811 | 0 | 0.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4967 | 0.0000 |
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
| page @ data.wiki | 78 | 172 | 3146 | 1.0000 | 0.2487 | 0.3983 | 1.0000 | 0.2500 | 0.4000 |
| rc @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| revision @ data.wiki | 1 | 2 | 3 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 388 | 917 | 17117 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5096 at 2 paths (most: [("data.log_action", 5091), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 1127}. Undecodable records: 0.

## Key dev-key-v1.q4-separate.json (q4-separate, sha256 9f0be88923f7eebf95a9f76f1158120b64657c5e36490353da7794edec68d236)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.6555 | 0.2882 | 0.4004 | 0.3445 | 0.0000 |
| mapping, without singleton-only types ["event", "log"] | 0.6555 | 0.3275 | 0.4368 |  |  |
| ceiling | 1.0000 | 0.8271 | 0.9054 | 0.0000 | 0.2196 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.8035 | 0.8911 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| domain | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.2500 | 0.4000 | 1.0000 | 0.2500 | 0.4000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| server_name | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| server_url | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| wiki | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3964 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 100000 | 100000 | 1.0000 | 1.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 34634 | 0 | 0.0000 | 1.0000 |
| data.revision.old | 28811 | 0 | 0.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 1.0000 | 1.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 1.0000 |
| data.title | 99995 | 0 | 0.0000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.4999 | 0.0000 |
| data.user | 99994 | 0 | 0.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 1.0000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 78 | 172 | 3146 | 1.0000 | 0.2487 | 0.3983 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 1 | 2 | 3 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 388 | 917 | 17117 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5091 at 1 paths (most: [("data.log_action", 5091)]). Key abstained: {}. Excluded (no_identity): {"data.log_id": 1127}. Undecodable records: 0.

## Key dev-key-v1.user-global.json (user-global, sha256 8454ec7e6ec60d21e8b714fcc8c74cfc6253e2d63d9efd541a1dcda8ebc473df)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9815 | 0.1722 | 0.2929 | 0.0185 | 0.0000 |
| mapping, without singleton-only types ["event", "log"] | 0.9815 | 0.1956 | 0.3262 |  |  |
| ceiling | 1.0000 | 0.3948 | 0.5661 | 0.0000 | 0.2021 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3123 | 0.4760 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.2500 | 0.4000 | 1.0000 | 0.2500 | 0.4000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9973 | 0.2484 | 0.3977 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3964 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4967 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 34634 | 0 | 0.0000 | 1.0000 |
| data.revision.old | 28811 | 0 | 0.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4967 | 0.0000 |
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
| page @ data.wiki | 78 | 172 | 3146 | 1.0000 | 0.2487 | 0.3983 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 1 | 2 | 3 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5096 at 2 paths (most: [("data.log_action", 5091), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 1127}. Undecodable records: 0.


