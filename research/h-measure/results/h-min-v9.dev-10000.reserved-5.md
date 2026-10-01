# h-measure score

A v0 stream mapping cannot say that different values name one entity; a version-2 mapping can, with links (decision 0027). A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the oracle-v0 mapping for that key), its ceiling-with-links row (that mapping plus a rule per alias path, linked into the oracle rule on the same identity, the ceiling for a mapping with links) and the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/frozen/h-min-v9.dev-10000.json (sha256 7b09bac871ead50299bc8fe322820ffa3c403e09fcb008b2bcff73952a8c9c51): profiler 9 proposed a mapping from the first 10000 events of dev (sha256 53d35b1c8631cc506d762a855ea554d4ca833f5c02a6fe80c5ceeb52205ae4dd); it abstained on 16 paths. Scored on reserved-5 (sha256 7bc24d57e4940db06a0f6d2edb153db24ed6c54b22ad230be1a654c0ca472320), 100000 records.

## Key dev-key-v1.json (base, sha256 006c50d4a8067c620a157534cb17f5cdfcf64f58ff275184aafaaefc55a48595)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9943 | 0.6781 | 0.8063 | 0.0057 | 0.9807 |
| mapping, without singleton-only types ["event", "log"] | 0.9943 | 0.7697 | 0.8677 |  |  |
| ceiling | 1.0000 | 0.3949 | 0.5662 | 0.0000 | 0.1457 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3131 | 0.4768 |  |  |
| ceiling with links | 0.9993 | 0.9989 | 0.9991 | 0.0007 | 0.9999 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9994 | 0.9987 | 0.9991 | 1.0000 | 0.2500 | 0.4000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9675 | 1.0000 | 0.9835 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9987 | 0.5607 | 0.7182 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3292 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.7476 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 34297 | 34297 | 1.0000 | 1.0000 |
| data.revision.old | 29964 | 29964 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.7476 | 0.0000 |
| data.server_url | 99995 | 99995 | 0.7476 | 0.0000 |
| data.title | 99995 | 99995 | 0.9987 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.9987 | 0.0000 |
| data.user | 99990 | 99990 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 54 | 128 | 1606 | 0.9288 | 0.9430 | 0.9358 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 1 | 2 | 2 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 249 | 656 | 12783 | 0.7455 | 1.0000 | 0.8542 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5 at 1 paths (most: [("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 771}. Undecodable records: 0.

## Key dev-key-v1.user-global.json (user-global, sha256 8454ec7e6ec60d21e8b714fcc8c74cfc6253e2d63d9efd541a1dcda8ebc473df)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9992 | 0.6781 | 0.8079 | 0.0008 | 0.9894 |
| mapping, without singleton-only types ["event", "log"] | 0.9992 | 0.7697 | 0.8696 |  |  |
| ceiling | 1.0000 | 0.3949 | 0.5662 | 0.0000 | 0.1417 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3131 | 0.4768 |  |  |
| ceiling with links | 0.9993 | 0.9989 | 0.9991 | 0.0007 | 0.9999 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9994 | 0.9987 | 0.9991 | 1.0000 | 0.2500 | 0.4000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9987 | 0.5607 | 0.7182 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3292 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.7476 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 34297 | 34297 | 1.0000 | 1.0000 |
| data.revision.old | 29964 | 29964 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.7476 | 0.0000 |
| data.server_url | 99995 | 99995 | 0.7476 | 0.0000 |
| data.title | 99995 | 99995 | 0.9987 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.9987 | 0.0000 |
| data.user | 99990 | 99990 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 54 | 128 | 1606 | 0.9288 | 0.9430 | 0.9358 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 1 | 2 | 2 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5 at 1 paths (most: [("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 771}. Undecodable records: 0.

## Key dev-key-v1.canonical-mention.json (canonical-mention, sha256 82fa6b0118fb35d3c6f8f446fe78776d84d87a8e8bfe960b0bc9fb6aca1ff4b4)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9873 | 0.5652 | 0.7189 | 0.0127 | 0.9537 |
| mapping, without singleton-only types ["event", "log"] | 0.9873 | 0.7255 | 0.8364 |  |  |
| ceiling | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 1.0000 | 1.0000 |  |  |
| ceiling with links | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9991 | 1.0000 | 0.9995 | 1.0000 | 1.0000 | 1.0000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9675 | 1.0000 | 0.9835 | 1.0000 | 1.0000 | 1.0000 |
| wiki | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3292 | 0 | 0.0000 | 1.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 34297 | 34297 | 1.0000 | 1.0000 |
| data.revision.old | 29964 | 29964 | 1.0000 | 1.0000 |
| data.title | 99995 | 99995 | 1.0000 | 1.0000 |
| data.user | 99990 | 99990 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 1.0000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 54 | 128 | 803 | 0.8856 | 1.0000 | 0.9393 | 1.0000 | 1.0000 | 1.0000 |
| revision @ data.wiki | 1 | 2 | 2 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 249 | 656 | 12783 | 0.7455 | 1.0000 | 0.8542 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 0 at 0 paths (most: []). Key abstained: {}. Excluded (no_identity): {"data.log_id": 771}. Undecodable records: 0.


