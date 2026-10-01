# Profile: obf-r6-dev (sha256 91948fb864b0752ea08e3980ef484f48a5acb2fc3d5f05c935a4478cc5526091), first 10000 events, profiler 9

events 10000, skipped 0, decode ["data"], event-type field data.f91

| path | count | distinct | role |
|---|---|---|---|
| `data.f100` | 9997 | 4822 | Entity |
| `data.f19` | 9997 | 2 | Flag |
| `data.f2` | 644 | 621 | GreyUniqueness |
| `data.f20` | 9997 | 9 | Sequence |
| `data.f27` | 9997 | 99 | NoDependents |
| `data.f3` | 9828 | 9828 | EventId |
| `data.f31` | 9997 | 99 | NoDependents |
| `data.f36` | 9997 | 626 | Entity |
| `data.f39.f104` | 4140 | 2918 | NoDependents |
| `data.f39.f62` | 3793 | 2711 | NoDependents |
| `data.f4` | 9997 | 99 | NoDependents |
| `data.f45.f80` | 4140 | 4140 | EventId |
| `data.f45.f92` | 3793 | 3793 | EventId |
| `data.f46` | 9997 | 4 | FewGroups |
| `data.f48` | 9997 | 3497 | Entity |
| `data.f52.f16` | 10000 | 1 | Constant |
| `data.f52.f18` | 10000 | 1 | Constant |
| `data.f52.f30` | 10000 | 10000 | EventId |
| `data.f52.f49` | 10000 | 100 | NoDependents |
| `data.f52.f57` | 10000 | 4704 | NoDependents |
| `data.f52.f69` | 10000 | 2 | Flag |
| `data.f52.f76` | 9997 | 4822 | Entity |
| `data.f52.f78` | 10000 | 9777 | Timestamp |
| `data.f52.f85` | 10000 | 10000 | EventId |
| `data.f53.f22` | 1 | 1 | Sparse |
| `data.f53.f33` | 10 | 5 | Sparse |
| `data.f53.f38` | 295 | 294 | EventId |
| `data.f53.f47` | 5 | 5 | Sparse |
| `data.f53.f50` | 5 | 2 | Sparse |
| `data.f53.f60` | 143 | 37 | Entity |
| `data.f53.f61` | 143 | 6 | NoDependents |
| `data.f53.f64` | 20 | 20 | EventId |
| `data.f53.f67` | 2 | 1 | Sparse |
| `data.f53.f74` | 10 | 1 | Sparse |
| `data.f53.f81` | 143 | 3 | FewGroups |
| `data.f53.f82` | 295 | 171 | Entity |
| `data.f53.f83` | 2 | 2 | Sparse |
| `data.f53.f84` | 4 | 1 | Sparse |
| `data.f53.f86` | 2 | 1 | Sparse |
| `data.f53.f88` | 1 | 1 | Sparse |
| `data.f53.f96` | 143 | 143 | EventId |
| `data.f53.f98` | 10 | 10 | Sparse |
| `data.f55` | 644 | 502 | FewGroups |
| `data.f58` | 9353 | 7038 | Entity |
| `data.f66` | 9997 | 289 | NoDependents |
| `data.f7` | 9997 | 3544 | Entity |
| `data.f71` | 2325 | 2 | Flag |
| `data.f73` | 644 | 18 | Category |
| `data.f77` | 4140 | 2 | Flag |
| `data.f79` | 9997 | 22 | NoDependents |
| `data.f91` | 10000 | 2 | Flag |
| `data.f93` | 9997 | 4822 | Entity |
| `data.f99` | 644 | 12 | NoDependents |
| `id` | 10000 | 9777 | NearUnique |

## Containment (stage 5b)

| referrer | referenced | shared | coverage % | carry % | accepted |
|---|---|---|---|---|---|
| `data.f100` | `data.f52.f76` | 4805 | 99 | 0 | false |
| `data.f100` | `data.f93` | 4804 | 99 | 0 | false |
| `data.f45.f80` | `data.f45.f92` | 541 | 13 | 0 | false |
| `data.f45.f92` | `data.f45.f80` | 541 | 14 | 100 | true |
| `data.f48` | `data.f7` | 279 | 7 | 0 | false |
| `data.f52.f76` | `data.f100` | 4805 | 99 | 0 | false |
| `data.f52.f76` | `data.f93` | 4804 | 99 | 0 | false |
| `data.f7` | `data.f48` | 279 | 7 | 0 | false |
| `data.f93` | `data.f100` | 4804 | 99 | 0 | false |
| `data.f93` | `data.f52.f76` | 4804 | 99 | 0 | false |

## Types (key paths per type label)

- `data/f100+data/f93+f52/f76`: data.f100, data.f52.f76, data.f93
- `data/f36`: data.f36
- `data/f48+data/f7`: data.f48, data.f7
- `data/f58`: data.f58
- `f45/f80+f45/f92`: data.f45.f80, data.f45.f92
- `f53/f60`: data.f53.f60
- `f53/f82`: data.f53.f82

