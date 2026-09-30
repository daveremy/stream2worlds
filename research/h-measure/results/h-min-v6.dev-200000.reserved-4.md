# h-measure score

A v0 stream mapping cannot say that different values name one entity. A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the oracle-v0 mapping for that key) and against the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/frozen/h-min-v6.dev-200000.json (sha256 fd1cee508c660dd87cd2e79770995ac2ce654e59c7452d52135f0244eb1f7048): profiler 6 proposed a mapping from the first 200000 events of dev (sha256 53d35b1c8631cc506d762a855ea554d4ca833f5c02a6fe80c5ceeb52205ae4dd); it abstained on 18 paths. Scored on reserved-4 (sha256 dfa075fbe5b58b368e7d3d895d9e27296bd75e695bd7f2150e5b130ac0e0166f), 100000 records.

## Key dev-key-v1.json (base, sha256 006c50d4a8067c620a157534cb17f5cdfcf64f58ff275184aafaaefc55a48595)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9875 | 0.4254 | 0.5946 | 0.0125 | 0.1281 |
| mapping, without singleton-only types ["event", "log"] | 0.9875 | 0.4824 | 0.6482 |  |  |
| ceiling | 1.0000 | 0.4015 | 0.5730 | 0.0000 | 0.1347 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3213 | 0.4863 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9992 | 0.5000 | 0.6665 | 1.0000 | 0.2500 | 0.4000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9761 | 1.0000 | 0.9879 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9986 | 0.2492 | 0.3988 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3754 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4983 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 38686 | 38686 | 1.0000 | 1.0000 |
| data.revision.old | 34807 | 34807 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4983 | 0.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 0.0000 |
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.5000 | 0.0000 |
| data.user | 99994 | 99994 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 119 | 247 | 778 | 0.8008 | 0.4979 | 0.6141 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 1 | 2 | 2 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |
| user @ data.wiki | 198 | 572 | 9785 | 0.7559 | 1.0000 | 0.8610 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 4414 at 2 paths (most: [("data.log_action", 4409), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 655}. Undecodable records: 0.

## Key dev-key-v1.user-global.json (user-global, sha256 8454ec7e6ec60d21e8b714fcc8c74cfc6253e2d63d9efd541a1dcda8ebc473df)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9916 | 0.4254 | 0.5953 | 0.0084 | 0.1315 |
| mapping, without singleton-only types ["event", "log"] | 0.9916 | 0.4824 | 0.6491 |  |  |
| ceiling | 1.0000 | 0.4015 | 0.5730 | 0.0000 | 0.1315 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3213 | 0.4863 |  |  |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 0.9992 | 0.5000 | 0.6665 | 1.0000 | 0.2500 | 0.4000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 0.9986 | 0.2492 | 0.3988 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.log_id | 3754 | 0 | 0.0000 | 1.0000 |
| data.meta.domain | 99995 | 99995 | 0.4983 | 0.0000 |
| data.meta.id | 100000 | 0 | 0.0000 | 1.0000 |
| data.revision.new | 38686 | 38686 | 1.0000 | 1.0000 |
| data.revision.old | 34807 | 34807 | 1.0000 | 1.0000 |
| data.server_name | 99995 | 99995 | 0.4983 | 0.0000 |
| data.server_url | 99995 | 0 | 0.0000 | 0.0000 |
| data.title | 99995 | 99995 | 0.5000 | 0.5000 |
| data.title_url | 99995 | 99995 | 0.5000 | 0.0000 |
| data.user | 99994 | 99994 | 1.0000 | 1.0000 |
| data.wiki | 99995 | 0 | 0.0000 | 0.2500 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.wiki | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.namespace | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.wiki | 119 | 247 | 778 | 0.8008 | 0.4979 | 0.6141 | 1.0000 | 0.2500 | 0.4000 |
| revision @ data.wiki | 1 | 2 | 2 | 0.5000 | 1.0000 | 0.6667 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 4414 at 2 paths (most: [("data.log_action", 4409), ("data.meta.domain", 5)]). Key abstained: {"data.meta.domain": 5}. Excluded (no_identity): {"data.log_id": 655}. Undecodable records: 0.


