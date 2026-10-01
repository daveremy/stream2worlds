# h-measure score

A v0 stream mapping cannot say that different values name one entity; a version-2 mapping can, with links (decision 0027). A key type whose mentions sit at alias paths holding different values (one entity written four ways) scores low recall even for a perfect v0 mapping. Read each mapping row against its ceiling row (the oracle-v0 mapping for that key), its ceiling-with-links row (that mapping plus a rule per alias path, linked into the oracle rule on the same identity, the ceiling for a mapping with links) and the canonical-mention key (alias paths unscored), not against 1.0.

Mapping research/h-measure/committed/h-s2.obf-r2-dev.r2.json (sha256 5a3f492e9598f32473be01906cc045fc6b97d5614e0bdaad032e505807f04955): profiler 9 proposed a mapping from the first 10000 events of obf-r2-dev (sha256 f77717e875f36b46eb0e2a340599dd1da097d56eafeedefac8a7a88a2371ceac); it abstained on 15 paths. Scored on obf-r2-reserved-6 (sha256 5e5d714e83d44b397dd2f1fb407f26b350100db82c92c64f1cc4eae25081905c), 100000 records.

System 2 (arm h-s2, replicate 2, model claude-sonnet-5-5) started from the heuristic above and proposed a mapping, $0.1668 by the price table; its first valid mapping matched the sample; no repair call. Its output is what is graded below.

## Key dev-key-v3.obf-r2.json (base, sha256 6ee8bf5695d120856e792298ae26a801b38813048c1e69392155d972289f1c13)

| row | P | R | F1 | false-merge (mention-weighted) | recovery |
| --- | --- | --- | --- | --- | --- |
| mapping | 0.9889 | 0.1977 | 0.3295 | 0.0111 | 0.0391 |
| mapping, without singleton-only types ["event", "log"] | 0.9889 | 0.2238 | 0.3649 |  |  |
| ceiling | 1.0000 | 0.4069 | 0.5785 | 0.0000 | 0.2060 |
| ceiling, without singleton-only types ["event", "log"] | 1.0000 | 0.3287 | 0.4948 |  |  |
| ceiling with links | 1.0000 | 1.0000 | 1.0000 | 0.0000 | 1.0000 |

| type | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- |
| event | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| log | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| page | 1.0000 | 0.2500 | 0.4000 | 1.0000 | 0.2500 | 0.4000 |
| revision | undefined | 0.0000 | undefined | 1.0000 | 1.0000 | 1.0000 |
| user | 0.9668 | 1.0000 | 0.9831 | 1.0000 | 1.0000 | 1.0000 |
| wiki | 1.0000 | 0.0625 | 0.1176 | 1.0000 | 0.0625 | 0.1176 |

| key path | key mentions | predicted | recall | ceiling recall |
| --- | --- | --- | --- | --- |
| data.f104 | 99995 | 99995 | 0.2500 | 0.0000 |
| data.f17 | 3135 | 0 | 0.0000 | 1.0000 |
| data.f18 | 99995 | 0 | 0.0000 | 0.0000 |
| data.f28 | 99995 | 99995 | 0.5000 | 0.5000 |
| data.f3.f13 | 38647 | 0 | 0.0000 | 1.0000 |
| data.f3.f55 | 43445 | 0 | 0.0000 | 1.0000 |
| data.f37 | 99995 | 0 | 0.0000 | 0.2500 |
| data.f79 | 99995 | 99995 | 1.0000 | 1.0000 |
| data.f89 | 99995 | 0 | 0.0000 | 0.0000 |
| data.f93.f35 | 99995 | 0 | 0.0000 | 0.0000 |
| data.f93.f66 | 100000 | 0 | 0.0000 | 1.0000 |

Context collisions (the composite-key sub-metric, unfloored):

| type @ context | groups | entities | mentions | P | R | F1 | ceiling P | ceiling R | ceiling F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| log @ data.f37 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.f100 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| page @ data.f37 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| revision @ data.f37 | 0 | 0 | 0 | undefined | undefined | undefined | undefined | undefined | undefined |
| user @ data.f37 | 350 | 871 | 11626 | 0.7143 | 1.0000 | 0.8333 | 1.0000 | 1.0000 | 1.0000 |

Spurious predicted mentions: 0 at 0 paths (most: []). Key abstained: {"data.f93.f35": 5}. Excluded (no_identity): {"data.f17": 1515}. Undecodable records: 0.

Relationships (contract B3, unique typed directed edges):

| row | key edges | predicted | TP | FP | FN | P | R | F1 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| mapping | 178265 | 4319 | 0 | 4319 | 178265 | 0.0000 | 0.0000 | undefined |
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

Unaligned predicted edge types (all false): data/f104 → data/f79, owns (4319). Predicted edges with a no-majority endpoint: 262. Dropped (unscored endpoint): 206895. Unobservable key rows: 0 (predicted edges dropped on them: 0). Key edge types with no edge in this corpus: none.


