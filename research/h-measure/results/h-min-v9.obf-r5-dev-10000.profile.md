# Profile: obf-r5-dev (sha256 d5e5e17f8052fcdf32a74aad8def392763f6f026a21004f28de2253292f02aab), first 10000 events, profiler 9

events 10000, skipped 0, decode ["data"], event-type field data.f79

| path | count | distinct | role |
|---|---|---|---|
| `data.f1` | 644 | 502 | FewGroups |
| `data.f10` | 9997 | 99 | NoDependents |
| `data.f104` | 9997 | 99 | NoDependents |
| `data.f12` | 9997 | 3497 | Entity |
| `data.f13` | 9997 | 3544 | Entity |
| `data.f14` | 9353 | 7038 | Entity |
| `data.f25` | 644 | 18 | Category |
| `data.f40` | 4140 | 2 | Flag |
| `data.f43` | 644 | 621 | GreyUniqueness |
| `data.f49` | 9828 | 9828 | EventId |
| `data.f55.f15` | 10000 | 10000 | EventId |
| `data.f55.f2` | 10000 | 10000 | EventId |
| `data.f55.f28` | 10000 | 1 | Constant |
| `data.f55.f54` | 10000 | 100 | NoDependents |
| `data.f55.f60` | 10000 | 9777 | Timestamp |
| `data.f55.f65` | 10000 | 2 | Flag |
| `data.f55.f69` | 10000 | 4704 | NoDependents |
| `data.f55.f72` | 9997 | 4822 | Entity |
| `data.f55.f99` | 10000 | 1 | Constant |
| `data.f6` | 9997 | 626 | Entity |
| `data.f61` | 9997 | 9 | Sequence |
| `data.f62.f78` | 4140 | 4140 | EventId |
| `data.f62.f87` | 3793 | 3793 | EventId |
| `data.f63` | 9997 | 99 | NoDependents |
| `data.f66` | 9997 | 289 | NoDependents |
| `data.f74` | 9997 | 4822 | Entity |
| `data.f75` | 9997 | 22 | NoDependents |
| `data.f79` | 10000 | 2 | Flag |
| `data.f83` | 644 | 12 | NoDependents |
| `data.f84` | 9997 | 2 | Flag |
| `data.f91.f101` | 143 | 37 | Entity |
| `data.f91.f17` | 5 | 2 | Sparse |
| `data.f91.f18` | 10 | 10 | Sparse |
| `data.f91.f20` | 295 | 294 | EventId |
| `data.f91.f26` | 1 | 1 | Sparse |
| `data.f91.f30` | 2 | 1 | Sparse |
| `data.f91.f34` | 5 | 5 | Sparse |
| `data.f91.f35` | 2 | 1 | Sparse |
| `data.f91.f47` | 143 | 3 | FewGroups |
| `data.f91.f48` | 10 | 1 | Sparse |
| `data.f91.f64` | 143 | 143 | EventId |
| `data.f91.f7` | 295 | 171 | Entity |
| `data.f91.f73` | 10 | 5 | Sparse |
| `data.f91.f81` | 1 | 1 | Sparse |
| `data.f91.f86` | 4 | 1 | Sparse |
| `data.f91.f88` | 143 | 6 | NoDependents |
| `data.f91.f9` | 2 | 2 | Sparse |
| `data.f91.f93` | 20 | 20 | EventId |
| `data.f94` | 9997 | 4822 | Entity |
| `data.f95.f22` | 3793 | 2711 | NoDependents |
| `data.f95.f39` | 4140 | 2918 | NoDependents |
| `data.f96` | 9997 | 4 | FewGroups |
| `data.f98` | 2325 | 2 | Flag |
| `id` | 10000 | 9777 | NearUnique |

## Containment (stage 5b)

| referrer | referenced | shared | coverage % | carry % | accepted |
|---|---|---|---|---|---|
| `data.f12` | `data.f13` | 279 | 7 | 0 | false |
| `data.f13` | `data.f12` | 279 | 7 | 0 | false |
| `data.f55.f72` | `data.f74` | 4804 | 99 | 0 | false |
| `data.f55.f72` | `data.f94` | 4805 | 99 | 0 | false |
| `data.f62.f78` | `data.f62.f87` | 541 | 13 | 0 | false |
| `data.f62.f87` | `data.f62.f78` | 541 | 14 | 100 | true |
| `data.f74` | `data.f55.f72` | 4804 | 99 | 0 | false |
| `data.f74` | `data.f94` | 4804 | 99 | 0 | false |
| `data.f94` | `data.f55.f72` | 4805 | 99 | 0 | false |
| `data.f94` | `data.f74` | 4804 | 99 | 0 | false |

## Types (key paths per type label)

- `data/f12+data/f13`: data.f12, data.f13
- `data/f14`: data.f14
- `data/f6`: data.f6
- `data/f74+data/f94+f55/f72`: data.f55.f72, data.f74, data.f94
- `f62/f78+f62/f87`: data.f62.f78, data.f62.f87
- `f91/f101`: data.f91.f101
- `f91/f7`: data.f91.f7

