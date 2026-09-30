# h-measure score

A v0 stream mapping cannot say that different values name one entity. A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the best v0 mapping for that key) and against the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/frozen/h-min-v5.dev-200000.json (sha256 99c13f9e64190eee2a58a00cd53839a7046c675849db5e80eebffd9308fcf7e8): profiler 5 proposed a mapping from the first 200000 events of dev (sha256 53d35b1c8631cc506d762a855ea554d4ca833f5c02a6fe80c5ceeb52205ae4dd); it abstained on 18 paths. Scored on reserved (sha256 b420214c92c0ac8151b2b64279cbebd9c8250349b28712dc898974dc7a56f501), 100000 records.

## Key dev-key-v1.json (base, sha256 006c50d4a8067c620a157534cb17f5cdfcf64f58ff275184aafaaefc55a48595)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9873 | 0.4220 | 0.5912 | 0.0127 | 0.1250 |
| mapping, without singleton-only types ["event", "log"] | 0.9873 | 0.4787 | 0.6448 |  |  |
| ceiling | 1.0000 | 0.3973 | 0.5687 | 0.0000 | 0.1320 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3163 | 0.4806 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9988 | 0.5000 | 0.6664 | 1.0000 | 0.2500 | 0.4000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9713 | 1.0000 | 0.9855 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9988 | 0.2492 | 0.3988 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3202 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4983 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 36494 | 36494 | 1.0000 | 1.0000 |
| data.revision.old | 31399 | 31399 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4983 | 0.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 0.0000 |
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.5000 | 0.0000 |
| data.user | 99995 | 99995 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 205 | 412 | 1368 | 0.8280 | 0.4995 | 0.6231 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 2 | 4 | 4 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 203 | 543 | 11443 | 0.7494 | 1.0000 | 0.8568 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 3884 at 2 paths (most: [("data.log_action", 3879), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 677}. Undecodable records: 0.

## Key dev-key-v1.canonical-mention.json (canonical-mention, sha256 82fa6b0118fb35d3c6f8f446fe78776d84d87a8e8bfe960b0bc9fb6aca1ff4b4)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9743 | 0.5687 | 0.7182 | 0.0257 | 0.9541 |
| mapping, without singleton-only types ["event", "log"] | 0.9743 | 0.7282 | 0.8335 |  |  |
| ceiling | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 1.0000 | 1.0000 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9976 | 1.0000 | 0.9988 | 1.0000 | 1.0000 | 1.0000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9713 | 1.0000 | 0.9855 | 1.0000 | 1.0000 | 1.0000 |
| wiki | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3202 | 0 | 0.0000 | 1.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 36494 | 36494 | 1.0000 | 1.0000 |
| data.revision.old | 31399 | 31399 | 1.0000 | 1.0000 |
| data.title | 99995 | 99995 | 1.0000 | 1.0000 |
| data.user | 99995 | 99995 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 1.0000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 205 | 412 | 684 | 0.6560 | 1.0000 | 0.7923 | 1.0000 | 1.0000 | 1.0000 |
| revision @ data.wiki | 2 | 4 | 4 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 203 | 543 | 11443 | 0.7494 | 1.0000 | 0.8568 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 3879 at 1 paths (most: [("data.log_action", 3879)]). Key abstained: {}. Excluded (no_identity): {"data.log_id": 677}. Undecodable records: 0.

## Key dev-key-v1.q1-no-wiki.json (q1-no-wiki, sha256 5cb8a5658054920c10f220a38dc7338ccf44c2f5a466d0993bb0ee5215542514)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9878 | 0.4218 | 0.5912 | 0.0122 | 0.1256 |
| mapping, without singleton-only types ["event", "log"] | 0.9878 | 0.4785 | 0.6447 |  |  |
| ceiling | 1.0000 | 0.3973 | 0.5687 | 0.0000 | 0.1326 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3163 | 0.4806 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.4994 | 0.6661 | 1.0000 | 0.2500 | 0.4000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9713 | 1.0000 | 0.9855 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9988 | 0.2492 | 0.3988 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3202 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4983 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 36494 | 36494 | 1.0000 | 1.0000 |
| data.revision.old | 31399 | 31399 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4983 | 0.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 0.0000 |
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.4988 | 0.0000 |
| data.user | 99995 | 99995 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| revision @ data.wiki | 2 | 4 | 4 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 203 | 543 | 11443 | 0.7494 | 1.0000 | 0.8568 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 3884 at 2 paths (most: [("data.log_action", 3879), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 677}. Undecodable records: 0.

## Key dev-key-v1.q3-rcid-scored.json (q3-rcid-scored, sha256 bd1ae190f4dc115d88f8755587dbacae85466ff998d3e4712c9bdf13bdd344c2)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9873 | 0.3788 | 0.5476 | 0.0127 | 0.1250 |
| mapping, without singleton-only types ["event", "log", "rc"] | 0.9873 | 0.4787 | 0.6448 |  |  |
| ceiling | 1.0000 | 0.4589 | 0.6291 | 0.0000 | 0.1320 |
| ceiling, without singleton-only types ["event", "log", "rc"] | 1.0000 | 0.3163 | 0.4806 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9988 | 0.5000 | 0.6664 | 1.0000 | 0.2500 | 0.4000 |
| rc | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9713 | 1.0000 | 0.9855 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9988 | 0.2492 | 0.3988 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.id | 99163 | 0 | 0.0000 | 1.0000 |
| data.log_id | 3202 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4983 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 36494 | 36494 | 1.0000 | 1.0000 |
| data.revision.old | 31399 | 31399 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4983 | 0.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 0.0000 |
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.5000 | 0.0000 |
| data.user | 99995 | 99995 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 205 | 412 | 1368 | 0.8280 | 0.4995 | 0.6231 | 1.0000 | 0.2500 | 0.4000 |
| rc @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| revision @ data.wiki | 2 | 4 | 4 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 203 | 543 | 11443 | 0.7494 | 1.0000 | 0.8568 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 3884 at 2 paths (most: [("data.log_action", 3879), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 677}. Undecodable records: 0.

## Key dev-key-v1.q4-separate.json (q4-separate, sha256 9f0be88923f7eebf95a9f76f1158120b64657c5e36490353da7794edec68d236)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.8129 | 0.5371 | 0.6468 | 0.1871 | 0.1238 |
| mapping, without singleton-only types ["event", "log"] | 0.8129 | 0.6093 | 0.6965 |  |  |
| ceiling | 1.0000 | 0.8278 | 0.9058 | 0.0000 | 0.1443 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.8047 | 0.8918 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| domain | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9988 | 0.5000 | 0.6664 | 1.0000 | 0.2500 | 0.4000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| server_name | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| server_url | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9713 | 1.0000 | 0.9855 | 1.0000 | 1.0000 | 1.0000 |
| wiki | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3202 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 100000 | 100000 | 1.0000 | 1.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 36494 | 36494 | 1.0000 | 1.0000 |
| data.revision.old | 31399 | 31399 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 1.0000 | 1.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 1.0000 |
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.5000 | 0.0000 |
| data.user | 99995 | 99995 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 1.0000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 205 | 412 | 1368 | 0.8280 | 0.4995 | 0.6231 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 2 | 4 | 4 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 203 | 543 | 11443 | 0.7494 | 1.0000 | 0.8568 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 3879 at 1 paths (most: [("data.log_action", 3879)]). Key abstained: {}. Excluded (no_identity): {"data.log_id": 677}. Undecodable records: 0.

## Key dev-key-v1.user-global.json (user-global, sha256 8454ec7e6ec60d21e8b714fcc8c74cfc6253e2d63d9efd541a1dcda8ebc473df)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9924 | 0.4220 | 0.5921 | 0.0076 | 0.1284 |
| mapping, without singleton-only types ["event", "log"] | 0.9924 | 0.4787 | 0.6458 |  |  |
| ceiling | 1.0000 | 0.3973 | 0.5687 | 0.0000 | 0.1284 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3163 | 0.4806 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9988 | 0.5000 | 0.6664 | 1.0000 | 0.2500 | 0.4000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9988 | 0.2492 | 0.3988 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3202 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4983 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 36494 | 36494 | 1.0000 | 1.0000 |
| data.revision.old | 31399 | 31399 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4983 | 0.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 0.0000 |
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.5000 | 0.0000 |
| data.user | 99995 | 99995 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 205 | 412 | 1368 | 0.8280 | 0.4995 | 0.6231 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 2 | 4 | 4 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 3884 at 2 paths (most: [("data.log_action", 3879), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 677}. Undecodable records: 0.


