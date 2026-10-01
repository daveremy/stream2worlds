# Profile: obf-r4-dev (sha256 10096e68682e88b77e47fa81bac2e45fc749c47e760868c975997f1425da513d), first 10000 events, profiler 9

events 10000, skipped 0, decode ["data"], event-type field data.f26

| path | count | distinct | role |
|---|---|---|---|
| `data.f100` | 644 | 18 | Category |
| `data.f101.f17` | 10000 | 9777 | Timestamp |
| `data.f101.f31` | 10000 | 2 | Flag |
| `data.f101.f35` | 10000 | 10000 | EventId |
| `data.f101.f4` | 10000 | 100 | NoDependents |
| `data.f101.f62` | 10000 | 4704 | NoDependents |
| `data.f101.f76` | 9997 | 4822 | Entity |
| `data.f101.f86` | 10000 | 1 | Constant |
| `data.f101.f89` | 10000 | 10000 | EventId |
| `data.f101.f98` | 10000 | 1 | Constant |
| `data.f102` | 9353 | 7038 | Entity |
| `data.f12` | 644 | 502 | FewGroups |
| `data.f16` | 9997 | 99 | NoDependents |
| `data.f23` | 9997 | 99 | NoDependents |
| `data.f26` | 10000 | 2 | Flag |
| `data.f29` | 4140 | 2 | Flag |
| `data.f32` | 9997 | 4 | FewGroups |
| `data.f33` | 2325 | 2 | Flag |
| `data.f43` | 9997 | 3544 | Entity |
| `data.f45` | 9997 | 4822 | Entity |
| `data.f48` | 9997 | 99 | NoDependents |
| `data.f49` | 9997 | 4822 | Entity |
| `data.f52` | 644 | 621 | GreyUniqueness |
| `data.f58` | 9997 | 626 | Entity |
| `data.f59` | 9997 | 2 | Flag |
| `data.f65` | 9997 | 22 | NoDependents |
| `data.f66` | 9997 | 3497 | Entity |
| `data.f67.f20` | 3793 | 2711 | NoDependents |
| `data.f67.f85` | 4140 | 2918 | NoDependents |
| `data.f73` | 644 | 12 | NoDependents |
| `data.f78` | 9997 | 289 | NoDependents |
| `data.f80` | 9828 | 9828 | EventId |
| `data.f83` | 9997 | 9 | Sequence |
| `data.f94.f18` | 10 | 10 | Sparse |
| `data.f94.f21` | 20 | 20 | EventId |
| `data.f94.f25` | 1 | 1 | Sparse |
| `data.f94.f36` | 5 | 5 | Sparse |
| `data.f94.f37` | 2 | 1 | Sparse |
| `data.f94.f42` | 143 | 3 | FewGroups |
| `data.f94.f46` | 10 | 1 | Sparse |
| `data.f94.f50` | 143 | 143 | EventId |
| `data.f94.f51` | 143 | 37 | Entity |
| `data.f94.f53` | 10 | 5 | Sparse |
| `data.f94.f54` | 5 | 2 | Sparse |
| `data.f94.f61` | 4 | 1 | Sparse |
| `data.f94.f64` | 143 | 6 | NoDependents |
| `data.f94.f68` | 295 | 171 | Entity |
| `data.f94.f69` | 2 | 1 | Sparse |
| `data.f94.f71` | 295 | 294 | EventId |
| `data.f94.f74` | 1 | 1 | Sparse |
| `data.f94.f79` | 2 | 2 | Sparse |
| `data.f99.f19` | 3793 | 3793 | EventId |
| `data.f99.f96` | 4140 | 4140 | EventId |
| `id` | 10000 | 9777 | NearUnique |

## Containment (stage 5b)

| referrer | referenced | shared | coverage % | carry % | accepted |
|---|---|---|---|---|---|
| `data.f101.f76` | `data.f45` | 4805 | 99 | 0 | false |
| `data.f101.f76` | `data.f49` | 4804 | 99 | 0 | false |
| `data.f43` | `data.f66` | 279 | 7 | 0 | false |
| `data.f45` | `data.f101.f76` | 4805 | 99 | 0 | false |
| `data.f45` | `data.f49` | 4804 | 99 | 0 | false |
| `data.f49` | `data.f101.f76` | 4804 | 99 | 0 | false |
| `data.f49` | `data.f45` | 4804 | 99 | 0 | false |
| `data.f66` | `data.f43` | 279 | 7 | 0 | false |
| `data.f99.f19` | `data.f99.f96` | 541 | 14 | 100 | true |
| `data.f99.f96` | `data.f99.f19` | 541 | 13 | 0 | false |

## Types (key paths per type label)

- `data/f102`: data.f102
- `data/f43+data/f66`: data.f43, data.f66
- `data/f45+data/f49+f101/f76`: data.f101.f76, data.f45, data.f49
- `data/f58`: data.f58
- `f94/f51`: data.f94.f51
- `f94/f68`: data.f94.f68
- `f99/f19+f99/f96`: data.f99.f19, data.f99.f96

