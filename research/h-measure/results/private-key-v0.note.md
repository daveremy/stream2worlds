# private-key-v0: verifier report and hand-inspected sample on private-test

Answer key `private-key-v0.json` (base, sha256 `f5e283b98e6022bf530527857ef63b2100bead90f80661785546954981f411c2`)
and its `context-scored` variant, checked on the reserved corpus `private-test` (s2w#372 PR 2,
plan §5 row 2 and Q6). Run 2026-10-01 (MST) on hub from `main` @ `1e2e9bd`.

**Result:** the verifier passes with 0 rule failures, and the hand inspection agrees with the
key on 60 of 60 sampled mentions. The key stays at v0. The inspection found one capture bug
(sprint rows dropped, filed as s2w#398, a #371 follow-up) and one wrong description in the
README (the `seat` type is not singleton-only). Neither changes a key rule.

This note holds counts, rule names and paths only. It holds no frame value and no `detail` text;
the 60-item worksheet stayed in `~/.local/share/stream2worlds/h-measure/` (mode 0600, outside
every git tree).

## Inputs

| | file | sha256 | matches `corpora.toml` |
|---|---|---|---|
| corpus | `private-test.raw.sse` (5501 events) | `f19f69eddb2e4528f04d35e6307869b48131a5dfcf7cdb339f32e4d931109357` | yes |
| sidecar | `private-test.provenance.jsonl` | `f4d997f29de2e9fb0f0bc2dd1eeee2efcd1ee7792234e47053669a41e912db1b` | yes |

## Verifier report (verbatim)

`node --experimental-strip-types research/h-measure/private/key.ts --check --corpus <dir>/private-test.raw.sse --provenance <dir>/private-test.provenance.jsonl`
exited 0 and printed:

```json
{
  "frames": 5501,
  "undecodable": 0,
  "capture_header": [
    "capture.ts --name private-test --since 2026-09-29T00:00:00Z --until 2026-10-01T00:00:00Z --dir <dir>",
    "events=5501 dropped=outside-span:717,timeline-other-kind:1168,worker-other-repo:191 max_worker_row=11514 extraction: pr=\\bPR #(\\d+)\\b sha=@([0-9a-f]{7,40})\\b branch=\\b((?:feat|fix|chore|docs|pair)\\/[A-Za-z0-9._-]+(?:\\/[A-Za-z0-9._-]+)*) leg=\\bleg ([A-Z])\\b round=\\bround (\\d+)\\b verdict=\\b(BLOCK|APPROVE)\\b engines=\\b(P:[A-Za-z0-9+:,-]+ I:[A-Za-z0-9+:,-]+ R:[A-Za-z0-9+:,-]+)"
  ],
  "key": {
    "mentions": 10673,
    "entities": 1913,
    "mentions_per_path": {
      "data.branch": 140,
      "data.comment_id": 391,
      "data.commit_sha": 830,
      "data.head_ref": 163,
      "data.head_sha": 227,
      "data.issue": 1295,
      "data.key": 1291,
      "data.merge_commit_sha": 163,
      "data.number": 3270,
      "data.parents.0": 877,
      "data.parents.1": 5,
      "data.pr": 168,
      "data.ref_number": 847,
      "data.seat_id": 29,
      "data.sha": 977
    },
    "entity_sizes": {
      "branch": {
        "1": 104,
        "2": 41,
        "3": 20,
        "4": 2,
        "6": 1,
        "43": 1
      },
      "comment": {
        "1": 391
      },
      "commit": {
        "1": 112,
        "2": 185,
        "3": 419,
        "4": 165,
        "5": 52,
        "6": 26,
        "7": 16,
        "8": 4,
        "9": 3,
        "10": 3,
        "12": 3,
        "13": 1,
        "14": 1
      },
      "item": {
        "1": 2,
        "2": 5,
        "3": 23,
        "4": 13,
        "5": 31,
        "6": 18,
        "7": 25,
        "8": 20,
        "9": 12,
        "10": 14,
        "11": 17,
        "12": 12,
        "13": 10,
        "14": 11,
        "15": 15,
        "16": 10,
        "17": 9,
        "18": 5,
        "19": 6,
        "20": 5,
        "21": 4,
        "22": 3,
        "23": 3,
        "24": 3,
        "25": 2,
        "26": 1,
        "27": 1,
        "28": 1,
        "29": 2,
        "30": 1,
        "31": 1,
        "32": 3,
        "33": 2,
        "35": 3,
        "37": 3,
        "41": 2,
        "42": 2,
        "43": 2,
        "44": 2,
        "45": 2,
        "46": 1,
        "47": 2,
        "48": 1,
        "49": 1,
        "50": 1,
        "51": 2,
        "52": 1,
        "55": 1,
        "56": 1,
        "57": 3,
        "59": 3,
        "61": 1,
        "63": 2,
        "71": 1,
        "72": 1,
        "73": 2,
        "74": 2,
        "77": 1,
        "78": 1,
        "80": 2,
        "82": 1,
        "84": 1,
        "86": 1,
        "101": 1,
        "104": 1,
        "123": 1,
        "128": 1,
        "136": 1,
        "188": 1,
        "215": 1
      },
      "seat": {
        "1": 10,
        "2": 4,
        "3": 2,
        "5": 1
      }
    },
    "abstained": {},
    "excluded": {}
  },
  "context_scored": {
    "mentions": 21168,
    "entities": 1916,
    "mentions_per_path": {
      "data.actor": 3270,
      "data.author": 877,
      "data.branch": 140,
      "data.comment_id": 391,
      "data.commit_sha": 830,
      "data.head_ref": 163,
      "data.head_sha": 227,
      "data.issue": 1295,
      "data.key": 1291,
      "data.merge_commit_sha": 163,
      "data.number": 3270,
      "data.parents.0": 877,
      "data.parents.1": 5,
      "data.pr": 168,
      "data.ref_number": 847,
      "data.ref_repo": 847,
      "data.repo": 5501,
      "data.seat_id": 29,
      "data.sha": 977
    },
    "entity_sizes": {
      "actor": {
        "4147": 1
      },
      "branch": {
        "1": 104,
        "2": 41,
        "3": 20,
        "4": 2,
        "6": 1,
        "43": 1
      },
      "comment": {
        "1": 391
      },
      "commit": {
        "1": 112,
        "2": 185,
        "3": 419,
        "4": 165,
        "5": 52,
        "6": 26,
        "7": 16,
        "8": 4,
        "9": 3,
        "10": 3,
        "12": 3,
        "13": 1,
        "14": 1
      },
      "item": {
        "1": 2,
        "2": 5,
        "3": 23,
        "4": 13,
        "5": 31,
        "6": 18,
        "7": 25,
        "8": 20,
        "9": 12,
        "10": 14,
        "11": 17,
        "12": 12,
        "13": 10,
        "14": 11,
        "15": 15,
        "16": 10,
        "17": 9,
        "18": 5,
        "19": 6,
        "20": 5,
        "21": 4,
        "22": 3,
        "23": 3,
        "24": 3,
        "25": 2,
        "26": 1,
        "27": 1,
        "28": 1,
        "29": 2,
        "30": 1,
        "31": 1,
        "32": 3,
        "33": 2,
        "35": 3,
        "37": 3,
        "41": 2,
        "42": 2,
        "43": 2,
        "44": 2,
        "45": 2,
        "46": 1,
        "47": 2,
        "48": 1,
        "49": 1,
        "50": 1,
        "51": 2,
        "52": 1,
        "55": 1,
        "56": 1,
        "57": 3,
        "59": 3,
        "61": 1,
        "63": 2,
        "71": 1,
        "72": 1,
        "73": 2,
        "74": 2,
        "77": 1,
        "78": 1,
        "80": 2,
        "82": 1,
        "84": 1,
        "86": 1,
        "101": 1,
        "104": 1,
        "123": 1,
        "128": 1,
        "136": 1,
        "188": 1,
        "215": 1
      },
      "repo": {
        "1195": 1,
        "5153": 1
      },
      "seat": {
        "1": 10,
        "2": 4,
        "3": 2,
        "5": 1
      }
    },
    "abstained": {},
    "excluded": {}
  },
  "failures": {
    "sidecar-aligned": 0,
    "sha-shape": 0,
    "sha-resolved": 0,
    "ref-repo": 0,
    "seat-issue": 0,
    "leg-key": 0
  },
  "failed": 0,
  "leg_pr_unopened": 2,
  "edges_observed": {
    "cross-references data.number -> data.ref_number": 847,
    "references data.number -> data.commit_sha": 830,
    "has-comment data.number -> data.comment_id": 391,
    "head-branch data.number -> data.head_ref": 163,
    "head-commit data.number -> data.head_sha": 164,
    "merged-as data.number -> data.merge_commit_sha": 163,
    "reviews-commit data.seat_id -> data.head_sha": 29,
    "reviews-branch data.seat_id -> data.branch": 29,
    "reviews-item data.seat_id -> data.issue": 4,
    "leg-branch data.key -> data.branch": 77,
    "leg-commit data.key -> data.sha": 100,
    "leg-pr data.key -> data.pr": 168,
    "parent data.sha -> data.parents.0": 877,
    "parent data.sha -> data.parents.1": 5,
    "names data.number -> data.refs": 0,
    "names data.sha -> data.refs": 0
  }
}
```

`leg_pr_unopened: 2` counts legs naming a PR whose `pulls` frame falls outside the span; it is
reported, not a failure (README "Verifier"). The `sprint` type has no mention path in the report
because the corpus has no `sprint.boundary` frame; see "Capture bug" below.

## Hand inspection

`key.ts --sample 60 --seed 372` (same inputs, `--out` outside the repository) drew 60 mentions,
4 from each of the 15 mention paths in the report. They come from 59 frames: two `review.seat`
mentions (`branch` and `head_sha`) share one frame. Each mention was checked against its source
system, read-only:

- `leg.status`: the `worker_history` row in `~/.dev-worker/dev-worker.db` (`mode=ro`): the row's
  `key` and `step` equal the frame's; `issue` and `key` are the key's number and repository;
  `pr` is the `PR #n` in `detail`, and `gh api repos/<repo>/pulls/<n>` is a merged PR whose head
  branch names the leg's issue; `branch` is the branch in `detail`; `sha` is the `@<hex>` in
  `detail`, resolved by `git rev-parse` in that repository's clone.
- `review.seat`: line `offset mod 100000` of `~/lifeos/logs/review-seats/<day>.jsonl`: the row's
  `cwd` gives the frame's repository; `seat_id` and `branch` are equal; the logged short
  `head_sha` is a prefix of the frame's and resolves to it in the clone.
- `issue.event`: `gh api --paginate repos/<repo>/issues/<n>/timeline`: the event found by its
  `event_id` (by `ordinal` for `cross-referenced`, which has no id) has the frame's `event`;
  `comment_id` is the event's id; `commit_sha` is the event's `commit_id` and exists in the clone;
  `ref_number` is the cross-reference source's number, and the source's repository is `ref_repo`.
- `pr.opened`, `pr.merged`: `gh api repos/<repo>/pulls/<n>` and `.../issues/<n>`: the sidecar's
  `item_id` is the issue id; `head_ref`, `head_sha` and `merge_commit_sha` are equal, and every
  sha is in the clone.
- `commit`: `git -C <clone> show -s --format='%H %P' <sha>`: the sha exists in the frame's
  repository, and `parents.i` is the i-th parent.

| path | sampled | agree | disagree |
|---|---|---|---|
| `data.branch` | 4 | 4 | 0 |
| `data.comment_id` | 4 | 4 | 0 |
| `data.commit_sha` | 4 | 4 | 0 |
| `data.head_ref` | 4 | 4 | 0 |
| `data.head_sha` | 4 | 4 | 0 |
| `data.issue` | 4 | 4 | 0 |
| `data.key` | 4 | 4 | 0 |
| `data.merge_commit_sha` | 4 | 4 | 0 |
| `data.number` | 4 | 4 | 0 |
| `data.parents.0` | 4 | 4 | 0 |
| `data.parents.1` | 4 | 4 | 0 |
| `data.pr` | 4 | 4 | 0 |
| `data.ref_number` | 4 | 4 | 0 |
| `data.seat_id` | 4 | 4 | 0 |
| `data.sha` | 4 | 4 | 0 |
| **total** | **60** | **60** | **0** |

**Disagreements:** none, so no rule is broken and no fix is needed. `private-key-v1.json` is not
written.

Two observations that are not disagreements:

- Two of the four `data.branch` samples are a lifeos seat on the trunk (`master`, no issue). The
  key reads them as one `branch` entity `(lifeos, master)`, which is the source's identity. That
  entity is the largest `branch` entity in the report (43 mentions). A system that keeps the
  trunk apart from feature branches is still scored correctly; one that merges seats by branch
  will merge every trunk review, which is what the key says.
- The sample cannot reach the `sprint` type (no mentions in the span) or the `pr.closed` kind
  (one frame in the span, no path drawn from it).

## Capture bug: no sprint frames in private-test (s2w#398)

`private-test` has 0 `sprint.boundary` frames although 24 sprint-table rows are dated
2026-09-29 or 2026-09-30. `sprintRows` in `private/events.ts` accepts only
`YYYY-MM-DD-<h>a|p` or `... (ran HH:MM)` in the slot cell; the table has written
`(slot HH:MM–HH:MM)` since sprint 63, and a row that fails the pattern is skipped without a drop
count. `private-dev` loses 15 of the rows dated on its days the same way. The key's `sprint`
rules are correct; the stream lacks the frames. Per the plan, a capture bug is a #371 follow-up
and the key stays: filed as s2w#398, fixed for #375's re-freeze. The pinned corpora are not
re-captured.

## `seat_id`: 242 mentions and 43 values in the private-dev profile

The private-dev profile (`h-min-v8.private-dev-8067.profile.md`) lists `data.seat_id` with 242
mentions and 43 distinct values, while plan §2 called `seat` a singleton type (one mention per
seat). Measured on both corpora:

| | `review.seat` frames | `seat_id` null | `seat_id` set (key mentions) | distinct ids (key entities) | ids on more than one frame |
|---|---|---|---|---|---|
| `private-dev` | 242 | 184 | 58 | 43 | 13 (sizes: 12 of 2, 1 of 4) |
| `private-test` | 63 | 34 | 29 | 17 | 7 (sizes: 4 of 2, 2 of 3, 1 of 5) |

Two causes:

1. **The profile counts the field, not the value.** Every `review.seat` frame carries the
   `seat_id` field, so the profile's 242 is the frame count. 184 of them are `null`: the caller
   did not pass `--seat-id` (the flag is optional, lifeos#1045). The key requires a scalar, so a
   null is no mention; the key has 58 mentions on private-dev, and the verifier's 29 on
   private-test matches the table.
2. **One seat logs several rows.** `seat_id` is minted by the caller once per review seat and
   passed again on each attempt to fill it. Within a repeated id the engine and model change in
   11 of 13 groups (private-dev) and 6 of 7 (private-test): the reviewer fallback ladder
   (a walled engine, then the next one) logs one row per attempt. The `head_sha` is the same in
   12 of 13 and 5 of 7 groups; the rest are a seat re-run on a newer head.

So `seat_id` does identify one seat, as plan §5 assumes, and the key's identity rule is right.
What was wrong is the description "singleton-only type": `seat` entities have up to 4 mentions
(private-dev) and 5 (private-test). `score` derives the singleton-only types from the key's
entities, not from the description, so no score changes. This PR corrects the README table row
and the comment in `private/key.ts`; the key files' bytes are unchanged (`key.ts --write` writes
the same files).
