# Profile: private-dev (sha256 1e3fea5a96878a33ac603f3a5e98ffbe51f15e5e92d0fcb8d6c49fa931d4507d), first 8067 events, profiler 8

events 8067, skipped 0, decode ["data"], event-type field data.kind

| path | count | distinct | role |
|---|---|---|---|
| `data.actor` | 4923 | 1 | Constant |
| `data.author` | 1070 | 1 | Constant |
| `data.base_ref` | 223 | 2 | Flag |
| `data.branch` | 361 | 105 | Other |
| `data.co_authored` | 1070 | 2 | Flag |
| `data.comment_bytes` | 679 | 601 | NoDependents |
| `data.comment_id` | 679 | 679 | EventId |
| `data.commit_sha` | 4230 | 553 | Other |
| `data.engine` | 242 | 6 | NoDependents |
| `data.engines` | 112 | 57 | NoDependents |
| `data.event` | 4230 | 8 | NoDependents |
| `data.file_id` | 57 | 57 | EventId |
| `data.head_ref` | 223 | 223 | EventId |
| `data.head_sha` | 464 | 269 | Sequence |
| `data.is_pr` | 693 | 2 | Flag |
| `data.issue` | 2017 | 169 | Other |
| `data.key` | 1775 | 169 | NoDependents |
| `data.kind` | 8067 | 9 | NoDependents |
| `data.label` | 4230 | 23 | Other |
| `data.leg` | 271 | 7 | NoDependents |
| `data.merge_commit_sha` | 222 | 217 | Other |
| `data.model` | 242 | 8 | NoDependents |
| `data.number` | 4923 | 674 | NoDependents |
| `data.pr` | 311 | 151 | Entity |
| `data.pts` | 389 | 4 | Other |
| `data.ref_number` | 4230 | 418 | Other |
| `data.ref_repo` | 4230 | 2 | Other |
| `data.repo` | 8067 | 2 | Flag |
| `data.round` | 407 | 6 | NoDependents |
| `data.row_id` | 1775 | 1775 | EventId |
| `data.script` | 242 | 2 | Flag |
| `data.seat_id` | 242 | 43 | Other |
| `data.sha` | 1175 | 1070 | GreyUniqueness |
| `data.slot` | 57 | 56 | EventId |
| `data.sprint` | 57 | 57 | EventId |
| `data.step` | 1775 | 17 | NoDependents |
| `data.tier` | 242 | 2 | Flag |
| `data.ts` | 8067 | 6187 | Timestamp |
| `data.ts_end` | 242 | 152 | Timestamp |
| `data.ts_start` | 242 | 143 | Timestamp |
| `data.verdict` | 389 | 4 | Other |
| `id` | 8067 | 8067 | EventId |

## Containment (stage 5b)

| referrer | referenced | shared | coverage % | carry % | accepted |
|---|---|---|---|---|---|

## Types (key paths per type label)

- `data/pr`: data.pr

