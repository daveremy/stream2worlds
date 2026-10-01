# h-measure score

A v0 stream mapping cannot say that different values name one entity; a version-2 mapping can, with links (decision 0027). A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the oracle-v0 mapping for that key), its ceiling-with-links row (that mapping plus a rule per alias path, linked into the oracle rule on the same identity, the ceiling for a mapping with links) and the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/frozen/h-min-v9.obf-r6-dev-10000.json (sha256 0e5d7bb8d9a9e139da80fd441a83a0fe080b5f5d3f3dca2bc00290c1a1fcc914): profiler 9 proposed a mapping from the first 10000 events of obf-r6-dev (sha256 91948fb864b0752ea08e3980ef484f48a5acb2fc3d5f05c935a4478cc5526091); it abstained on 15 paths. Scored on obf-r6-reserved-6 (sha256 11dff7ce49bda518a20a71f8378ea5c56b58d06249d44645ddcf6458b010a42f), 100000 records.

## Key dev-key-v3.obf-r6.json (base, sha256 eb6974232e1ebb2cb082c312d4950ec173225a303e5ad8fb8948666c397f28d5)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9913 | 0.4313 | 0.6011 | 0.0087 | 0.9819 |
| mapping, without singleton-only types ["event", "log"] | 0.9913 | 0.4882 | 0.6542 |  |  |
| ceiling | 1.0000 | 0.4069 | 0.5785 | 0.0000 | 0.2060 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3287 | 0.4948 |  |  |
| ceiling with links | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.9987 | 0.9993 | 1.0000 | 0.2500 | 0.4000 |
| revision | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9668 | 1.0000 | 0.9831 | 1.0000 | 1.0000 | 1.0000 |
| wiki | undefined | 0.0000 | undefined | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.f100 | 99995 | 99995 | 0.9986 | 0.0000 |
| data.f27 | 99995 | 0 | 0.0000 | 0.0000 |
| data.f31 | 99995 | 0 | 0.0000 | 0.0000 |
| data.f36 | 99995 | 99995 | 1.0000 | 1.0000 |
| data.f4 | 99995 | 0 | 0.0000 | 0.2500 |
| data.f45.f80 | 43445 | 43445 | 1.0000 | 1.0000 |
| data.f45.f92 | 38647 | 38647 | 1.0000 | 1.0000 |
| data.f52.f30 | 100000 | 0 | 0.0000 | 1.0000 |
| data.f52.f49 | 99995 | 0 | 0.0000 | 0.0000 |
| data.f55 | 3135 | 0 | 0.0000 | 1.0000 |
| data.f93 | 99995 | 99995 | 0.9987 | 0.5000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.f4 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.f4 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.f79 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| revision @ data.f4 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| user @ data.f4 | 350 | 871 | 11626 | 0.7143 | 1.0000 | 0.8333 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 0 at 0 paths (most: []). Key abstained: {"data.f52.f49": 5}. Excluded (no_identity): {"data.f55": 1515}. Undecodable records: 0.

Relationships (contract B3, unique typed directed edges):

| row | key edges | predicted | TP | FP | FN | P | R | F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mapping | 178265 | 44564 | 0 | 44564 | 178265 | 0.0000 | 0.0000 | undefined |
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

Unaligned predicted edge types (all false): data/f36 → data/f100+data/f93+f52/f76, n:m (44564). Predicted edges with a no-majority endpoint: 1339. Dropped (unscored endpoint): 121337. Unobservable key rows: 0 (predicted edges dropped on them: 0). Key edge types with no edge in this corpus: none.


