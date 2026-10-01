# Profile: obf-r2-dev (sha256 f77717e875f36b46eb0e2a340599dd1da097d56eafeedefac8a7a88a2371ceac), first 10000 events, profiler 9

events 10000, skipped 0, decode ["data"], event-type field data.f54

| path | count | distinct | role |
|---|---|---|---|
| `data.f100` | 9997 | 22 | NoDependents |
| `data.f101` | 9997 | 9 | Sequence |
| `data.f104` | 9997 | 99 | NoDependents |
| `data.f17` | 644 | 502 | FewGroups |
| `data.f18` | 9997 | 99 | NoDependents |
| `data.f22` | 644 | 621 | GreyUniqueness |
| `data.f28` | 9997 | 4822 | Entity |
| `data.f29` | 9997 | 289 | NoDependents |
| `data.f3.f13` | 3793 | 3793 | EventId |
| `data.f3.f55` | 4140 | 4140 | EventId |
| `data.f37` | 9997 | 99 | NoDependents |
| `data.f46` | 4140 | 2 | Flag |
| `data.f49` | 9997 | 3497 | Entity |
| `data.f5` | 9997 | 2 | Flag |
| `data.f53.f19` | 143 | 6 | NoDependents |
| `data.f53.f2` | 295 | 294 | EventId |
| `data.f53.f30` | 295 | 171 | Entity |
| `data.f53.f33` | 2 | 1 | Sparse |
| `data.f53.f36` | 1 | 1 | Sparse |
| `data.f53.f39` | 143 | 37 | Entity |
| `data.f53.f42` | 5 | 5 | Sparse |
| `data.f53.f56` | 10 | 1 | Sparse |
| `data.f53.f57` | 2 | 1 | Sparse |
| `data.f53.f64` | 143 | 143 | EventId |
| `data.f53.f68` | 4 | 1 | Sparse |
| `data.f53.f70` | 1 | 1 | Sparse |
| `data.f53.f74` | 2 | 2 | Sparse |
| `data.f53.f80` | 143 | 3 | FewGroups |
| `data.f53.f85` | 10 | 5 | Sparse |
| `data.f53.f88` | 5 | 2 | Sparse |
| `data.f53.f90` | 10 | 10 | Sparse |
| `data.f53.f99` | 20 | 20 | EventId |
| `data.f54` | 10000 | 2 | Flag |
| `data.f58` | 9997 | 4 | FewGroups |
| `data.f65` | 9997 | 3544 | Entity |
| `data.f71` | 9353 | 7038 | Entity |
| `data.f72` | 644 | 12 | NoDependents |
| `data.f73` | 644 | 18 | Category |
| `data.f79` | 9997 | 626 | Entity |
| `data.f81.f27` | 4140 | 2918 | NoDependents |
| `data.f81.f45` | 3793 | 2711 | NoDependents |
| `data.f84` | 2325 | 2 | Flag |
| `data.f89` | 9997 | 4822 | Entity |
| `data.f9` | 9828 | 9828 | EventId |
| `data.f93.f102` | 10000 | 1 | Constant |
| `data.f93.f11` | 10000 | 10000 | EventId |
| `data.f93.f25` | 10000 | 2 | Flag |
| `data.f93.f32` | 9997 | 4822 | Entity |
| `data.f93.f35` | 10000 | 100 | NoDependents |
| `data.f93.f61` | 10000 | 9777 | Timestamp |
| `data.f93.f62` | 10000 | 4704 | NoDependents |
| `data.f93.f63` | 10000 | 1 | Constant |
| `data.f93.f66` | 10000 | 10000 | EventId |
| `id` | 10000 | 9777 | NearUnique |

## Containment (stage 5b)

| referrer | referenced | shared | coverage % | carry % | accepted |
|---|---|---|---|---|---|
| `data.f28` | `data.f89` | 4804 | 99 | 0 | false |
| `data.f28` | `data.f93.f32` | 4804 | 99 | 0 | false |
| `data.f3.f13` | `data.f3.f55` | 541 | 14 | 100 | true |
| `data.f3.f55` | `data.f3.f13` | 541 | 13 | 0 | false |
| `data.f49` | `data.f65` | 279 | 7 | 0 | false |
| `data.f65` | `data.f49` | 279 | 7 | 0 | false |
| `data.f89` | `data.f28` | 4804 | 99 | 0 | false |
| `data.f89` | `data.f93.f32` | 4805 | 99 | 0 | false |
| `data.f93.f32` | `data.f28` | 4804 | 99 | 0 | false |
| `data.f93.f32` | `data.f89` | 4805 | 99 | 0 | false |

## Types (key paths per type label)

- `data/f28+data/f89+f93/f32`: data.f28, data.f89, data.f93.f32
- `data/f49+data/f65`: data.f49, data.f65
- `data/f71`: data.f71
- `data/f79`: data.f79
- `f3/f13+f3/f55`: data.f3.f13, data.f3.f55
- `f53/f30`: data.f53.f30
- `f53/f39`: data.f53.f39

