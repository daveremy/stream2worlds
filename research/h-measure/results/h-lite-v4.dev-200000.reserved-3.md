# h-measure score

A v0 stream mapping cannot say that different values name one entity. A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the best v0 mapping for that key) and against the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/frozen/h-lite-v4.dev-200000.json (sha256 494d56c4770bd1ccee8030aaf0975c0b1e84b5126a3d1c7856c4d09926f06402): profiler 4 proposed a mapping from the first 200000 events of dev (sha256 53d35b1c8631cc506d762a855ea554d4ca833f5c02a6fe80c5ceeb52205ae4dd); it abstained on 18 paths. Scored on reserved-3 (sha256 32593b5564824be07dc47d7b85ba2f378c03b32d0cf19efac03b6c9fbbf84447), 100000 records.

## Key dev-key-v1.json (base, sha256 006c50d4a8067c620a157534cb17f5cdfcf64f58ff275184aafaaefc55a48595)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9844 | 0.3425 | 0.5082 | 0.0156 | 0.0420 |
| mapping, without singleton-only types ["event", "log"] | 0.9844 | 0.3883 | 0.5570 |  |  |
| ceiling | 1.0000 | 0.3997 | 0.5712 | 0.0000 | 0.1917 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3194 | 0.4842 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9991 | 0.5000 | 0.6664 | 1.0000 | 0.2500 | 0.4000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9682 | 1.0000 | 0.9838 | 1.0000 | 1.0000 | 1.0000 |
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
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.4999 | 0.0000 |
| data.user | 99994 | 99994 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 116 | 271 | 1442 | 0.8701 | 0.4967 | 0.6324 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 4 | 8 | 8 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 325 | 913 | 17694 | 0.8201 | 1.0000 | 0.9011 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 4135 at 2 paths (most: [("data.log_action", 4130), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 900}. Undecodable records: 0.

## Key dev-key-v1.canonical-mention.json (canonical-mention, sha256 82fa6b0118fb35d3c6f8f446fe78776d84d87a8e8bfe960b0bc9fb6aca1ff4b4)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9633 | 0.4214 | 0.5863 | 0.0367 | 0.5310 |
| mapping, without singleton-only types ["event", "log"] | 0.9633 | 0.5385 | 0.6908 |  |  |
| ceiling | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 1.0000 | 1.0000 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9981 | 1.0000 | 0.9991 | 1.0000 | 1.0000 | 1.0000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9682 | 1.0000 | 0.9838 | 1.0000 | 1.0000 | 1.0000 |
| wiki | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3230 | 0 | 0.0000 | 1.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 38418 | 0 | 0.0000 | 1.0000 |
| data.revision.old | 32972 | 0 | 0.0000 | 1.0000 |
| data.title | 99995 | 99995 | 1.0000 | 1.0000 |
| data.user | 99994 | 99994 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 1.0000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 116 | 271 | 721 | 0.7401 | 1.0000 | 0.8506 | 1.0000 | 1.0000 | 1.0000 |
| revision @ data.wiki | 4 | 8 | 8 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 325 | 913 | 17694 | 0.8201 | 1.0000 | 0.9011 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 4130 at 1 paths (most: [("data.log_action", 4130)]). Key abstained: {}. Excluded (no_identity): {"data.log_id": 900}. Undecodable records: 0.

## Key dev-key-v1.user-global.json (user-global, sha256 8454ec7e6ec60d21e8b714fcc8c74cfc6253e2d63d9efd541a1dcda8ebc473df)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9907 | 0.3425 | 0.5090 | 0.0093 | 0.0469 |
| mapping, without singleton-only types ["event", "log"] | 0.9907 | 0.3883 | 0.5580 |  |  |
| ceiling | 1.0000 | 0.3997 | 0.5712 | 0.0000 | 0.1871 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3194 | 0.4842 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9991 | 0.5000 | 0.6664 | 1.0000 | 0.2500 | 0.4000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
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
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.4999 | 0.0000 |
| data.user | 99994 | 99994 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 116 | 271 | 1442 | 0.8701 | 0.4967 | 0.6324 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 4 | 8 | 8 | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 4135 at 2 paths (most: [("data.log_action", 4130), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 900}. Undecodable records: 0.


