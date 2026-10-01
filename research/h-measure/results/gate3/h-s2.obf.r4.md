# h-measure score

A v0 stream mapping cannot say that different values name one entity; a version-2 mapping can, with links (decision 0027). A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the oracle-v0 mapping for that key), its ceiling-with-links row (that mapping plus a rule per alias path, linked into the oracle rule on the same identity, the ceiling for a mapping with links) and the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/committed/h-s2.obf-r4-dev.r4.json (sha256 ce7547eb3acb2ea41ab2351cb6e544b84848a4756707241148709ddafedb4e54): profiler 9 proposed a mapping from the first 10000 events of obf-r4-dev (sha256 10096e68682e88b77e47fa81bac2e45fc749c47e760868c975997f1425da513d); it abstained on 15 paths. Scored on obf-r4-reserved-6 (sha256 ce13f3b89acea1c88965982c9cd324f2d42b3cdc2bc309d4f4fff96e0942f783), 100000 records.

System 2 (arm h-s2, replicate 4, model claude-sonnet-5-5) started from the heuristic above and proposed a mapping, $0.1640 by the price table; its first valid mapping matched the sample; no repair call. Its output is what is graded below.

## Key dev-key-v3.obf-r4.json (base, sha256 51a56df258214fa7ec1a5aeda3714e0b148fd0b1c8a12bc1117c1fa2850e3725)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9889 | 0.1977 | 0.3295 | 0.0111 | 0.0391 |
| mapping, without singleton-only types ["event", "log"] | 0.9889 | 0.2237 | 0.3649 |  |  |
| ceiling | 1.0000 | 0.4069 | 0.5785 | 0.0000 | 0.2060 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3287 | 0.4948 |  |  |
| ceiling with links | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.2500 | 0.3999 | 1.0000 | 0.2500 | 0.4000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9668 | 1.0000 | 0.9831 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 1.0000 | 0.0625 | 0.1176 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.f101.f4 | 99995 | 99995 | 0.2500 | 0.0000 |
| data.f101.f89 | 100000 | 0 | 0.0000 | 1.0000 |
| data.f12 | 3135 | 0 | 0.0000 | 1.0000 |
| data.f16 | 99995 | 0 | 0.0000 | 0.0000 |
| data.f23 | 99995 | 0 | 0.0000 | 0.2500 |
| data.f45 | 99995 | 99995 | 0.4999 | 0.0000 |
| data.f48 | 99995 | 0 | 0.0000 | 0.0000 |
| data.f49 | 99995 | 0 | 0.0000 | 0.5000 |
| data.f58 | 99995 | 99995 | 1.0000 | 1.0000 |
| data.f99.f19 | 38647 | 0 | 0.0000 | 1.0000 |
| data.f99.f96 | 43445 | 0 | 0.0000 | 1.0000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.f23 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.f23 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.f65 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| revision @ data.f23 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| user @ data.f23 | 350 | 871 | 11626 | 0.7143 | 1.0000 | 0.8333 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 5 at 1 paths (most: [("data.f101.f4", 5)]). Key abstained: {"data.f101.f4": 5}. Excluded (no_identity): {"data.f12": 1515}. Undecodable records: 0.

Relationships (contract B3, unique typed directed edges):

| row | key edges | predicted | TP | FP | FN | P | R | F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mapping | 178265 | 86473 | 42139 | 44334 | 136126 | 0.4873 | 0.2364 | 0.3183 |
| ceiling | 178265 | 178265 | 178265 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 |
| ceiling with links | 178265 | 178265 | 178265 | 0 | 0 | 1.0000 | 1.0000 | 1.0000 |

| key edge type | key edges | aligned predicted type | TP | FP | FN | P | R | F1 | ceiling R |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| by | 43445 | (unaligned) | 0 | 0 | 43445 | undefined | 0.0000 | undefined | 1.0000 |
| edits | 43445 | (unaligned) | 0 | 0 | 43445 | undefined | 0.0000 | undefined | 1.0000 |
| follows | 38647 | (unaligned) | 0 | 0 | 38647 | undefined | 0.0000 | undefined | 1.0000 |
| logged-by | 3135 | (unaligned) | 0 | 0 | 3135 | undefined | 0.0000 | undefined | 1.0000 |
| page-on | 42139 | f45 → group, belongs to | 42139 | 12 | 0 | 0.9997 | 1.0000 | 0.9999 | 1.0000 |
| targets | 3135 | (unaligned) | 0 | 0 | 3135 | undefined | 0.0000 | undefined | 1.0000 |
| user-on | 4319 | (unaligned) | 0 | 0 | 4319 | undefined | 0.0000 | undefined | 1.0000 |

Unaligned predicted edge types (all false): f58 → f45, related to (44322). Predicted edges with a no-majority endpoint: 1338. Dropped (unscored endpoint): 176975. Unobservable key rows: 0 (predicted edges dropped on them: 0). Key edge types with no edge in this corpus: none.


