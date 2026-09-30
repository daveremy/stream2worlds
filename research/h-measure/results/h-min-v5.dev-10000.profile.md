# Profile: dev (sha256 53d35b1c8631cc506d762a855ea554d4ca833f5c02a6fe80c5ceeb52205ae4dd), first 10000 events, profiler 5

events 10000, skipped 0, decode ["data"], event-type field data.$schema

| path | count | distinct | role |
|---|---|---|---|
| `data.$schema` | 10000 | 2 | Flag |
| `data.bot` | 9997 | 2 | Flag |
| `data.comment` | 9997 | 3497 | Entity |
| `data.id` | 9828 | 9828 | EventId |
| `data.length.new` | 4140 | 2918 | NoDependents |
| `data.length.old` | 3793 | 2711 | NoDependents |
| `data.log_action` | 644 | 18 | Category |
| `data.log_action_comment` | 644 | 621 | GreyUniqueness |
| `data.log_id` | 644 | 502 | FewGroups |
| `data.log_params.action` | 143 | 3 | FewGroups |
| `data.log_params.actions` | 143 | 6 | NoDependents |
| `data.log_params.auto` | 10 | 1 | Sparse |
| `data.log_params.count` | 4 | 1 | Sparse |
| `data.log_params.curid` | 10 | 10 | Sparse |
| `data.log_params.filter` | 143 | 37 | Entity |
| `data.log_params.img_sha1` | 295 | 294 | EventId |
| `data.log_params.img_timestamp` | 295 | 171 | Entity |
| `data.log_params.log` | 143 | 143 | EventId |
| `data.log_params.newmodel` | 1 | 1 | Sparse |
| `data.log_params.nfield` | 2 | 2 | Sparse |
| `data.log_params.noredir` | 5 | 2 | Sparse |
| `data.log_params.ofield` | 2 | 1 | Sparse |
| `data.log_params.oldmodel` | 1 | 1 | Sparse |
| `data.log_params.previd` | 10 | 3 | Sparse |
| `data.log_params.target` | 5 | 5 | Sparse |
| `data.log_params.type` | 2 | 1 | Sparse |
| `data.log_params.userid` | 20 | 20 | EventId |
| `data.log_type` | 644 | 12 | NoDependents |
| `data.meta.domain` | 10000 | 101 | Entity |
| `data.meta.dt` | 10000 | 9777 | NearUnique |
| `data.meta.id` | 10000 | 10000 | EventId |
| `data.meta.offset` | 10000 | 10000 | EventId |
| `data.meta.partition` | 10000 | 1 | Constant |
| `data.meta.request_id` | 10000 | 4704 | NoDependents |
| `data.meta.stream` | 10000 | 1 | Constant |
| `data.meta.topic` | 10000 | 2 | Flag |
| `data.meta.uri` | 9997 | 4822 | Entity |
| `data.minor` | 4140 | 2 | Flag |
| `data.namespace` | 9997 | 22 | NoDependents |
| `data.notify_url` | 9353 | 7038 | Entity |
| `data.parsedcomment` | 9997 | 3544 | Entity |
| `data.patrolled` | 2325 | 2 | Flag |
| `data.revision.new` | 4140 | 4140 | EventId |
| `data.revision.old` | 3793 | 3793 | EventId |
| `data.server_name` | 9997 | 100 | Entity |
| `data.server_script_path` | 9997 | 9 | Sequence |
| `data.server_url` | 9997 | 100 | Entity |
| `data.timestamp` | 9997 | 289 | NoDependents |
| `data.title` | 9997 | 4819 | Entity |
| `data.title_url` | 9997 | 4822 | Entity |
| `data.type` | 9997 | 4 | FewGroups |
| `data.user` | 9997 | 626 | Entity |
| `data.wiki` | 9997 | 99 | GreyDependency |
| `id` | 10000 | 9777 | NearUnique |

## Containment (stage 5b)

| referrer | referenced | shared | coverage % | carry % | accepted |
|---|---|---|---|---|---|
| `data.comment` | `data.parsedcomment` | 279 | 7 | 0 | false |
| `data.meta.domain` | `data.server_name` | 100 | 99 | 0 | false |
| `data.meta.uri` | `data.title_url` | 4805 | 99 | 0 | false |
| `data.parsedcomment` | `data.comment` | 279 | 7 | 0 | false |
| `data.revision.new` | `data.revision.old` | 541 | 13 | 0 | false |
| `data.revision.old` | `data.revision.new` | 541 | 14 | 100 | true |
| `data.server_name` | `data.meta.domain` | 100 | 100 | 0 | false |
| `data.title_url` | `data.meta.uri` | 4805 | 99 | 0 | false |

## Types (key paths per type label)

- `data/notify_url`: data.notify_url
- `data/parsedcomment`: data.parsedcomment
- `data/server_name+meta/domain`: data.meta.domain, data.server_name
- `data/title_url+meta/uri`: data.meta.uri, data.title_url
- `data/user`: data.user
- `log_params/filter`: data.log_params.filter
- `log_params/img_timestamp`: data.log_params.img_timestamp
- `revision/new+revision/old`: data.revision.new, data.revision.old

