# Profile: obf-r3-dev (sha256 0fb6e674d9eece9397958e7f11b6cf3719fc4a48780813bf1a2244486a31a1ec), first 10000 events, profiler 9

events 10000, skipped 0, decode ["data"], event-type field data.f5

| path | count | distinct | role |
|---|---|---|---|
| `data.f1` | 9997 | 22 | NoDependents |
| `data.f102` | 644 | 18 | Category |
| `data.f103.f70` | 3793 | 2711 | NoDependents |
| `data.f103.f96` | 4140 | 2918 | NoDependents |
| `data.f11.f32` | 10000 | 10000 | EventId |
| `data.f11.f38` | 10000 | 100 | NoDependents |
| `data.f11.f41` | 10000 | 4704 | NoDependents |
| `data.f11.f57` | 10000 | 1 | Constant |
| `data.f11.f7` | 10000 | 10000 | EventId |
| `data.f11.f72` | 10000 | 9777 | Timestamp |
| `data.f11.f79` | 10000 | 1 | Constant |
| `data.f11.f8` | 10000 | 2 | Flag |
| `data.f11.f93` | 9997 | 4822 | Entity |
| `data.f19` | 9997 | 4822 | Entity |
| `data.f2` | 9828 | 9828 | EventId |
| `data.f29` | 9997 | 4822 | Entity |
| `data.f3` | 9997 | 9 | Sequence |
| `data.f31` | 9353 | 7038 | Entity |
| `data.f33` | 9997 | 2 | Flag |
| `data.f36` | 644 | 621 | GreyUniqueness |
| `data.f39` | 9997 | 99 | NoDependents |
| `data.f4` | 9997 | 626 | Entity |
| `data.f42` | 9997 | 99 | NoDependents |
| `data.f47` | 644 | 12 | NoDependents |
| `data.f5` | 10000 | 2 | Flag |
| `data.f52` | 9997 | 3544 | Entity |
| `data.f53` | 4140 | 2 | Flag |
| `data.f54` | 9997 | 289 | NoDependents |
| `data.f6.f100` | 5 | 5 | Sparse |
| `data.f6.f18` | 2 | 1 | Sparse |
| `data.f6.f21` | 1 | 1 | Sparse |
| `data.f6.f28` | 143 | 6 | NoDependents |
| `data.f6.f40` | 1 | 1 | Sparse |
| `data.f6.f43` | 143 | 143 | EventId |
| `data.f6.f46` | 2 | 1 | Sparse |
| `data.f6.f55` | 2 | 2 | Sparse |
| `data.f6.f58` | 295 | 171 | Entity |
| `data.f6.f68` | 10 | 10 | Sparse |
| `data.f6.f69` | 143 | 3 | FewGroups |
| `data.f6.f74` | 10 | 5 | Sparse |
| `data.f6.f82` | 295 | 294 | EventId |
| `data.f6.f84` | 5 | 2 | Sparse |
| `data.f6.f9` | 20 | 20 | EventId |
| `data.f6.f91` | 10 | 1 | Sparse |
| `data.f6.f94` | 143 | 37 | Entity |
| `data.f6.f95` | 4 | 1 | Sparse |
| `data.f62` | 9997 | 4 | FewGroups |
| `data.f63.f59` | 4140 | 4140 | EventId |
| `data.f63.f61` | 3793 | 3793 | EventId |
| `data.f71` | 644 | 502 | FewGroups |
| `data.f73` | 9997 | 3497 | Entity |
| `data.f75` | 2325 | 2 | Flag |
| `data.f77` | 9997 | 99 | NoDependents |
| `id` | 10000 | 9777 | NearUnique |

## Containment (stage 5b)

| referrer | referenced | shared | coverage % | carry % | accepted |
|---|---|---|---|---|---|
| `data.f11.f93` | `data.f19` | 4804 | 99 | 0 | false |
| `data.f11.f93` | `data.f29` | 4805 | 99 | 0 | false |
| `data.f19` | `data.f11.f93` | 4804 | 99 | 0 | false |
| `data.f19` | `data.f29` | 4804 | 99 | 0 | false |
| `data.f29` | `data.f11.f93` | 4805 | 99 | 0 | false |
| `data.f29` | `data.f19` | 4804 | 99 | 0 | false |
| `data.f52` | `data.f73` | 279 | 7 | 0 | false |
| `data.f63.f59` | `data.f63.f61` | 541 | 13 | 0 | false |
| `data.f63.f61` | `data.f63.f59` | 541 | 14 | 100 | true |
| `data.f73` | `data.f52` | 279 | 7 | 0 | false |

## Types (key paths per type label)

- `data/f19+data/f29+f11/f93`: data.f11.f93, data.f19, data.f29
- `data/f31`: data.f31
- `data/f4`: data.f4
- `data/f52+data/f73`: data.f52, data.f73
- `f6/f58`: data.f6.f58
- `f6/f94`: data.f6.f94
- `f63/f59+f63/f61`: data.f63.f59, data.f63.f61

