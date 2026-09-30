# 0030: RFC 3339 date-times are a format, never an entity type

Date: 2026-09-29 · Status: accepted · Gate 2 · Issue #291 (PR 2, `PROFILER_VERSION` 7) · Builds on [0018](0018-no-compiled-domain-code.md), [0022](0022-discover-profiler.md)

## Decision

**A path whose every value is an RFC 3339 `date-time` names a moment, never a thing.** The
`s2w-discover` profiler gives it the role `Timestamp`: it keys no type and is no stage-5b
candidate, but it stays a candidate dependent, so it can still be another type's attribute (a
user's registration time stays on the user).

The shape test is `s2w_discover::stamp::shaped`: RFC 3339 §5.6 exactly,
`YYYY-MM-DDTHH:MM:SS[.fraction](Z|+HH:MM|-HH:MM)`, `T` and `Z` in either case, every field in
range and the day valid for its month and year. A space separator, a date alone, a missing
offset and MediaWiki's 14-digit `YYYYMMDDHHMMSS` are not date-times. A path is `Timestamp` when
it holds only strings, at least one, and every distinct string value has the shape; the role is
decided after `Sparse`, `Other`, `Constant` and `Flag` and before every uniqueness role
(`EventId` included).

**`cargo xtask check` 12 shifts date-times instead of hashing them.** Its renaming now moves every
shaped string by one constant (`SHIFT`, 100 years of 365.25 days plus 12,345 s) with
`s2w_discover::stamp::shift`, which keeps the fraction, the offset text and the letter case. A
date-time the shift cannot move (a leap second, or a year leaving 0000 to 9999) fails the check;
it is never hashed silently. Check 12 also compares every path's role between the two passes
and requires at least one `Timestamp` path in pass A. Check 11 is unchanged: the mapping engine
reads no value's shape, so it still hashes every string.

## Why this is allowed

- **0018 item 3 allows formats.** "Transports and formats (SSE, Kafka, HTTP, stdin NDJSON, JSON,
  Avro, Protobuf) are generic protocols." RFC 3339 is a protocol format, like JSON: it says
  nothing about what any stream means.
- **The evaluation contract keeps the format.** `docs/evaluation-contract.md` (line 349):
  "timestamps shift by one secret constant". The scored stream keeps every date-time's shape, so
  the rule behaves the same on plain and obfuscated streams. Hashing date-times in check 12 was
  stricter than the contract; shifting them makes check 12 test what the contract does.
- **0022's invariance contract becomes: rename every key, shift every date-time by one constant,
  hash every other string.** The crate shares one predicate between the profiler and check 12
  by design: the check must obfuscate exactly the values the profiler may read.

## Why the rule is needed

Measured on live `page_change` (21,528 events captured 2026-09-27 by `s2w watch`) at the
serve window (`DISCOVER_WINDOW` = 10^4), v6 mints a type keyed by a date-time: the editor's
first-edit time in events 0-10k and in a second 5,232-event run, and the first-edit time and the
revision time in events 10k-20k. A date-time passes the entity test (it recurs apart and a user's
fields follow it). It merges into the user's class only while it stays one-to-one with the user
id. Over a demo-sized window that stops: two users share a second (5 to 6 date-times per 10^4
events), and one user's date-time differs between wikis (1 to 3), so the one-to-one merge fails
and the date-time keeps its own type. The recorded page-change fixture (11.7k events) never shows
this; the live logs do.

## Limits, accepted

- **All cells or nothing.** A date-time path with one non-date-time string (an empty string, a
  placeholder) is not `Timestamp` and is profiled as before.
- **Other date formats are out.** MediaWiki's 14-digit `img_timestamp` on `recentchange` stays an
  entity type. A second format is a new decision.
- **Free text stays out** (#291 ruling item 3): the contract hashes it, so a text-shape rule would
  behave differently on the scored stream.

## Measured

See 0022's amendment `(s2w#291 PR 2, PROFILER_VERSION 7)` and research 0009's #291 addendum for
the held-out `reserved-4` scores and the page-change memory measurement.
