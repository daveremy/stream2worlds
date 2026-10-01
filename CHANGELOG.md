# Changelog

How Stream2Worlds got built, one entry per sprint, newest first. Git history records every commit; this file records the arc: what became possible, what we learned, and where the plan changed direction.

Each entry has the same four parts:

- **Shipped:** what now works or is decided, and why it matters.
- **Learned:** what a measurement, review or research note taught us.
- **Changed course:** decisions that overturned an earlier plan, with the reason.
- **Next:** what the following sprint picks up.

A sprint without a merge still gets an entry. What it learned is often the most useful part.

---

## Public APIs are pinned, four oversized modules split, and the lint-exception count falls to 53 — [#362](https://github.com/daveremy/stream2worlds/pull/362), [#367](https://github.com/daveremy/stream2worlds/pull/367), [#365](https://github.com/daveremy/stream2worlds/pull/365), [#363](https://github.com/daveremy/stream2worlds/pull/363), [#366](https://github.com/daveremy/stream2worlds/pull/366), [#364](https://github.com/daveremy/stream2worlds/pull/364), [#68](https://github.com/daveremy/stream2worlds/issues/68), [#66](https://github.com/daveremy/stream2worlds/issues/66) (2026-09-30)

**Shipped:** a library crate's public API can no longer change by accident. Check 19 ([#362](https://github.com/daveremy/stream2worlds/pull/362), closing [#68](https://github.com/daveremy/stream2worlds/issues/68)) reads each library crate's `pub` items with `syn`, which runs on stable Rust, and compares them with a committed snapshot in `xtask/public-api/`. `cargo xtask api --update` rewrites a snapshot on purpose, so the change shows up as a diff in review. Four oversized modules were then split along their seams, in three PRs, the first PRs of [#66](https://github.com/daveremy/stream2worlds/issues/66): the `xtask` root drops from 439 to 299 non-test lines, with the allowlist, stack-table and lint code in `deps.rs` and `lints.rs`, and the module-size walker now refuses only `include!`, not `include_str!` or `include_bytes!` ([#367](https://github.com/daveremy/stream2worlds/pull/367)); `query::view` goes from 897 non-test lines to `view.rs` plus four submodules and a test module, the largest 308 non-test lines ([#365](https://github.com/daveremy/stream2worlds/pull/365)); and `s2w_log`'s `lib.rs` and `verdicts.rs` split into modules ([#363](https://github.com/daveremy/stream2worlds/pull/363)). The rustdoc item pages are the same before and after both library splits, apart from a whitespace rewrap of two doc comments in `query::view`'s. Batch 2 of the `#[expect]` burn-down ([#366](https://github.com/daveremy/stream2worlds/pull/366)) takes the count from 58 to 53 with a `BridgeParts` struct, `judge_event` split into `check_rows`, `judge_engines` and `tally`, four helpers pulled out of `pump`, and the two large `http.rs` scenario tests split into four. Check 15 also lost its unreachable report-only branch ([#364](https://github.com/daveremy/stream2worlds/pull/364), [#357](https://github.com/daveremy/stream2worlds/issues/357)); a cycle still fails it.

**Learned:** check 19 proved itself the same day. Both library split PRs ([#363](https://github.com/daveremy/stream2worlds/pull/363), [#365](https://github.com/daveremy/stream2worlds/pull/365)) had to commit a snapshot diff, and each showed only module paths moving, with nothing removed. Main also moved under the last split PR: two rebases and three reads of the change before it merged, so finishing the splits took far longer than the diffs suggest. Four timing-sensitive tests (three in `serve::tests`, one `s2w` integration test) flake under heavy CI load ([#369](https://github.com/daveremy/stream2worlds/issues/369)).

**Changed course:** none. The `include!` change amends [decision 0001](docs/decisions/0001-workspace-layers.md) with a dated note.

**Next:** [#66](https://github.com/daveremy/stream2worlds/issues/66) PR 4 records a ceiling for each of the 14 remaining oversized modules and turns the module-size check on, so none can grow unnoticed. `fold_with_delta` and `graph_view`, which were waiting on the view split, are the next `#[expect]` slice ([#156](https://github.com/daveremy/stream2worlds/issues/156)).

## Module cycles break and the lint-exception count can only fall — [#358](https://github.com/daveremy/stream2worlds/pull/358), [#359](https://github.com/daveremy/stream2worlds/pull/359), [#356](https://github.com/daveremy/stream2worlds/pull/356), [#360](https://github.com/daveremy/stream2worlds/pull/360), [#240](https://github.com/daveremy/stream2worlds/issues/240), [#156](https://github.com/daveremy/stream2worlds/issues/156), [#267](https://github.com/daveremy/stream2worlds/issues/267) (2026-09-30)

**Shipped:** `cargo xtask check` now fails on two things it used to let through. Check 15 is enforced: no module may depend back on a module that depends on it ([#358](https://github.com/daveremy/stream2worlds/pull/358) and [#359](https://github.com/daveremy/stream2worlds/pull/359), closing [#240](https://github.com/daveremy/stream2worlds/issues/240)). To get there, `SourceStats` moved into `query::sources`, `generation` became a child of `query::http`, and `/sentences` reads mappings through `query::read_mappings` instead of calling up into `routes`, with no traits added and no re-exports left behind. That untangled two cycles, one through `bridge`, `query::generation` and `query::http` and one through `bridge`, `query` and `routes`, so the dependency direction in the server is now a rule. Check 18 is new ([#356](https://github.com/daveremy/stream2worlds/pull/356)): it counts every `#[expect(<lint>)]` per lint against `xtask/expect-baseline.toml`. A count above the baseline fails and lists every site; a count below it fails until `--tighten-baseline` lowers the file; raising it needs a `Baseline-growth: s2w#N` commit trailer. Batch 1 of the burn-down then removed eight exceptions ([#360](https://github.com/daveremy/stream2worlds/pull/360), [#156](https://github.com/daveremy/stream2worlds/issues/156)): `serve_live`, `supervise`, `pump`, `flush_and_report` and `catch_up` take small parameter structs, and four helpers were extracted, taking the count from 66 to 58. Both checks were shown to fire by adding a cycle and an extra exception on purpose.

**Learned:** the backfill memory row that looked over 1 GiB passes. Re-measured on main it reads 1,116 to 1,125 MiB on glibc and 1,105 to 1,112 MiB on mimalloc, both under the 1,340 MiB limit [#326](https://github.com/daveremy/stream2worlds/pull/326) had already raised, so [#267](https://github.com/daveremy/stream2worlds/issues/267) closed with no code. The nightly job asserts the 5 s row and the 1 s row runs by hand; asserting the limits under mimalloc, the shipped allocator, is [#352](https://github.com/daveremy/stream2worlds/issues/352). On the demo box ([#283](https://github.com/daveremy/stream2worlds/issues/283)), mimalloc held 1,222 MiB at 30 minutes of uptime against 1,386 MiB on glibc, one sample each with load not held equal.

**Changed course:** none. [#66](https://github.com/daveremy/stream2worlds/issues/66)'s planning pass found 18 modules over the 400-line cap, not 2, so the work became four PRs rather than one.

**Next:** public API snapshots ([#68](https://github.com/daveremy/stream2worlds/issues/68)), the module splits of [#66](https://github.com/daveremy/stream2worlds/issues/66) and the second `#[expect]` batch.

## The demo reads as Wikipedia, scoring keys get a prefix form, and the binary moves to mimalloc — [#350](https://github.com/daveremy/stream2worlds/pull/350), [#348](https://github.com/daveremy/stream2worlds/pull/348), [#354](https://github.com/daveremy/stream2worlds/pull/354), [#353](https://github.com/daveremy/stream2worlds/pull/353), [#289](https://github.com/daveremy/stream2worlds/issues/289), [#224](https://github.com/daveremy/stream2worlds/issues/224), [#283](https://github.com/daveremy/stream2worlds/issues/283) (2026-09-30)

**Shipped:** the viewer now shows what the manifest knows ([#350](https://github.com/daveremy/stream2worlds/pull/350), deployed to the demo box at 14:04). `/sentences` entities carry a `label`; the graph and legend show each type's name, noun and kind icon; a **Live changes** feed lists the newest events as sentences; and **Active now** reads `<icon> <label> (<n> changes)`, for example `👤 1isall (22 changes)` in the README's new screenshot ([`docs/assets/demo-live.jpg`](docs/assets/demo-live.jpg), taken from the live demo after the deploy). Separately, `h-measure` key format 2 adds a prefix form for `unscored` paths ([#348](https://github.com/daveremy/stream2worlds/pull/348), closing [#224](https://github.com/daveremy/stream2worlds/issues/224)): `{"prefix": ["data", "log_params"]}` leaves that path and every path under it unscored, in any corpus, where the v1 keys list 84 exact `log_params` paths from the dev corpus. New `dev-key-v2*.json` files use it; the v0 and v1 key files are unchanged, and a v2 key grades only mappings frozen after it. Review follow-ups landed in [#354](https://github.com/daveremy/stream2worlds/pull/354) ([#349](https://github.com/daveremy/stream2worlds/issues/349)). Last, the `s2w` binary now allocates through mimalloc ([#353](https://github.com/daveremy/stream2worlds/pull/353), [decision 0031](docs/decisions/0031-mimalloc-global-allocator.md), [#283](https://github.com/daveremy/stream2worlds/issues/283)), deployed to the demo box at 14:35: on the recorded load at history cap 50,000 the backfill's resident peak drops from 774 to 689 MiB on the main thread and from 1,122 to 1,053 MiB with a viewer connected. SQLite's C heap stays on glibc malloc.

**Learned:** the screenshot shows the gap honestly. Types that have no manifest row still show raw key text in the graph and legend (for example `editor/first_edit_dt+performer/first_edit_dt · keys`), because the viewer only names what the manifest names ([#351](https://github.com/daveremy/stream2worlds/issues/351)).

**Changed course:** none. The "changes, not edits" wording the feed shipped with was ruled in Sprint 93 (below).

**Next:** name the unnamed types in the graph and legend ([#351](https://github.com/daveremy/stream2worlds/issues/351)), then the manifest noun ([#347](https://github.com/daveremy/stream2worlds/issues/347)). The live-demo label share ([#342](https://github.com/daveremy/stream2worlds/issues/342)) is still open.

## The demo's display work gets a first leg, with no merge — [#289](https://github.com/daveremy/stream2worlds/issues/289) (2026-09-30)

**Shipped:** no merge. Leg A of [#289](https://github.com/daveremy/stream2worlds/issues/289) cleaned the server-side sentence text (`display_text` strips `/* ... */` section markers, spaces underscored titles and drops a trailing separator) so sentences stop carrying raw wiki markup. The viewer work (names, kind icons, the sentence feed) was still owed and merged in the next sprint ([#350](https://github.com/daveremy/stream2worlds/pull/350)).

**Learned:** plan review blocked twice on the viewer design, and both points were folded into the plan: one renderer per mode, so live rows cannot repaint the old table over the sentence list, and an explicit `<tbody>`.

**Changed course:** the brief promised "Live edits" wording, but manifest v0 has an entity noun and no event noun, and hard-coding "edits" would break decision 0018 (no compiled domain vocabulary). The page says "Live changes"; the manifest noun is [#347](https://github.com/daveremy/stream2worlds/issues/347).

**Next:** the viewer work, then the README screenshot from the live demo.

## A System 2 manifest names the demo's world, and the fallback label needs a floor — [#344](https://github.com/daveremy/stream2worlds/pull/344), [#288](https://github.com/daveremy/stream2worlds/issues/288) (2026-09-30)

**Shipped:** the demo box now serves a manifest written by System 2 (proposal
`f2dcaa8c5ffe5c07`, `claude-sonnet-5-5/cli`, through the headless model CLI), stored under the
accept policy. A spot check of `/sentences?last=20` scored 20 of 20 readable and 20 of 20 faithful
to the raw events, against a target of 18 of 20. One live row reads "Llewee edited
Saint_David's_Day: /* School celebrations */ added link". The fallback proposer is now
`dashboard-fallback/3`: an attribute can label a type only when it is present on about as many
events as the rule's key and has at least 8 distinct values ([#344](https://github.com/daveremy/stream2worlds/pull/344)).

**Learned:** the dry run on the box found the case the half-share rule missed. The redirect types'
key appears on 40 of 2,000 tail events (16 distinct values), while `content_model` appears on all
2,000 (4 distinct), so the rule compared two different populations and labelled links by a
category word. A coverage test plus a distinct-value floor fixes it, with a test for each half that
fails on version 2. The fallback still writes "type + key" sentences by design, so readable
sentences need a System 2 manifest; the deterministic path alone scored 0 of 20.

**Next:** the live-demo label share, which needs a bounded world read in the measurement script
([#342](https://github.com/daveremy/stream2worlds/issues/342)), then the display work: names, kind icons and "Live edits" sentences
in the viewer ([#289](https://github.com/daveremy/stream2worlds/issues/289)).

## Cold restarts pass on the demo box, and the world starts naming itself — [#343](https://github.com/daveremy/stream2worlds/pull/343), [#340](https://github.com/daveremy/stream2worlds/pull/340), [#331](https://github.com/daveremy/stream2worlds/issues/331), [#292](https://github.com/daveremy/stream2worlds/issues/292) (2026-09-30)

**Shipped:** all 10 cold restarts on the demo box pass both gates (first paint 1 s, graph 3 s),
so [#331](https://github.com/daveremy/stream2worlds/issues/331) and [#292](https://github.com/daveremy/stream2worlds/issues/292) are closed with no new code. Worst case over 4 viewers: first paint 875 ms and
graph 1,819 ms with the file cache cleared, and 776 ms and 1,584 ms on a real deploy checked at
23 s. The manifest's labels, nouns and sentences ([#343](https://github.com/daveremy/stream2worlds/pull/343)) went live with `/sentences`; with the
manifest label, 25 of 25 primary Active-now rows had a human name, against 13 of 60 for the old
viewer. The Sprint 90 entry and README refresh landed as [#340](https://github.com/daveremy/stream2worlds/pull/340).

**Learned:** Sprint 90's 36 ms miss was gone once we measured it properly. CPU sampled once a
second from the service's start shows replay ends about 11 s after start, not the ~30 s assumed, so
by 22 s the catch-up budget no longer matters. First paint is mostly network: about 450 ms of the
~800 ms is round trips to the box. The real remaining worst case is replay length: at about
100k events since the last snapshot (roughly 8 h), the graph gate fails until replay finishes near
26 s ([#341](https://github.com/daveremy/stream2worlds/issues/341)). The 20 sampled sentences matched their raw events 20 of 20 but read well for a
person 0 of 20, because the fallback writes "type + key" by design, so readable sentences need a System 2 manifest.

**Changed course:** the plan was to tune the replay batch budget on the box. The measurement made
that unnecessary, and the work moved to the labels.

**Next:** re-propose the demo world's manifest with System 2 and check the sentences by eye
([#288](https://github.com/daveremy/stream2worlds/issues/288)).

## Events read as sentences, and the fallback stops labelling editors "edit" — [#302](https://github.com/daveremy/stream2worlds/issues/302) (2026-09-30)

**Shipped:** the dashboard manifest names each type's noun, label and kind, and gives each event
type a sentence ([decision 0029](docs/decisions/0029-dashboard-manifest-v0.md)). One renderer in
`s2w-model` fills the sentence from the payload; `GET /worlds/{world}/sentences?last=N` and the
MCP `sentences` tool serve the last N events that way, byte-equal, each with the entities it
names. The System 2 prompt asks for the new fields, and the deterministic proposer's version 2
never labels a type by a date-time or by a category.

**Learned:** measuring labels on the demo's own mapping found the category case. An editor's
only attributes were its edit kind and content model, so the most distinct one, three values,
became every editor's label, and the viewer would have named every editor "edit" or
"wikitext". A string test for "human name" passes those words, so the measurement script
reports the labels it scored, not only the share.

**Next:** re-proposal of the demo world's manifest once the build is deployed, and the viewer
reading `label` and `sentences` in place of its own guesses (s2w#288).

## The demo check fails loudly, and a restart stalls the page far less — [#338](https://github.com/daveremy/stream2worlds/pull/338), [#336](https://github.com/daveremy/stream2worlds/pull/336), [#334](https://github.com/daveremy/stream2worlds/pull/334), [#332](https://github.com/daveremy/stream2worlds/pull/332) (2026-09-30)

**Shipped:** the demo check now asserts its first-paint and graph gates on every run, with
`--no-gates` to opt out, so a red demo run means something. The acceptance table behind that
change passed 11 of 12 runs across 1 and 4 viewers, quiet and mid-backfill; the one failure came
60 s after a restart ([#292](https://github.com/daveremy/stream2worlds/issues/292)). Measuring that failure found two causes. The 410 was our own
check script asking for the evidence tail the old way (`from=`); it now asks `last=500`, as the
viewer does. And `serve` ran HTTP and replay on one thread, so for the first seconds after a
restart every request took 88 to 207 ms, even static `/main.js`. Each catch-up batch now has a
20 ms budget, so HTTP gets a turn between batches. On the hub, over the first 30 s of a full
replay, mean `/main.js` latency fell from 144 ms to 29 ms for a 14% cost in replay throughput
([#331](https://github.com/daveremy/stream2worlds/issues/331), [#338](https://github.com/daveremy/stream2worlds/pull/338)). The profiler stopped minting byte sizes as entity types: a churn
floor on integer-valued keys (`PROFILER_VERSION` 8) takes `mediainfo/content_size` out, going from
13 to 12 types and dropping 0.40% of world events with no recall loss on the reserved span
([#327](https://github.com/daveremy/stream2worlds/issues/327), [#336](https://github.com/daveremy/stream2worlds/pull/336)). `cargo xtask check` now fails when the README's Scale row
disagrees with `scale-baseline.toml`
([#324](https://github.com/daveremy/stream2worlds/issues/324), [#334](https://github.com/daveremy/stream2worlds/pull/334)). The Sprint 89 entry below landed as [#332](https://github.com/daveremy/stream2worlds/pull/332).

**Learned:** all five predictions posted before the #327 run held. Leg A showed that no rule based
on value equality can also remove `revision/comment` without losing the real `title` type (`page`
recall 0.50 to 0.25), and that the profiler's never-read-text invariant rules out the "free text
by length" guard the issue proposed. `revision/comment` stays a recorded false class, and whether
the profiler may read text is now [#333](https://github.com/daveremy/stream2worlds/issues/333). On the demo box, the first run after a restart
still misses first paint by 36 ms (1,036 ms against the 1,000 ms gate); by about 70 s both gates
pass, and the 410 is gone. The gates-on default did its job in that run. Local gates also passed
in Sprint 89 while CI failed on rustdoc; the S2W gates line now carries
`RUSTDOCFLAGS="-D warnings" cargo doc`, whose absence let that step pass on anything.

**Changed course:** [#331](https://github.com/daveremy/stream2worlds/issues/331) was filed mid-sprint from #292's table and both PRs merged, but
it stays open because the box still misses the first run by 36 ms, and #292 stays open until a cold
load passes too. Dave also set the direction for making the demo compelling, in three layers:
legible ([#288](https://github.com/daveremy/stream2worlds/issues/288) to [#290](https://github.com/daveremy/stream2worlds/issues/290)), then alive (surging now, born today, a 24 h
time-lapse), then meaningful (diff text plus cited System 2 summaries, which needs a new source).
System 2 runs on a headless model CLI for now, with two model tiers and pluggable providers for
open source ([#337](https://github.com/daveremy/stream2worlds/issues/337), [#335](https://github.com/daveremy/stream2worlds/issues/335)).

**Next:** measure the catch-up batch budget on the demo box's 1.5 CPU rather than the hub's and
close [#331](https://github.com/daveremy/stream2worlds/issues/331) and [#292](https://github.com/daveremy/stream2worlds/issues/292); then the legible layer of the Living Wikipedia design
([#337](https://github.com/daveremy/stream2worlds/issues/337)), and the text-reading question in [#333](https://github.com/daveremy/stream2worlds/issues/333).

## The full type view builds in 274 ms, and `/world` stops making the fold wait — [#329](https://github.com/daveremy/stream2worlds/pull/329), [#328](https://github.com/daveremy/stream2worlds/pull/328), [#326](https://github.com/daveremy/stream2worlds/pull/326), [#323](https://github.com/daveremy/stream2worlds/pull/323) (2026-09-30)

**Shipped:** the demo box passes every gate for the first time: first paint 960 ms (gate 1 s) and
graph 1,813 ms (gate 3 s), the maximum over 4 viewers, at 1b6a4a9. The full type view, the one the
graph gate times, now builds in 274 ms instead of 1,488 ms (median of 5 reads at 213k entities and
2.09M links on the recorded backfill), with a byte-identical 29,807 B body. It reuses the type
summary's nodes, numbers each type and hub, groups links in one pass with no string per link, and
builds strings only for the roughly 182 output links; new tests compare the old and new paths at
every golden-log offset
([#325](https://github.com/daveremy/stream2worlds/issues/325), [#329](https://github.com/daveremy/stream2worlds/pull/329)).
`/world` no longer holds the timeline's read lock while it sorts and writes. A new owned
projection copies only what the body needs under the lock, using shared entity state, then
releases it. Measured main against the branch on the hub, the entity view's lock hold went from
2,899 ms (all locked) to 1,694 ms locked plus 531 ms of prepare and 1,478 ms of write after it. With
4 viewers the backfill finished in 109 s instead of 783 s, the slowest `poll_once` fell from
4,202 ms to 958 ms, and peak memory from 1,635 to 1,353 MiB
([#272](https://github.com/daveremy/stream2worlds/issues/272), [#328](https://github.com/daveremy/stream2worlds/pull/328)).
The world's memory budget is re-measured after #291: 14.32M world events, a 598.9 MiB head world,
and a bridge peak of 780.7 MiB with no viewer and 1,280 MiB with one. Decision 0026's limits move
to 810 and 1,340 MiB with a dated amendment. A nightly workflow runs the full `backfill_memory`
sweep at 03:17 MST on the hub runner and files an issue on failure, and a new standing test,
`discovered_types`, fails whenever the set of discovered entity types changes
([#282](https://github.com/daveremy/stream2worlds/issues/282), [#326](https://github.com/daveremy/stream2worlds/pull/326)).
The Sprint 88 entry below landed as [#323](https://github.com/daveremy/stream2worlds/pull/323).

**Learned:** the plan for #272 assumed moving the write out of the lock would move the graph gate.
Leg A's measurement showed the view was 1,524 ms of build against 0.1 ms of write, so it could
not; the gate moved because #325 was filed mid-sprint to cheapen the build. Profiling that build
found 69% of it in a `format!` per link, and a first replacement using a HashMap measured 480 ms
because hashing each link's kind string was the cost; a short per-pair kind list gave 274 ms. The
first run of `discovered_types` found two attribute-shaped types still minted as entities:
`revision/comment` at 5.2% of world events and `mediainfo/content_size` at 0.4%
([#327](https://github.com/daveremy/stream2worlds/issues/327)). Local gates passed while CI failed
on `cargo doc -D warnings` in #329.

**Changed course:** [#272](https://github.com/daveremy/stream2worlds/issues/272) was filed at 3 points
and took 3 legs and an 847/301-line diff, because the owned projection is a new module with three
new concurrency tests; by the sizing rubric it was a 5. Two of
[#235](https://github.com/daveremy/stream2worlds/issues/235)'s targets were missed as measured: the
lock hold is 0.58 to 0.62 of main (target at most one half), and 4 viewers peak above 1 GiB, so that
issue stays open. With both Codex accounts and agy walled all sprint, Opus implemented every leg
and Fable and grok held the review seats.

**Next:** [#292](https://github.com/daveremy/stream2worlds/issues/292) owes its full acceptance table
(1 and 4 viewers, 3 runs each, quiet and mid-backfill) before it closes, and first paint passes by
only 40 ms, so watch it. The demo box has no build-time figure for #325 yet, only the 274 ms on the recorded backfill. Then
[#327](https://github.com/daveremy/stream2worlds/issues/327), the two attribute-shaped types, and a
mechanical README-versus-baseline check ([#324](https://github.com/daveremy/stream2worlds/issues/324)).
The demo box runs merged main through [#329](https://github.com/daveremy/stream2worlds/pull/329).

## The world answers a cheap summary first, and System 2 can propose a dashboard — [#317](https://github.com/daveremy/stream2worlds/pull/317), [#318](https://github.com/daveremy/stream2worlds/pull/318), [#319](https://github.com/daveremy/stream2worlds/pull/319), [#320](https://github.com/daveremy/stream2worlds/pull/320), [#321](https://github.com/daveremy/stream2worlds/pull/321), [#322](https://github.com/daveremy/stream2worlds/pull/322), [#316](https://github.com/daveremy/stream2worlds/pull/316) (2026-09-30)

**Shipped:** a large world can now say how big it is before it says anything expensive.
`/world?lod=type&links=none` and MCP `world_view { links: "none" }` return the type summary: the
type nodes with their counts and the hub nodes, with no relationship pass. At 360,830 entities it
takes about 150 ms, against about 2.5 s for the full type view, and a repeat request is served from
memory. The ETag gains a `-nolinks` suffix and every existing tag stays byte-identical
([#296](https://github.com/daveremy/stream2worlds/issues/296), [#318](https://github.com/daveremy/stream2worlds/pull/318)).
The viewer asks for that summary first and decides from it whether the world is too big to draw.
A small world goes from the summary to the entity view and never builds the full type view; a large
one draws the summary, then the full type
view adds the links in place with node positions kept
([#303](https://github.com/daveremy/stream2worlds/issues/303), [#319](https://github.com/daveremy/stream2worlds/pull/319)).
Full `lod=type` requests now join the single-flight `/world` queue, so four viewers share one
build; the summary is served alone and never waits
([#297](https://github.com/daveremy/stream2worlds/issues/297), [#322](https://github.com/daveremy/stream2worlds/pull/322)).
System 2 gained its `Provider` seam: `ExecProvider` runs any model CLI with no shell, the prompt on
stdin, a cleared environment, an empty working directory, a 180 s timeout and byte caps;
`ReplayProvider` answers from recorded replies; `System2Proposer` makes at most two calls per
attempt, one of them a repair. Its prompts are committed files, and check 9 scans them for
denylisted terms
([#311](https://github.com/daveremy/stream2worlds/issues/311), [#317](https://github.com/daveremy/stream2worlds/pull/317)).
`s2w dashboard propose --system2-model M/V --system2-cmd <program>` puts that proposer behind the
manifest filer from Sprint 87, with the operator recipe in
[decision 0029](docs/decisions/0029-dashboard-manifest-v0.md#the-system-2-proposer-311-2026-09-30)
([#320](https://github.com/daveremy/stream2worlds/pull/320)). The flaky `bridge_replay` test now waits
for the bridge to commit through the log's last position instead of stopping at the claim that
moves the head, and passed 50 of 50 runs in a loop
([#258](https://github.com/daveremy/stream2worlds/issues/258), [#321](https://github.com/daveremy/stream2worlds/pull/321)).
The Sprint 87 entry below landed as [#316](https://github.com/daveremy/stream2worlds/pull/316); its
scrub added a dated amendment to decision 0006 for single-flight `/world` and its 503
`world_queue_full` code, and the single-flight invariant to `crates/s2w-app/AGENTS.md`.

**Learned:** the summary's first build took 143 to 159 ms at 360k entities, over the 100 ms line
the brief set, so it is memoized by (epoch, hub cap, offset) and a repeat costs about 0.01 ms. On
the demo box, RTT-adjusted first paint held at 825 ms for the slowest of 4 viewers against its 1 s
gate. The graph fell through the sprint's three deploys, from 7.66 s to 6.72 s to 4.79 s, and still
fails its 3 s gate ([#292](https://github.com/daveremy/stream2worlds/issues/292)). The viewer's status
line clears only when the full type view lands, so the graph gate still measures that view, and
the full view's server build alone is about 2.5 s at this size. Meeting 3 s needs a cheaper full
view, or an RTT allowance on the gate, which is an open question in [#292](https://github.com/daveremy/stream2worlds/issues/292). Plan review of [#311](https://github.com/daveremy/stream2worlds/issues/311)
found that a raw U+2028 in stream data could forge the prompt's end-of-data marker; every untrusted
value now goes through one single-line encoder that escapes it.

**Changed course:** [#311](https://github.com/daveremy/stream2worlds/issues/311) was filed at 3 points
and took three legs and two PRs, because it defined a new trait with a sandboxed exec path; by the
sizing rubric it was a 5. It was split into the provider seam and the CLI wiring rather than one
large PR. With both Codex accounts and agy walled all sprint, Opus implemented every leg and
Fable and grok held the two review seats.

**Next:** the owned `/world` projection, which sorts and serializes after the world lock is released
([#272](https://github.com/daveremy/stream2worlds/issues/272)), as the next attempt at a cheaper full
type view. The graph gate waits on that; [#292](https://github.com/daveremy/stream2worlds/issues/292) stays open until it passes.
The first real System 2 run on the demo world is one command, waiting on the choice of provider and
model class ([#288](https://github.com/daveremy/stream2worlds/issues/288)). The demo box runs merged
main through [#322](https://github.com/daveremy/stream2worlds/pull/322).

## A world proposes its own dashboard, and the page paints in under a second — [#312](https://github.com/daveremy/stream2worlds/pull/312), [#313](https://github.com/daveremy/stream2worlds/pull/313), [#314](https://github.com/daveremy/stream2worlds/pull/314), [#315](https://github.com/daveremy/stream2worlds/pull/315), [#310](https://github.com/daveremy/stream2worlds/pull/310) (2026-09-30)

**Shipped:** a world can file its own dashboard manifest. `s2w dashboard propose` profiles the
newest 2,000 events of each mapped source and stores a proposal under the `dashboard-auto-apply/1`
policy: a manifest that validates is accepted, an empty or invalid one is rejected with its error.
The first proposer is deterministic (`dashboard-fallback/1`), so the command never waits on a
model; it produces a `feed` projection with one default role. Re-running on the same log writes
nothing, and failed attempts stop at 3 per input ([decision 0029](docs/decisions/0029-dashboard-manifest-v0.md),
[#301](https://github.com/daveremy/stream2worlds/issues/301), [#312](https://github.com/daveremy/stream2worlds/pull/312)).
A date-time is now a format the profiler knows: a path whose every value is an RFC 3339 date-time
gets a `Timestamp` role, may be an attribute, and never keys an entity type; check 12 shifts
timestamps by a constant instead of hashing them (`PROFILER_VERSION` 7,
[decision 0030](docs/decisions/0030-timestamps-are-a-format.md), [#291](https://github.com/daveremy/stream2worlds/issues/291) PR 2, [#314](https://github.com/daveremy/stream2worlds/pull/314)).
`s2w proposals propose` files a human-authored stream mapping, and `demos/local-routed-world/run.sh --keep`
serves a routed Wikipedia world locally, so the viewer's paint gates run against it in seconds
([#309](https://github.com/daveremy/stream2worlds/issues/309), [#313](https://github.com/daveremy/stream2worlds/pull/313)). At most one full `/world` projection is built at a time;
requests with the same key share one serialized body and a full queue answers 503
`world_queue_full` ([#270](https://github.com/daveremy/stream2worlds/issues/270), [#315](https://github.com/daveremy/stream2worlds/pull/315)). The Sprint 86 entry below landed as [#310](https://github.com/daveremy/stream2worlds/pull/310).

**Learned:** on live Wikipedia windows the demo had minted `first_edit_dt` and `rev_dt` as entity
types, because two users registering in the same second, or one user id on two wikis, made
timestamps stop being 1:1 with users. The v7 change went from one or two timestamp types to none, and
all four pre-registered predictions hit on the new `reserved-4` span: the same score as v6, `user`
recall 1.0, `img_timestamp` still a type (an accepted limit), and the [#282](https://github.com/daveremy/stream2worlds/issues/282) fixture at 599 MiB against 597.
On the demo box, RTT-adjusted first paint fell from 3.3 s for one viewer (5.1 to 5.9 s for four) at the last sprint's wrap to 1.5 s after
the first deploy and to 825 ms (max of 4 viewers) at this one: the 1 s gate passes for the first
time. The graph still takes 6.5 s against its 3 s gate, so the demo check fails on that alone
([#292](https://github.com/daveremy/stream2worlds/issues/292)). The ASan panic that stalled [#270](https://github.com/daveremy/stream2worlds/issues/270) did not recur in about 2,900 builds and ASan found
nothing; the evidence points to a single flipped bit (0x2006 is 6 plus one bit), rather than a code bug.
Review of [#310](https://github.com/daveremy/stream2worlds/pull/310) caught two errors in the S86 entry (the parse gate is `xtask scale`, and
there were nine child issues, not eleven).

**Changed course:** [#301](https://github.com/daveremy/stream2worlds/issues/301) was filed at 5 points and took three legs for its first PR alone, because one
issue spanned four crates, the service, the CLI and docs. Its System 2 proposer was split out as
[#311](https://github.com/daveremy/stream2worlds/issues/311). The sprint also slowed itself: with both Codex accounts and agy walled and the
weekly limit climbing, two freed slots stayed empty rather than burn on Claude alone.

**Next:** the type summary endpoint and its client, so the graph paints from a summary first
([#296](https://github.com/daveremy/stream2worlds/issues/296), [#303](https://github.com/daveremy/stream2worlds/issues/303)), then the System 2 dashboard proposer on the same trait
([#311](https://github.com/daveremy/stream2worlds/issues/311)). Merged main is one PR ahead of the demo box: [#315](https://github.com/daveremy/stream2worlds/pull/315) is not deployed yet.

## The world sheds a third of its memory, and the page stops waiting for it — [#299](https://github.com/daveremy/stream2worlds/pull/299), [#304](https://github.com/daveremy/stream2worlds/pull/304), [#305](https://github.com/daveremy/stream2worlds/pull/305), [#306](https://github.com/daveremy/stream2worlds/pull/306), [#307](https://github.com/daveremy/stream2worlds/pull/307) (2026-09-29)

**Shipped:** the profiler no longer mints per-edit counters as entity types. A churn rule drops
a key whose values move on and never come back under any key that follows it, and a type that only
the second entity test admits becomes a leaf with one relationship (`PROFILER_VERSION` 6,
[decision 0022](docs/decisions/0022-discover-profiler.md), [#291](https://github.com/daveremy/stream2worlds/issues/291) PR 1, [#306](https://github.com/daveremy/stream2worlds/pull/306)). On [#282](https://github.com/daveremy/stream2worlds/issues/282)'s
fixture the head world falls from 887.1 to 596.8 MiB and world events from 20.0M to 14.3M, with
`user` recall unchanged at 1.0. The demo page now asks for its first rows at load: `/events?last=N`
serves the latest N events up to the server's head, every `/events` response carries `S2W-Epoch`
and `S2W-Head`, and the viewer paints the evidence table and Active now from the first parsed
chunk while `/world` is still loading ([#304](https://github.com/daveremy/stream2worlds/pull/304), [#305](https://github.com/daveremy/stream2worlds/pull/305), [#294](https://github.com/daveremy/stream2worlds/issues/294), [#295](https://github.com/daveremy/stream2worlds/issues/295)). A world can now
have a dashboard manifest: [decision 0029](docs/decisions/0029-dashboard-manifest-v0.md) and
`DashboardManifest` format 1 name the world's projection, roles and primary types, with an
optional label, kind and noun per type. It is a `dashboard-manifest` proposal resolved per world
by 0023's rule, readable at `GET /worlds/{world}/dashboard`, `s2w dashboard show` and the MCP
`dashboard` tool; nothing proposes one yet ([#307](https://github.com/daveremy/stream2worlds/pull/307), [#300](https://github.com/daveremy/stream2worlds/issues/300)). `cargo xtask scale` also holds
parse instructions per event to a CI-measured baseline of 215,248 Ir/event ([#299](https://github.com/daveremy/stream2worlds/pull/299), [#166](https://github.com/daveremy/stream2worlds/issues/166)).

**Learned:** the memory doubling had a cause we could name and remove: [#261](https://github.com/daveremy/stream2worlds/pull/261)'s second test
admitted counters, sizes and comment text that no equality, presence or order statistic separates
from a real child entity such as `user`, so the fix had to be a rule about values that never come
back. The brief for [#291](https://github.com/daveremy/stream2worlds/issues/291) assumed a name-free separating rule existed before anyone had shown
one, and the first leg spent itself finding out; plans now get a plan-only leg first. The demo
check's new timing line is a baseline: RTT-adjusted first paint of 3.1 to 3.4 s for one viewer and
5.1 to 5.9 s for four, on a browser RTT of about 160 ms, not the 0.48 s the plan assumed from curl.
The wrap run passed again (S85 failed at 8.66 s), but that reflects a quieter box, not this
sprint's code.

**Changed course:** [#291](https://github.com/daveremy/stream2worlds/issues/291) ended as a churn rule for counters plus a narrow timestamp-format
change (check 12 shifts timestamps instead of hashing them, still owed as PR 2) plus a leaf cap;
free-text shape stays out of System 1. Two plans cleared review and became nine sized child
issues: progressive render ([#292](https://github.com/daveremy/stream2worlds/issues/292), children [#293](https://github.com/daveremy/stream2worlds/issues/293) to [#297](https://github.com/daveremy/stream2worlds/issues/297) and [#303](https://github.com/daveremy/stream2worlds/issues/303)) and the
dashboard manifest ([#288](https://github.com/daveremy/stream2worlds/issues/288), children [#300](https://github.com/daveremy/stream2worlds/issues/300) to [#302](https://github.com/daveremy/stream2worlds/issues/302)). Merged main is not deployed; the demo
box stays on b207c74f by the [#282](https://github.com/daveremy/stream2worlds/issues/282) ruling until the deploy below.

**Next:** deploy [#305](https://github.com/daveremy/stream2worlds/pull/305) and [#306](https://github.com/daveremy/stream2worlds/pull/306) to the demo box and re-run the check with `--gates`, then
single-flight `/world` ([#270](https://github.com/daveremy/stream2worlds/issues/270), in review), [#291](https://github.com/daveremy/stream2worlds/issues/291) PR 2, and the dashboard proposer and labels
([#301](https://github.com/daveremy/stream2worlds/issues/301), [#302](https://github.com/daveremy/stream2worlds/issues/302)).

## Mappings merge entities, and the box's memory is explained — [#276](https://github.com/daveremy/stream2worlds/pull/276), [#279](https://github.com/daveremy/stream2worlds/pull/279), [#280](https://github.com/daveremy/stream2worlds/pull/280), [#281](https://github.com/daveremy/stream2worlds/pull/281), [#285](https://github.com/daveremy/stream2worlds/pull/285), [#286](https://github.com/daveremy/stream2worlds/pull/286), [#287](https://github.com/daveremy/stream2worlds/pull/287) (2026-09-29)

**Shipped:** a mapping's `links` now do something. When both rules of a link match with
different keys, the engine emits one merge for the pair, so `enwiki` and `en.wikipedia.org` can
name one wiki; mappings without links produce byte-identical claims
([#279](https://github.com/daveremy/stream2worlds/pull/279), [#245](https://github.com/daveremy/stream2worlds/issues/245) PR 2). The question "why does the box run out of memory" has a measured
answer: the head world had grown from 403 MiB to 887 MiB unnoticed, because the 600 MiB test
is `#[ignore]`d. A bisect with a per-type tally traced it: [#261](https://github.com/daveremy/stream2worlds/pull/261) added four non-entity
types (edit counts, comment text, byte size), +6.1M events and +347 MiB for no recall gain,
while the revision entity from [#276](https://github.com/daveremy/stream2worlds/pull/276) cost +136 MiB and earned it. mimalloc measured 84 to
102 MiB lower and returns memory after a backfill ([#280](https://github.com/daveremy/stream2worlds/pull/280), [#282](https://github.com/daveremy/stream2worlds/issues/282)). The signed contract
can no longer be edited in place: `cargo xtask check` hashes everything above the dated-notes
heading, so an in-place edit fails and an appended note passes ([#281](https://github.com/daveremy/stream2worlds/pull/281)). The Source seat
reports lag per partition: Kafka gives a number per partition, while SSE and stdin say "not
reported" rather than 0, and the watch and serve status line shows it ([#286](https://github.com/daveremy/stream2worlds/pull/286)). `score`
accepts a span opening, so the re-freeze that [#244](https://github.com/daveremy/stream2worlds/issues/244) needed is gone (only Reserved to
Heldout with the same sha256 is allowed), and `PROFILER_MODEL` is now `h-min` in the discover
crate ([#285](https://github.com/daveremy/stream2worlds/pull/285), [#287](https://github.com/daveremy/stream2worlds/pull/287), [#277](https://github.com/daveremy/stream2worlds/issues/277) PR A and B). Containment ([#276](https://github.com/daveremy/stream2worlds/pull/276), stage 5b)
merged at the start of the sprint, before planning: F1 0.44 to 0.53 on the held-out span, all six predictions hit.

**Learned:** the demo page renders but first paint took about 8.6 s at the end of the sprint
against about 3 s in Sprint 84, and the check failed on its 8 s limit; the world grows and
`/world` still queues behind the fold. A test marked `#[ignore]` hides a doubling for as long as
nobody runs it, so [#282](https://github.com/daveremy/stream2worlds/issues/282) adds a scheduled sweep. [#250](https://github.com/daveremy/stream2worlds/issues/250) closed on evidence from
earlier sprints; no work was needed this sprint.

**Changed course:** Dave set the demo's direction: the user is the watcher, and the page shows
content (page titles, user names, events as sentences) rather than metadata. The planner starts
from the domain's quintessential projection and its roles, every feature must pass "could a
pre-AI dashboard do this?", and the first-paint gate is 1 s + 2 x RTT
([#288](https://github.com/daveremy/stream2worlds/issues/288) to [#292](https://github.com/daveremy/stream2worlds/issues/292)). The demo box stays on b207c74f until the profiler guard from
[#291](https://github.com/daveremy/stream2worlds/issues/291) lands.

**Next:** single-flight `/world` ([#270](https://github.com/daveremy/stream2worlds/issues/270), in review) and progressive rendering
([#292](https://github.com/daveremy/stream2worlds/issues/292)), then the profiler guard ([#291](https://github.com/daveremy/stream2worlds/issues/291)) and [#277](https://github.com/daveremy/stream2worlds/issues/277) PR C.

## The page renders — #262, #265, #269, #243, #245 PR 1 and #235 PR 3, and #244 just after (2026-09-29)

**Shipped:** a person opening the demo now sees a world. The page opens on the type view (about
6 KB, first paint in about 3 s on the live demo) instead of downloading a 370 MB graph, and
entities load only on worlds of 5,000 entities or fewer, or around a focus entity; API callers
see no change ([#263](https://github.com/daveremy/stream2worlds/pull/263)). Every entity type
is a labelled node: node area is proportional to count, the radius is capped at 8% of the
canvas, and the type view fits the canvas, so the three types that drew as circles larger than
the canvas no longer hide the rest ([#266](https://github.com/daveremy/stream2worlds/pull/266)).
The Detail selector reads "Types" when a huge world falls back, and adding a Focus still asks
for the entity neighbourhood ([#273](https://github.com/daveremy/stream2worlds/pull/273)). The
`/world` lock is measured: an opt-in timing hook splits each read into lock wait, build and
write ([#268](https://github.com/daveremy/stream2worlds/pull/268)). Two format steps landed for
the plans that follow. A `StreamMapping` can now carry `links`, which say two values name one
entity (format version 2; mappings without links keep version 1 and their identity, and routing
refuses a linked mapping until the engine emits merges,
[decision 0027](docs/decisions/0027-stream-mapping-links.md),
[#274](https://github.com/daveremy/stream2worlds/pull/274)). `World.entities` now holds
`Arc<EntityState>`, so a write copies only the entity it changes and a write that changes
nothing copies nothing ([decision 0028](docs/decisions/0028-shared-entity-states.md),
[#275](https://github.com/daveremy/stream2worlds/pull/275)). Containment (stage 5b, `PROFILER_VERSION` 5) merged just after
([#244](https://github.com/daveremy/stream2worlds/issues/244),
[#276](https://github.com/daveremy/stream2worlds/pull/276)): scored on `reserved`, F1 rose from
0.4447 to 0.5334 and recall from 0.2866 to 0.3646, and every prediction (P1 to P6 and the
control) hit.

**Learned:** the demo check had passed all of Sprint 83 without loading the page, so the sprint
made it load the page: it now needs a rendered world twice, 1.5 s apart, and saves a
screenshot. The first render showed a second problem the numbers had not: three types drew
larger than the canvas. The lock measurement decided #235: building the response is 37–39% of
the time the lock is held during backfill, so the fix is an owned projection, shared
entity states and single-flight (option d). Cloning the maps per request (option c) would copy
221.5 MiB and take 0.62 s, so it is ruled out. Shared states cost 2.5% more memory per entity,
recorded with a `Baseline-growth:` trailer; the bytes are unchanged (golden, snapshots, fold
hash).

**Changed course:** #235 goes with option (d), planned as three parts. Part A landed in #275;
single-flight and the owned projection are filed as
[#270](https://github.com/daveremy/stream2worlds/issues/270) and
[#272](https://github.com/daveremy/stream2worlds/issues/272). The mapping-links plan's version
bump to 2 was narrowed: only mappings that use links take version 2, because bumping every
mapping would have changed the identity of each newly written one.

**Next:** single-flight `/world` and the owned projection
([#270](https://github.com/daveremy/stream2worlds/issues/270),
[#272](https://github.com/daveremy/stream2worlds/issues/272)). For gate 3,
[#245](https://github.com/daveremy/stream2worlds/issues/245) PR 2, which makes the engine emit
merges from links.

## World loads finish, the viewer stays bounded, and H-lite finds editors — #259, #250 PR 2, #220 PR A and B, and #262 just after (2026-09-29)

**Shipped:** the demo world, now titled "Living Wikipedia", loads whole. Its page had been
blank, looping "Reconnecting: Failed to fetch", because the server cut every `/world` response
off at about 4 MiB. The cause was a lock: `serve`'s bridge wrote new events into the world on
the server's only async thread while a `/world` response held the world's read lock and waited
for that same thread to send its chunks, so after 5 s the response ended at the body channel's
capacity. The bridge now waits for in-flight `/world` responses before its next write, and a
waiting response goes before the next write, so a backfill cannot starve it; a response that
waits more than 30 s answers 503, which the viewer retries
([#260](https://github.com/daveremy/stream2worlds/pull/260)). For gate 3, H-lite now finds
editors: a second entity test keys a path whose repeat groups spread across the window while
another varying path stays constant within them, so Wikipedia's `user` becomes an entity type
(`PROFILER_VERSION` 4, [#261](https://github.com/daveremy/stream2worlds/pull/261)). The bridge
memory work merged: the measurement harness now refuses any tuned-allocator variable
(`MALLOC_*`, `GLIBC_TUNABLES`, `LD_PRELOAD`) in its default sweep rather than a fixed list of
names ([#253](https://github.com/daveremy/stream2worlds/pull/253)), batch 250 lowers the
bridge's peak at a history cap of 2 from 577 to 520–538 MiB, and at the new 50,000-event default
cap the peak is 548–563 MiB, against 589 at the old default
([#255](https://github.com/daveremy/stream2worlds/pull/255)). Rustdoc errors are fixed and CI
now runs `cargo doc -D warnings`
([#257](https://github.com/daveremy/stream2worlds/pull/257)). The Sprint 82 entry landed in
[#256](https://github.com/daveremy/stream2worlds/pull/256).
Just after the sprint, the viewer stopped asking for the whole graph: it requests the type view
(`?lod=type`) first, uses its per-type counts as a size probe, and loads entities only on worlds
of 5,000 entities or fewer, or around a focus entity
([#263](https://github.com/daveremy/stream2worlds/pull/263)).

**Learned:** fixing the truncation exposed the next problem. The whole demo world is 370 MB
(310,669 nodes) at the default entity level, and the page froze trying to draw it; the type
view of the same world is 5.9 KB. The server-side demo check said PASS all sprint because it
never loads the page, so a demo PASS now needs a rendered page. H-lite's new rule was scored on
a newly pinned held-out span, `reserved-3`, opened only after the predictions were posted, and
every prediction hit: identity F1 rose from 0.2920 to 0.4431 and recall from 0.1710 to 0.2853,
with `user` at precision 0.968 and recall 1.0. Entity recovery is 0.042, still far below gate
3's 0.60, against an oracle-v0 ceiling of F1 0.5712.

**Changed course:** #259 asked for a bounded viewer timeout and a viewer-level slow-`/world`
test. Both rested on a wrong diagnosis: the body was cut short on the server, so the regression
test is server-side. #263 bounds the view in the page, not on the server: the bare `/world`
default is still the entity level, and the API's bytes, etags and pins are unchanged. The demo
page is also now meant to be the world's own dashboard, with no s2w terms, authored by System 2
as a declarative dashboard manifest on the world's log (design pass on
[#116](https://github.com/daveremy/stream2worlds/issues/116)).

**Next:** deploy #263 and check that the page renders in a browser. Rebase #243's `/world` lock
measurement onto #260, which changed the same lock code. #220 PR C (mimalloc) is re-measured on
PR B's base. For gate 3, containment
([#244](https://github.com/daveremy/stream2worlds/issues/244), `PROFILER_VERSION` 5) and the
alias limit ([#245](https://github.com/daveremy/stream2worlds/issues/245)).

## The demo is live and holds, and H-lite stops minting an action name as an entity — #216, #250 PR 1 (2026-09-29)

**Shipped:** the demo runs again and held. At 11:50 the learned Wikipedia world was OOM-killed
and the box had served an empty world since. The cause was a setting, not the code: the demo
box has 3.8 GB of RAM, and the 1 GiB limit was our own unit's `MemoryMax`. Raising it to 2.5 GB,
redeploying main and accepting the mapping again brought the world back in a 72.5 s rebuild
(210,440 events, 8.85 M claims). The new hold check, `bash scripts/s2w-demo-check.sh --hold 10`,
then passed: 11 samples over 10 minutes, `NRestarts` 0, the head advancing, and the process's
own memory flat at 755 MiB (anon). `MemoryCurrent` sits at the cap because about 1.8 GB of it is
page cache the kernel reclaims (3,292 reclaims, no OOM kills), so anon is the number to track.
For gate 3, a profiler rule now treats a small closed-vocabulary key (32 values or fewer) whose
value decides which optional fields an event carries as a `Category`, not an entity type, so
`log_action` is no longer minted as an entity (`PROFILER_VERSION` 3,
[#254](https://github.com/daveremy/stream2worlds/pull/254)).

**Learned:** the rule was scored on a newly pinned reserved span, opened only after the
predictions were posted, and every pre-registered prediction hit. Precision rose from 0.9840 to
0.9987, and spurious `log_action` mentions fell from 4,474 to 0. Recall (0.1716) and F1
(+0.0006) moved as predicted, so recall is still the gap. The Sprint 81 "1 GiB wall" was
also not a wall: it was a limit we set, and it surfaced only once the demo became the priority.

**Changed course:** the demo's limit is now 2.5 GB on the 3.8 GB box, and a demo PASS has to
hold, not pass once. The "runs in 1 GiB" goal is kept in the memory tests
([#220](https://github.com/daveremy/stream2worlds/issues/220)), not in the deployed unit.
[#250](https://github.com/daveremy/stream2worlds/issues/250) was filed as one issue and is a
split plan; #254 is its first PR.

**Next:** #220's two PRs are open and not merged: PR A
([#253](https://github.com/daveremy/stream2worlds/pull/253), measurement knobs) and PR B
([#255](https://github.com/daveremy/stream2worlds/pull/255), bridge batch 1000 to 250, history
cap 20,000 to 50,000, cap-2 peak 577 to 520–538 MiB). Both are reviewed and green; a review of
#253's latest commit found the allocator-env refusal is a fixed name list, so a named variant
skips it and the test still asserts (harness only), and that will be fixed before merge. PR C
(mimalloc) measured −12 MiB on the main-thread topology and −77 MiB on `serve`'s; the ruling is
that `serve`'s topology decides, so it will be re-measured on PR B's base. Then deploy and a
hold check. For gate 3, #250 PR 2 adds a `user` type, then containment
([#244](https://github.com/daveremy/stream2worlds/issues/244)) and the alias limit
([#245](https://github.com/daveremy/stream2worlds/issues/245)).

## The first gate-3 number, and a demo that passed once — #56 PR 2b and 3, #216 PR 2b-ii, #67 (2026-09-29)

**Shipped:** the gate-3 instrument can now freeze a mapping, prove the freeze is real, and
score it. `cargo xtask h-measure freeze` pins every key and corpus by sha256 and writes a
frozen mapping once ([#236](https://github.com/daveremy/stream2worlds/pull/236)).
`h-measure score` refuses anything whose pins moved, grades held-out corpora, and reports the
context-collision sub-metric ([#237](https://github.com/daveremy/stream2worlds/pull/237)).
`score` also re-runs the freeze on the recorded development corpus and refuses a file that
differs, so a hand-written frozen file cannot score, and it compares only the pins it uses, so
a new corpus row no longer breaks old freezes
([#242](https://github.com/daveremy/stream2worlds/pull/242)). The first number came out of it:
H-lite, frozen on the development corpus and scored once on two held-out plain-Wikipedia spans,
reaches identity F1 0.284 and 0.293 with precision 0.97 to 0.98 and entity recovery 0
([#249](https://github.com/daveremy/stream2worlds/pull/249), research note 0009). Plain
Wikipedia is reported and never counted toward gate 3 (contract B2.1), so this is not a pass or
a fail. `/world` now streams from the head one node at a time with ETag/304, and the memory test's
viewer refresh is an env knob
([#239](https://github.com/daveremy/stream2worlds/pull/239)). `cargo xtask check` gains Check
15, a report-only test for module cycles inside a crate
([#246](https://github.com/daveremy/stream2worlds/pull/246)); it found three real cycles, one
fixed by moving `grade` out of `score`
([#247](https://github.com/daveremy/stream2worlds/pull/247)) and two of the three `s2w_app`
edges fixed by moving proposal reads and `sse_cap_guard` into `query`
([#248](https://github.com/daveremy/stream2worlds/pull/248)); the last edge, through
`bridge::SourceStats`, is #240 PR 2. The Sprint 80 entry was written
late ([#234](https://github.com/daveremy/stream2worlds/pull/234)).

**Learned:** a viewer at the page's real 5 s cadence costs the backfill about 1.4×, not the 5× a 1 s
viewer showed: 60–65 s against 43–48 s with no viewer, at a peak of 876–902 MiB. One viewer
needs no redesign. The oracle-v0 reference scores F1 0.587 and 0.566 on the same spans, so
H-lite is too weak to be a baseline yet: it has no `user` or `revision` type and mints
`log_action` as an entity. The demo passed its check once and then failed to hold. After the
box was redeployed the check printed `demo: PASS`, and 2 s after `rebuild complete` the kernel
OOM-killed the process at 1,016 MiB under `MemoryMax=1G`. After the restart the backfill
reached 1,012 MiB, so the mapping was rejected again to stop a crash loop and the box is up with
an empty world. Freed memory is not returned to the OS either: with the world dropped, RSS
stayed at 906 MiB, which points at allocator settings. A point-in-time check is not a hold.

**Changed course:** #216 was closed on that single PASS and is reopened; the demo is not yet
holding. The remaining 40 to 100 MiB is now
[#220](https://github.com/daveremy/stream2worlds/issues/220) (the bridge's transients and
allocator retention), not a guess. Many concurrent viewers are their own planned fix
([#235](https://github.com/daveremy/stream2worlds/issues/235)), and a hold mode for the demo
check is filed as a process fix.

**Next:** #220, then redeploy for a `demo: PASS` that holds for 10 minutes. For gate 3, fix
H-lite's type selection and score it on a reserved span
([#250](https://github.com/daveremy/stream2worlds/issues/250)), then containment
([#244](https://github.com/daveremy/stream2worlds/issues/244)) and the mapping format's alias
limit ([#245](https://github.com/daveremy/stream2worlds/issues/245)), and then the H versus
H+S2 comparison. Check 15 flips to enforcing after #240 PR 2.

## A viewer no longer copies the world, and the gate-3 corpus is pinned — #216 PR 2a and 2b-i, #56 PR 2a (2026-09-29)

**Shipped:** `/world` and `/diff` at the head borrow the head world under the read lock
instead of cloning it, `/diff` with `from == to` answers with no projection, and the web page
refetches every 5 s instead of every 1 s
([#229](https://github.com/daveremy/stream2worlds/pull/229)). The bridge appends a whole poll
batch under one write lock, `QueryState::append_batch`, instead of one lock per claim
([#233](https://github.com/daveremy/stream2worlds/pull/233)). For gate 3, four
`mediawiki.recentchange` corpora across all 231 wikis are captured and pinned by sha256 before
any key or score existed: a 200,000-event development corpus and three held-out sets
([#227](https://github.com/daveremy/stream2worlds/pull/227)). Key format 1 adds `no_identity`,
so the 1,348 AbuseFilter events with `log_id` 0 no longer merge into one fake log entity per wiki, and
grading drops them from every prediction alike
([#231](https://github.com/daveremy/stream2worlds/pull/231)). The four identifier-domain
answers from #17 are a dated amendment to the evaluation contract
([#226](https://github.com/daveremy/stream2worlds/pull/226)). CI runs on a self-hosted runner,
with `S2W_RUNNER=down` as the escape hatch back to hosted runners
([#232](https://github.com/daveremy/stream2worlds/pull/232)), and stray `.pyc` files are no longer
tracked ([#230](https://github.com/daveremy/stream2worlds/pull/230)).

**Learned:** a backfill with a viewer attached fell from 3,983 MiB to about 1,700 MiB with the
clone gone, and to a 935.5 MiB peak with 2b-i plus the streamed `/world` of 2b-ii on top, under
the demo box's 1 GiB. Memory is no longer the limit; lock contention is. A viewer refetching
every 1 s slows the backfill from 43–48 s to 224 s, because a reader holds the lock most of the
time. Before 2b-i, the fold did not finish in 27 minutes. The corpus capture took about
2 minutes, not the planned 2 hours: history replays at about 3,000 events/s.

**Changed course:** PR 2b of #216 was planned as one ~300-line PR and grew to ~620 lines while
hitting the lock contention the plan had flagged as conditional. It is split: 2b-i
(`append_batch`, merged) and 2b-ii (streamed `/world`, ETag/304, the viewer assertion), which
is measured at the page's real 5 s cadence before any redesign. PR 2b of #56 is split the same
way, into `freeze` and `score`.

**Next:** measure 2b-ii at a 5 s viewer cadence. If the backfill stays within about 2× the no-viewer
time, review 2b-ii as built and redeploy the demo box for `demo: PASS`. If not, write a plan
that compares a snapshot `Arc<World>` handoff with plan §2(b). Review #56 PR 2b (freeze and score), then the pre-registered held-out run.
Measure where the bridge's ~170 MiB over the head world goes
([#220](https://github.com/daveremy/stream2worlds/issues/220)).

## The timeline keeps a window, not the whole stream — #216 PR 1 (2026-09-29)

**Shipped:** `serve` keeps the head world and the most recent 20,000 world events, each with the
delta it made, instead of every event since offset 0
([decision 0026](docs/decisions/0026-bounded-timeline-history.md)). A backfill of the recorded
stream cycled to 150,000 raw events (11.3 million world events) now peaks at 588 MiB for the
whole process, down from about 6.1 GiB, and `tests/backfill_memory.rs` asserts it stays under
600 MiB. SSE followers replay the stored deltas instead of each folding a private world. The
web page restarts from a fresh read when its live stream falls behind the window.

**Learned:** on the box the world was never the problem; the history was 93% of resident
memory, at ~530 bytes per world event. Even with no history at all the process peaks at
576 MiB, 170 MiB over the head world, so the next lever is the bridge's own transients. One
`/world` read at the head still clones the head world (+1.1 GiB), so a connected viewer can
still OOM the box.

**Changed course:** the planned 100,000-event window peaked at 631 MiB, over budget, so the cap
is 20,000: about 270 raw events on the demo. Time travel and SSE resume reach back seconds to
minutes, and after a restart from a snapshot, world queries serve the head only.

**Next:** PR 2 of #216 serves `/world` and `/diff` at the head without cloning the world, the
last step to `demo: PASS`. Older history from snapshots plus the log is
[#218](https://github.com/daveremy/stream2worlds/issues/218).

## A near-unique key names no type — #208 (2026-09-29)

**Shipped:** the profiler no longer turns a field whose values are nearly all distinct into an
entity type ([#214](https://github.com/daveremy/stream2worlds/pull/214)). Those fields minted
about one entity per event. On the recorded stream the discovered mapping drops from 116 to 75
claims per event and folds to 252.0 MiB at 10^5 events instead of 1,009.9 MiB.

**Learned:** the deployed mapping still OOM-killed the demo box six seconds into its backfill.
The world was 252 MiB; the timeline history behind it was not measured at all (#216).

**Next:** measure what a serve process holds during a backfill, and bound it (#216).

## Learned mappings mid-run, and what they cost — #197 PR 4b (2026-09-29)

**Shipped:** a source that reaches the 10,000-event window while `serve` runs is profiled after
that poll; the proposal and its `policy` accept land at once and the live rebuild (#184 2b-ii)
routes it without a restart. A
window this profiler already filed and someone decided is not profiled again, so a human
reject costs nothing per start. Tests now prove a held proposal writer leaves `serve` up and
unrouted, and that `serve` builds the same world shape from an obfuscated copy of a stream.

**Learned:** the discovered mapping applies at 116 claims per event on the recorded stream. At
10^5 events the world is 110 MiB if entities keep repeating and 1,010 MiB if none repeat; the
~350 MiB deploy line sits between, and the 10k-window mapping differs from the full fixture's.

**Next:** a name-free prune of relationship rules and a window rule, then deploy.

## Scale gates replay a recorded stream — #174 (2026-09-29)

**Shipped:** the scale gates now measure a recorded 10-minute Wikipedia stream alongside the
seeded generator, not instead of it
([#204](https://github.com/daveremy/stream2worlds/pull/204),
[#209](https://github.com/daveremy/stream2worlds/pull/209)). The recording is raw SSE from
`mediawiki.page_change.v1`, 11,667 events, pinned by hash and by the world it folds to (11,462
entities, 19,512 relationships). Fold instructions per event are gated in CI job `scale` and
heap bytes per entity in `cargo xtask check`, for both supplies. The SSE frame parser the
fixture uses is now the live adapter's, shared with xtask's replay.

**Learned:** real traffic folds at 15,285 Ir per raw event against the generator's 5,764, but a
raw event maps to about 5 claims, so per claim it is about 3,060. It holds 346 bytes per entity
against the generator's 360, both inside decision 0004's 2× line: the gap to the 300 B planning
figure is about the same on observed data as on chosen data. Live traffic ran at about 19.4
events/s, more than twice the rate in the earlier 3-minute capture, so the fixture is 34.2 MB
rather than the planned 15 MB; it is still committed raw, without LFS. It contains 17 distinct
IPv4 addresses of logged-out editors, which is public Wikimedia data under CC BY-SA.

**Changed course:** the issue asked to replay the recording *instead of* the generator. The
two answer different questions (regression on a 10^6-event world versus real-workload shape),
so both stay gated ([decision 0004](docs/decisions/0004-scale-envelope.md), amendment). The pin
is FNV-1a 64, not sha256, to avoid a new dependency.

**Next:** parse cost on the same fixture (#166) and fork cost (#167).

## Learned mappings fill a world at start — #197 PR 4a (2026-09-29)

**Shipped:** `serve` profiles the first 10,000 logged events of each unrouted source with
`s2w-discover`, files the mapping as a `stream-mapping` proposal from `h-lite`, and accepts it
with a `policy` decision, the first `policy` writer in the codebase
([decision 0025](docs/decisions/0025-learned-mapping-auto-apply.md)). The source is routed and
backfilled in the same start, which is what fills the demo world. The producer is idempotent
by (source, identity), so a restart and a human reject are both stable, and it holds the
proposal writer only for the write.

**Next:** PR 4b profiles a source that reaches the window mid-run, proves obfuscation
invariance through `serve`, and measures the world a learned mapping builds.

## Sprint 77 — routes become data, the world gets lighter (2026-09-29, 03:00–05:00)

The sprint's aim was the path from a raw stream to a populated world with no domain code:
profile the stream, propose a mapping, accept it, and have `serve` run it. The back half of
that path landed: a human can accept or reject a mapping from the command line, and `serve`
runs the accepted one at its next start. The demo world is still empty: no mapping has been
proposed and accepted for it yet. Alongside, the scale gates went live and the first memory cuts came in under them.

**Shipped**
- **Routes from stored mappings** ([#187](https://github.com/daveremy/stream2worlds/pull/187),
  [decision 0023](docs/decisions/0023-routes-from-stored-mappings.md)). `serve` reads the
  proposal store and routes each source to the mapping its accepted `stream-mapping` proposal
  names. The engine is named `mapping-<identity>`, a hash of the mapping's canonical JSON and its key and mapping format versions, so
  replacing a mapping can never replay the old one's verdicts or restore its snapshots.
- **Human review from the command line** ([#195](https://github.com/daveremy/stream2worlds/pull/195),
  issue [#185](https://github.com/daveremy/stream2worlds/issues/185)). `s2w proposals
  list|grade|decide` shows the proposal store and which mapping each source runs, and records a
  human accept or reject; a reject revokes the mapping. Details in the #185 entry below.
- **Profiler obfuscation replay, check 12** ([#182](https://github.com/daveremy/stream2worlds/pull/182)).
  `s2w-discover` profiles a recorded stream plain and fully renamed and hashed; both runs must
  propose the same mapping (12 types, 19 entity rules, 143 relationship rules on the fixture).
- **Scale fitness gates** ([#175](https://github.com/daveremy/stream2worlds/pull/175)): bytes
  per entity in `cargo xtask check`, fold instructions per event in CI job `scale`, and storage
  figures on `watch`'s status line. Details in the #175 entry below.
- **Memory cuts under the new gates.** Snapshot restore shares one world instead of holding two
  ([#186](https://github.com/daveremy/stream2worlds/pull/186)): folding 10^6 events the restore
  peak fell from 1,624 MiB to 861 MiB and the snapshot write peak from 919 MiB to 50 MiB.
  Attributes stored as a sorted `Vec` ([#194](https://github.com/daveremy/stream2worlds/pull/194))
  took the fold from 830 to 438 bytes per entity; the gate's ceiling followed it down to 600 B.
- **Decision-number hygiene.** The snapshots record, which had collided with the stream-mapping
  record at 0021, became [0024](docs/decisions/0024-snapshots.md)
  ([#188](https://github.com/daveremy/stream2worlds/pull/188)), and check 14 now fails any
  decision number held by two files ([#193](https://github.com/daveremy/stream2worlds/pull/193)).
  `--tighten-baseline` no longer exits 1 over report-only module-size findings after a
  successful memory tighten ([#196](https://github.com/daveremy/stream2worlds/pull/196)).

**Learned**
- **The bytes were containers, not data.** One 544 B B-tree leaf per entity held about three
  attributes. The next costs are the same kind, the `entities` and `keys` maps, which is why
  dense entity ids are next.
- **Nothing checked decision numbers.** The stream-mapping and snapshots records both took
  0021, and only a filed issue ([#181](https://github.com/daveremy/stream2worlds/issues/181))
  caught it. Check 14 makes the next collision a build failure.
- **The attribute change also cut instructions.** Fold Ir per event
  measured 8,686 on #194's CI run against the 9,093 baseline (−4.5%); the baseline is
  human-owned and was not lowered.

**Changed course:** none.

**Next**
- Fill the demo world ([#163](https://github.com/daveremy/stream2worlds/issues/163) PR 4).
- The epoch contract and live rebuild, so a running `serve` picks up a decision without a restart ([#184](https://github.com/daveremy/stream2worlds/issues/184)).
- Entities indexed by dense id ([#190](https://github.com/daveremy/stream2worlds/issues/190)).

---

## World memory: 438 to 360 bytes per entity — #198 (2026-09-29)

**Shipped:** the world keeps its entities in a `Vec` indexed by id instead of a `BTreeMap`, and an
entity with no hub references no longer carries an empty map inline (`EntityState` is 56 B, was
72). The fold's heap per entity fell from 438 B to 360 B on the scale generator (1.20× decision
0004's 300 B target) and the gate's baseline moved with it. Snapshot bytes and `world_hash` are
unchanged, now pinned by two tests; a snapshot whose entity ids are not exactly `0..n` no longer
loads.

**Learned:** the cut was 78 B, not the ~110 B estimated. The B-tree nodes cost 152 B per entity,
and the `Vec` that replaced them is not free: it doubles, so 100,000 entities sit in 131,072
slots of 56 B, 73 B each. The figure will step at every power of two.

**Changed course:** none.

**Next:** interned type and attribute names
([#191](https://github.com/daveremy/stream2worlds/issues/191)), the cut expected to cross 300 B.

## Human review from the command line: `s2w proposals` — #185 (2026-09-29)

**Shipped:** `s2w proposals list|grade|decide`, part of #163 (PR 2c). `list` prints the proposal
store with its decisions and which stream mapping each source runs; `grade` prints the grades
per class and actor; both read without a lock and never create the store. `decide` appends a
`human` decision, the only decider whose review grades a producer, and prints what the decided
mapping's source runs afterwards, so a human reject visibly revokes a mapping. MCP
`decision_record` and the CLI now share one service, `s2w_app::proposals`: the same validation,
the same rule that an unknown id never creates the store, and the same JSON error body.

**Learned:** the store has no reviewer column, and decision 0019 already lets a basis name a
reviewer, so the reviewer goes into the basis as `reviewer=<id>; <basis>` rather than into a
schema change. `--reviewer` refuses `;` and whitespace so the prefix always splits cleanly.

**Changed course:** an accept on a `stream-mapping` proposal whose payload does not decode is
now refused at write time. Routing excludes that row whatever is decided, so the accept could
never take effect; a reject is still allowed so a bad row can be revoked.

**Next:** #184 (PR 2b) makes a running `serve` pick up a decision without a restart; until then
the change lands at the next start. The web view's human-decision surface is still open (#160).

## World memory: 830 to 438 bytes per entity — #194 (2026-09-29)

**Shipped:** each entity's attributes now live in a sorted `Vec` (`AttrMap`) instead of a
`BTreeMap`. The fold's heap per entity fell from 830 B to 438 B on the scale generator, and the
gate's ceiling dropped from an interim 900 B to 600 B, decision 0004's 2× line. Snapshots, the
`world_hash` and every JSON surface are byte-for-byte what they were; a new test pins the golden
world's hash.

**Learned:** the bytes were container overhead, not data. A dhat breakdown showed one 544 B
B-tree leaf per entity for about three attributes, 65% of the total. The next biggest costs are
the same kind: B-tree nodes for the `entities` and `keys` maps.

**Changed course:** none.

**Next:** entities indexed by their dense id
([#190](https://github.com/daveremy/stream2worlds/issues/190)) and interned type and attribute
names ([#191](https://github.com/daveremy/stream2worlds/issues/191)), the two cuts estimated to
bring the figure toward decision 0004's 300 B target.

## Scale fitness function: fold Ir and bytes per entity gated — #175 (2026-09-28)

**Shipped:** the first scale numbers as fitness functions. `cargo xtask scale` (Linux, Valgrind,
new CI job `scale` on `ubuntu-24.04`) runs a `gungraun` benchmark that folds 100,000 generated
events and judges instructions per event against `xtask/scale-baseline.toml` at a 5% tolerance.
`cargo xtask check` gains check 13: an ignored `dhat` test
measures heap bytes per entity after the fold and gates it against the same file, with a
hard interim ceiling. Raising a baseline
needs a `Baseline-growth: s2w#<N>` trailer, the same rule as module-size exemptions, and a
measurement that cannot be read fails instead of passing. `cargo xtask scale` also runs an append
benchmark, one event per transaction: it must run, and its events/s is reported, not judged (a
tmpfs number is labelled and excluded from comparison). `s2w watch`'s progress line now shows
the store's size (every SQLite file in the log directory) and days until its disk is full.

**#32's literal finish line is not fully met.** No recorded 10-minute Wikipedia fixture was
committed; a seeded synthetic generator (`crates/s2w-app/tests/support/scale_generator.rs`)
feeds every measurement instead. Not measured, deferred to follow-ups: parse instructions per
event (#166), fork cost (#167, blocked on the gate-4 fork API), source lag per
partition (#168), and fold-thread utilisation plus the entities and bytes-per-entity status
fields (#169). The `[ir]` baseline (9093 Ir/event) comes from the first run of CI job `scale`
(run 36527045232): it belongs to the CI image, so only a CI run sets it. Raising `[ir] events`
or `[memory] entities` counts as baseline growth, because a larger run lowers the per-unit figure.

**Learned:** measured on the synthetic generator, the fold holds **830 bytes per entity** (dhat
live heap, test profile), 2.77× the 300 B that decision 0004 planned from arithmetic. That is
past the record's 2× line, so the record now carries a dated note and the ruling (karpathy) accepts an interim 900 B ceiling; the 300 B target stands and #172 tracks the cut. Options were: a new
memory target or a smaller footprint. At 830 B, 10^6 entities are about 0.83 GB of live heap,
but that figure leaves out relationships (234 B each), allocator overhead and a real stream's
entity distribution (the generator's is chosen, not observed), while the 1 GB target is RSS, so
it does not show that 10^6 entities fit in 1 GB. Instruction counts measured locally (about 8,860 per event) are
not the baseline; only the CI image's count is. `cargo test --exact` with a wrong name runs
zero tests and exits 0, so the memory check requires the test's JSON line rather than trusting
the exit code.

**Changed course:** the recorded Wikipedia fixture from research 0006 §12 gave way to a synthetic
generator, so the numbers are reproducible without committing tens of megabytes of stream data;
parse and fork measurements moved to their own issues rather than holding this PR.

**Next:** #172 (cut bytes per entity toward 300 B, decision 0004);
 the four follow-ups above.

---

## World snapshots, part 1b: `serve` restarts from a snapshot — #33 (2026-09-28)

**Shipped:** `serve` now writes world snapshots and restarts from them
([decision 0021](docs/decisions/0024-snapshots.md)). At start it loads the newest valid
snapshot, installs it as the timeline's base and resumes the bridge after its log position, so
a restart replays only the tail. While running it snapshots every 1,000,000 raw events on its
own writer thread, and on Ctrl-C or SIGTERM it writes a final snapshot when at least 100,000
events arrived since the last one. **User-visible:** `serve --snapshot-every <n>` and
`--no-snapshot`; `serve` now stops on SIGTERM as well as SIGINT; `/time.base` becomes the
snapshot's offset after a restart; an SSE follower whose position falls below a newly installed
base gets one `event: error` frame with `offset_before_base` before the stream closes. An
integration test restarts `serve` on the same log directory and checks that restore plus tail
serves the same world as a full replay.

**Learned:** measured on a synthetic wiki-shaped world, a final write takes 0.18 s at 10^5 raw
events and 2.4 s at 10^6, well inside the 25 s the demo unit's stop budget leaves after the
HTTP drain. Memory is the tighter limit: a restored timeline holds a base and a head world, and
a write briefly holds a second head, so near 10^6 events of that shape `serve` would pass the
demo box's 1 GiB cap (as the event list without snapshots already would).

**Changed course:** the plan captured a snapshot only after a poll with no reported error. The
implementation captures after every successful poll, because a poll that reports a per-event
error still commits a consistent prefix; the decision records why.

**Next:** share the base world with the head after a restore to halve restore memory (#179); `mcp
--log-dir` loading snapshots; log and verdict compaction (part 2).

## World snapshots, part 1a: the format and the timeline base — #33 (2026-09-28)

**Shipped:** the pieces a restart-from-snapshot needs, without yet wiring them into `serve`
([decision 0021](docs/decisions/0024-snapshots.md)). A world snapshot is now a defined,
portable file (magic, length, a `postcard` payload, FNV-1a checksum) with pure validity rules:
it loads only when its format, fold (`FOLD_VERSION`, hub cap and a hash of the golden
fixtures), engine routing and log position all still match, and is otherwise ignored, never
deleted. The served timeline gained a base world and a live head world, so the world at the
head is a clone instead of a refold. **User-visible:** a new stable error,
`offset_before_base` (HTTP 410), answers any offset below a restored snapshot's base on
`/world`, `/diff`, `/events` (including `Last-Event-ID` resume), entity history, and `/time?ts=`
before the base; `/time` reports the new `base` field, which stays 0 until `serve` restores
from a snapshot. The MCP tool descriptions name both.

**Learned:** the SSE follower indexed the event list by absolute offset; once a timeline starts
at a base, that silently streams the wrong deltas instead of failing. Replacing
`Timeline::events()` with a base-relative `events_after(offset)` made every index site go
through one checked door, and a test that restores at offset 10 and compares the SSE bytes with
the full history's catches the old indexing.

**Changed course:** the engine-routing fingerprint hashes routes in registration order, not as
a sorted set, because the bridge runs matching engines in that order and reordering can reorder
claims. The golden-fixture hash is a pinned constant checked by a test rather than an
`include_bytes!`, which the module-size walker cannot see through.

**Next:** part 1b: the snapshot writer thread and its every-1,000,000-events trigger, `serve`
loading the newest valid snapshot and resuming the bridge from its log position, a final
snapshot on SIGINT/SIGTERM, and `--snapshot-every` / `--no-snapshot`. Then the view's scrubber
floor at `/time.base`.

---

## Generic field filter, replacing `--wiki` — #131 (2026-09-28)

**Shipped:** a generic, data-driven `--filter <json-path><op><value>` flag (repeatable) on
`watch` and `serve`, replacing the retired `--wiki` canary/examplewiki drop with a mechanism
any stream can use. `s2w-sources::filter` (`FieldFilter`, `FilterOp::{Eq,Ne}`) parses specs and
matches them against an event's decoded JSON; `sse::filtered::FilteredDialect` applies them as
a decorator around any `SseDialect`, filtering pre-envelope so a dropped frame never reaches
the log. `registry::resolve` takes the caller's filters and wraps SSE sources automatically;
Kafka and stdin refuse loudly (`ResolveError::FiltersUnsupported`) rather than silently
ignoring a filter they cannot honor. Presets (`presets::PRESETS`) carry their own default
filter specs as a fourth column — the `wikipedia` preset's canary/examplewiki drop is now data,
not code. `cargo xtask check`'s obfuscation-replay check (added for #119) now also covers the
`s2w-system1` engine layer (`JsonClaimsEngine`), reusing the same golden fixture through a
hand-built `RawEvent` per event rather than a new one.

**Learned:** a required end-to-end test (a canary-shaped frame driven through the real loopback
SSE harness with the `wikipedia` preset's exact filter specs, asserting it never reaches the
SQLite log) is what actually proves the mechanism works — round 1 of plan review found that
the original plan would have compiled and passed unit tests while silently no-op'ing the
filter in production.

**Changed course:** none — the original scope (replace `--wiki` with a generic filter) held;
plan review round 1 tightened where the mechanism lives (in `s2w-sources`, applied at
`SseDialect::accept`, not threaded through the CLI layer) and added the required end-to-end
test above.

**Next:** Kafka/stdin `--filter` support (today they refuse loudly instead) —
[#134](https://github.com/daveremy/stream2worlds/issues/134). Obfuscation-replay coverage for
the bridge registry (`s2w-app::Bridge`/`EngineRegistry`) remains a documented gap —
[#135](https://github.com/daveremy/stream2worlds/issues/135).

---

## No compiled domain code — #119 (2026-09-28)

**Shipped:** retired the Wikimedia-bound compiled engines (`WikimediaPageChangeEngine`, the
local-embeddings engine and its `model2vec-rs` dependency), the `Wikimedia` SSE dialect, and
the `--wiki` flag; the `wikipedia` preset is now URL+settings data over the generic `sse`
transport, with no domain-named dialect behind it (decision 0018). Two new `cargo xtask check`
fitness functions enforce the rule going forward: a vocabulary scan (`xtask/src/vocabulary.rs`)
that denylists domain terms across every crate's `src/` tree and the web view's TypeScript, and
an obfuscation-replay check (`xtask/src/obfuscation.rs`) that folds the golden event log twice —
once plain, once with every claim identifier/attribute-key/string value renamed and hashed —
and fails if the two folded worlds differ, catching code that reads a specific name or value
instead of just shape. Also retired the now-dormant version-history append-only check
(`xtask/src/version_history.rs`), a vestige of the deleted embeddings engine.

**Learned:** the token-boundary design for the vocabulary scan (a denylist entry matches a
contiguous run of tokens, so `wiki_id` and `wikiId` both hit a `wiki` entry) is what makes one
entry catch every spelling convention, at the cost of needing an explicit `// vocabulary: allow`
escape hatch for legitimate data (e.g. the `wikipedia` preset's own name and URL).

**Changed course:** domain-pack crates (this issue's original design) were superseded same-day
by Dave's ruling that no compiled code for a domain should exist at all — see decision 0018 and
the issue's superseding comment.

**Next:** a generic, data-driven field filter (`--filter <json-path>=<value>`) to replace what
`--wiki` provided, plus obfuscation-replay coverage for the System 1 bridge/engines layer —
[#131](https://github.com/daveremy/stream2worlds/issues/131).

---

## Read-only MCP over an on-disk world — #115 (2026-09-28)

**Shipped:** `s2w mcp --log-dir PATH [--world NAME]` opens the event and verdict SQLite
databases without either writer lock, reconstructs a bounded one-shot timeline from durably
stored verdicts, and serves it through the existing five MCP tools while another process can
keep writing.

**Learned:** the verdict cursor must be captured once to give replay an explicit upper bound,
and multiple stored versions from one engine must collapse to the first-written row or their
claims are served twice.

**Changed course:** read-only replay does not run engines and does not load manifest or
membership metadata. Without an engine registry it serves the first stored verdict for every
historic engine, including retired ones.

**Next:** add live snapshot refresh ([#128](https://github.com/daveremy/stream2worlds/issues/128))
and read-only manifest/membership loading ([#129](https://github.com/daveremy/stream2worlds/issues/129));
decide whether `mcp --log-dir` should gain an engine-policy configuration surface.

---

## Domain as data — decisions 0017/0018, readable view, research 0008 (2026-09-28)

**Shipped:** [Decision 0017](docs/decisions/0017-view-and-agents-first-class.md) makes
visualization and agent use first-class: a view spec is authored by System 2 and stored as a
log event, shape comes before domain, and every gate reports a lifetime baseline plus surprise
against it. [Decision 0018](docs/decisions/0018-no-compiled-domain-code.md) retires compiled
domain code entirely — no crate, type, flag or branch may name or key on a domain; entity
types, labels and links are discovered data, enforced by a domain-vocabulary scan and an
obfuscation replay. The live web view ([#114](https://github.com/daveremy/stream2worlds/issues/114))
now reads readable from world data alone: a derived per-entity-type label (highest
coverage×distinctness string attribute, id-like values rejected), deterministic colors with a
legend, degree-based sizing, and "Active now"/"Hubs" panels. `serve`/`mcp --json` gained the
same structured stdout/stderr progress `watch --json` already had ([#110](https://github.com/daveremy/stream2worlds/issues/110)
part 1 — MCP itself is tracked separately as #115). [#123](https://github.com/daveremy/stream2worlds/issues/123)
closed the review gap on #114: a rename-invariance test proving the label pick survives an
attribute rename (0018's own acceptance test), derivations computed once per snapshot instead
of once per SSE message, and three label-scoring fixes (score-before-whitespace tie-break, a
digit/dash guard on the id-like heuristic, hub ranking using the larger of recorded and
visible degree).

**Learned:** [Research 0008](research/0008-view-spec-and-surprise.md) (sagan) grounds 0017's shape: a
System 2-authored view spec, shape-before-domain, a lifetime baseline with surprise scoring,
and a readiness rule frozen to the H+S2 arm — the obfuscated view form is reported, not graded
as a pass/fail.

**Changed course:** `WikimediaPageChangeEngine` and the Wikimedia-bound embeddings schema are
retired per 0018; a new stream shows raw identifiers and inferred shapes until the H heuristics
land, which is the honest state of the product rather than a regression to hide.

**Next:** the H heuristics (research 0002 §6's seven domain-free stages) that make a new
stream typed without any domain code.

---

## World membership implementation — #95 (2026-09-28)

**Shipped in the working branch:** persistent world identity, an append-only source membership
history, atomic v2 migration, generation-bound ingestion, and the source-history read endpoint.

**Learned:** source startup can launch a producer before the pump's first poll, and one Kafka
adapter represents several partition identities. Startup checks now run before producers begin.

**Changed course:** re-add with `Now` fails explicitly because current adapters cannot promise
a live-tail restart. The binding storage plan records raw event heads, which are not generally
fold offsets; the README records this remaining limitation rather than promising equivalence.

**Next:** review the storage/bootstrap and ingestion/HTTP parts; provide an admin mutation path,
resolve the offset spaces, add adapter live-tail support, and support resuming workers live.

---

## Gate 2 — the local web view (2026-09-27)

**Shipped:** `s2w serve` now serves a web view on its loopback port: a 2D entity graph and an
evidence table of the newest 500 deltas, fed by Server-Sent Events. The URL holds the whole
view (`world`, `at`, `branch`, `lod`, `focus`, `hops`), so a pinned `?at=` link reopens the
exact moment. The TypeScript bundle is committed and embedded in the binary with `memory-serve`,
so a Rust build never runs Node. A CI `bundle` job rebuilds it from scratch and fails on any
drift, and a licence gate rejects anything outside MIT, Apache-2.0, ISC, BSD and 0BSD. A fixture
proves the gate rejects GPL-3.0 and CC-BY-NC-4.0 packages. This closes Gate 2's last item.
[Decision 0016](docs/decisions/0016-web-delivery.md)

**Learned:** Entity deltas cannot patch a graph that shows hubs and type aggregates, so the
graph never applies a delta: any change triggers at most one snapshot refetch per second.
Deltas feed only the evidence table, seeded from a bounded `/events?from=&at=` replay so a
pinned view's table and graph agree on "as of when". Embedding the view costs
1.7 MB of stripped binary (48.2 MB to 49.9 MB) and 14 new crates, measured, not estimated.

**Changed course:** Two carried-over items from `s2w serve` became requirements before a
browser could connect: a cap of 32 concurrent event streams, whose slot the stream itself holds
until the client disconnects, and an `Origin` allowlist alongside the existing Host allowlist.
Loopback plus both allowlists is the whole boundary; there is still no authentication.

**Next:** The 3D world explorer ([#21](https://github.com/daveremy/stream2worlds/issues/21))
replaces only the graph renderer behind the same interface.

---

## Sprint 64 — NDJSON watch progress (2026-09-27, 21:00–22:50)

The follow-up #39 deferred: `s2w watch --json` ([#79](https://github.com/daveremy/stream2worlds/issues/79)).

**Shipped**
- `s2w watch <source> --json`: one NDJSON object per flush on stdout
  (`{"appended","duplicates","reconnects","cursor","at"}`), and `{"error","fatal"}` lines on
  stderr for source errors and the one fatal error that stops the pump — a clean machine-readable
  stream alongside the existing human progress lines, never both at once.
- `crates/s2w-app/src/group_commit.rs` gained a `pub` `Reporter` trait (`HumanReporter`
  reproduces today's eprintln lines exactly) so `pump`/`pump_events` route through one seam
  instead of hardcoded `eprintln!`. `serve` keeps `HumanReporter` — `serve --json` is out of
  scope for this issue. The concrete `--json` reporter (`JsonReporter`) lives in
  `crates/s2w/src/reporter.rs`, not in `s2w-app` — it needs `output.rs`'s rendering seam, which
  `s2w-app` cannot depend on (round 2: the first-draft `JsonReporter` in `group_commit.rs` built
  its own JSON via `serde_json`, a second JSON path the issue explicitly ruled out). It renders
  three new `output.rs` helpers (`render_progress_line`, `render_source_note`,
  `render_source_error`) built by hand like the existing `render_stream_error`, and is stateless
  — every counter (`appended`/`duplicates`/`reconnects`) is tracked by `pump` in `s2w-app` and
  only rendered on the CLI side, per `crates/s2w/AGENTS.md`'s "no logic here beyond argument
  parsing and output formatting".
- `crates/s2w/AGENTS.md`'s dated exception (2026-09-27, #39) is resolved: `watch` now has
  `--json`; only `serve` still lacks one.

**Learned**
- `Reporter` needs an explicit `Send` bound because `crates/s2w`'s concrete `--json` reporter
  crosses a `tokio::spawn` in its own test harness (`crates/s2w-app/tests/kafka_broker.rs`), and
  because `&mut dyn Reporter` is threaded through `pump`'s `impl FnMut` closures, which the
  compiler requires to be `Send` the moment any caller might spawn them — `dyn Trait` isn't
  `Send` by default. Production `pump`/`report_progress` are only ever joined with `select!` on
  one task (see the doc comment on `pump_events`), never spawned.
- `"at"` is epoch milliseconds (`i64`), not an RFC3339 string, matching this codebase's existing
  convention (`query/timeline.rs`'s `first_ts`/`last_ts`) rather than the issue's original sketch
  — there's no RFC3339-rendering helper anywhere in the workspace to reuse.
- Round 1 and round 2 code review (codex) each found a real, in-scope bug the plan missed:
  round 1, a benign startup note (`"no stored cursor; starting fresh"`) rendered as
  `{"error":...}`, indistinguishable from a real source error to a `--json` consumer filtering
  stderr for `"error"` — `Reporter::note` split into `note` (benign, `{"note":...}`) and
  `source_error` (`{"error":...,"fatal":false}`). Round 2, the reconnect counter had moved into
  the CLI-side reporter along with the JSON rendering, silently reintroducing counting logic
  crates/s2w/AGENTS.md forbids — moved back into `pump`.

**Next**
- `serve --json` progress, if a future issue asks for it — explicitly out of scope here.

---

## Sprint 64 — world-scoped query APIs (2026-09-27)

**Shipped:** Every HTTP query is now scoped as `/worlds/{world}/…`, `GET /worlds` discovers
the one world served by the process, and all five MCP tools require the same `world` string.
`s2w serve` uses `default` unless `--world <name>` selects another URL-safe identifier.

**Learned:** Carrying world identity through route extraction, MCP schemas and CLI dispatch
from the start makes a forgotten dispatch setting observable; a configured-world integration
test now proves the server does not silently fall back to `default`.

**Changed course:** The unscoped HTTP routes and optional MCP shape were removed rather than
aliased. Clients use the named-world contract before the evidence view ships, avoiding a later
compatibility surface. [Decision 0015](docs/decisions/0015-named-worlds.md) records the design.

**Next:** A separate follow-up adds the manifest and append-only source-membership log; this
sprint deliberately does not create either.

---

## Sprint 63 — local embeddings (2026-09-27, 19:00–20:50)

A third System 1 engine ([#64](https://github.com/daveremy/stream2worlds/issues/64)), following
#63's verdict log: local embeddings classify an `enwiki` edit comment into a category by
similarity, additive alongside the existing rules engine on the same page.

**Shipped**
- **`LocalEmbeddingsEngine`**, using `model2vec-rs` with potion-base-8M (vendored, no network
  fetch), split into a plain `CommentClassifier` (no `Engine` dependency, reusable by a future
  System 2/Jev consumer) and a thin `Engine` adapter. Scoped to `enwiki` only; abstains below
  threshold or on a near-tie rather than guessing. [Decision 0013](docs/decisions/0013-local-embeddings-engine.md).
- **Provenance now covers two hashes**, model identity and taxonomy/config, independently
  visible in a stored verdict.
- **Version-bump enforcement is a table, not a pinned pair**: an append-only
  `(version, model_hash, config_hash)` history, with the shipped version derived from the
  table's last row so a hash change and a version bump can't drift apart.
- **A golden-output test on 8 real English edit comments** catches a `Cargo.lock` bump or
  scoring change that a model/config hash alone would miss, and doubles as a calibration
  sanity check: one of the eight lands in `NoMatch` on real, plausibly-worded text.
- README's "System 1 engines" row moves from "next" to built, and a new "How embeddings fit in"
  section explains the engine at the application level.

**Learned**
- One real comment in the golden set — a revert notice — lands in `NoMatch` rather than a
  confident match, evidence (not just a constructed boundary test) that the starting
  `threshold_bps`/`margin_bps` values may be too strict on real `enwiki` text. Named as a
  calibration follow-up in decision 0013, not built here.
- The golden test must run each case through the same `normalize_comment` step production
  runs before classification — an earlier version pinned one case's raw, marker-prefixed text,
  which described a classification production never actually produces (code review round 2,
  s2w#64).
- `model2vec-rs`'s `default-features = false` alone does not compile: `tokenizers` needs
  `onig` or `fancy-regex`. Chose `fancy-regex` over `onig` (which binds the C Oniguruma
  library) — the plan's round-2 dependency-tree check never actually compiled, so this was
  invisible until implementation, and it surfaced a second `cargo deny` advisory
  (`RUSTSEC-2025-0119`, `number_prefix` via `indicatif`) that round 2's check against the
  non-compiling tree couldn't have found either. `fancy-regex`'s own regex engine is pure
  Rust, but the tree still needs a C++ toolchain regardless of this choice: `tokenizers`'s
  `esaxx_fast` feature pulls in `esaxx-rs`, which compiles C++ via `cc` (decision 0013).
- Full-mode plan review hit its 2-round cap with both reviewers still blocking on real,
  convergent findings (the version-bump test and the `BelowThreshold`-reused-for-a-near-tie
  issue); a karpathy ruling folded all four remaining findings into implementation rather than
  spending a third plan-text round on changes that didn't alter the plan's shape.

**Next**
- Threshold/margin calibration against real `enwiki` comment traffic.
- Full boilerplate-template stripping beyond the single leading `/* Section */` marker.
- Jev joins behind the same `Engine` trait once its latency/cost/accuracy are measured.

---

## Gate 2 — live HTTP command (#10, PR1, 2026-09-27)

**Shipped:** `s2w serve <source>` owns source ingestion, durable verdicts and the live query API
in one process. It binds loopback, checks Host headers, refuses competing store owners and
bounds HTTP shutdown even with an open SSE client.

**Learned:** the existing bridge's async runner requires `Send`; a local `poll_once` loop lets
non-Send source streams and a single shared SQLite handle coexist on the current-thread runtime.

**Changed course:** the proposed separate watch/serve processes and lockless reader were
replaced by one owner. Loopback binding also needs a Host allowlist against DNS rebinding.
[Decision 0014](docs/decisions/0014-serve-topology.md) records both choices.

**Next:** PR2 adds the evidence view, assets and bundle gates. The dashboard remains unfinished.

---

## Sprint 62 — watch fails loudly (2026-09-27, 17:00–19:00)

The four hardening items deferred from the #29 review ([#39](https://github.com/daveremy/stream2worlds/issues/39)).
`watch` no longer looks healthy when it isn't: every reconnect says why, `--since` is validated
before any connection opens, and the stored-cursor-versus-`--since` decision is a pure, tested
function instead of live-run glue.

**Shipped**
- **SSE reconnects are reported, never silent.** Connection failures (with attempt count and
  retry delay), dropped byte streams, and zero-frame disconnects all send a `Retrying` error
  down the channel before the backoff; a connection that delivered frames and then closed
  (Wikimedia's routine periodic reconnects) stays quiet. The attempt count resets only when an
  event is accepted, not when a connection succeeds — a 200 that delivers nothing keeps
  counting.
- **`--since` is validated, and invalid values are exit code 2 everywhere.** Wikipedia and
  Kafka share one RFC 3339-or-epoch-millisecond parser (in `s2w-sources`); the SSE
  `SseDialect::apply_since` distinguishes unsupported from invalid, and invalid is a usage
  error. This intentionally narrows Wikipedia's old ISO-8601 wording: a bare date such as
  `2026-09-27` is not RFC 3339 and is now rejected instead of being forwarded silently.
- **The start decision is pure and positively tested.** `sse::start::choose` decides resume
  versus fresh versus error from `(stored cursor, --since)`; `SseSource::start` calls it instead
  of inlining the logic. An app-level loopback test (hand-rolled HTTP over `tokio::net`, no new
  dependency) seeds a cursor, asserts the request carries it as `Last-Event-ID`, and watches the
  event land in the SQLite log.

**Learned**
- **A conflict beats a typo.** When a stored cursor and `--since` are both present, the
  `SinceWithStoredCursor` error wins over validating the `--since` value — the user's mistake is
  the combination, and naming it first saves them fixing a value that was going to be refused
  anyway.

**Changed course**
- **`--json` on `watch` became a dated exception** instead of shipping untested: `watch` is
  streaming, and its NDJSON progress design is deferred to [#79](https://github.com/daveremy/stream2worlds/issues/79);
  `mcp` is already JSON-RPC over stdio. `crates/s2w/AGENTS.md` records the exception.

**Next**
- [#79](https://github.com/daveremy/stream2worlds/issues/79), when a machine consumer needs
  progress from a running watch.

---

## Sprint 61 — the live bridge (2026-09-27, 15:00–17:00)

The log and the world met. Until this sprint the query API and the MCP server served a world
that only a golden replay could fill; now a bridge reads the stored log, asks System 1 engines
what each raw event claims, and folds the claims into query state. It runs as a library, not yet
behind a command. Two engines ship behind one trait, and the second one is what will let anyone
write a world by hand from a file of claims.

**Shipped**
- **The live bridge: log → System 1 engines → query state** ([#51](https://github.com/daveremy/stream2worlds/issues/51), [decision 0011](docs/decisions/0011-system1-bridge.md)). `s2w_system1::Engine` names itself (`name()`, `version()`) and judges with `evaluate(&RawEvent) -> Verdict`, which is total: it never errors and must not panic; a `Verdict` is `Propose { claims, confidence }` or `Abstain { reason }`, and abstaining is a value with a named reason (`NotMine`, `Unparseable`, `Insufficient`, or `Panicked`, which only the bridge produces). Confidence is an integer in basis points, because a float threshold replayed across machines would break byte-identical replay just as floats in the world would. `Bridge<R: LogReader>` polls the log (SQLite has no cross-process notification), backs off on empty polls, and every matching engine runs in registration order, so the timeline is a deterministic function of the log and the registry.
- **Two engines, not one.** `WikimediaPageChangeEngine` turns a page-change event into claims by rules; `JsonClaimsEngine` treats a payload that already is a claim as one (Dave's choice for the second engine). The second is what will let a file of hand-written claims piped through `s2w watch -` fold into exactly the world you wrote, once the bridge is wired into a command ([#10](https://github.com/daveremy/stream2worlds/issues/10)); today `s2w watch -` only stores the lines. It also keeps the engine seam from being designed from a single case.
- **`WorldEvent`, `NaturalKey` and `AttrValue` moved to `s2w-model`** (a dated amendment to [decision 0005](docs/decisions/0005-pure-fold.md)). Engines see only the payload and mint natural keys; they never see a `World`, so the claim types belong to the model, not the core.
- **In-crate fitness functions, first slice: the module-size checker** ([#44](https://github.com/daveremy/stream2worlds/issues/44), a dated amendment to [decision 0001](docs/decisions/0001-workspace-layers.md)). `cargo xtask check` walks every non-test target with `syn`, counts non-test lines per module, refuses `#[path]` and `include!`-family macros, and cross-checks rustc dep-info so a compiled file the walker missed is reported. Cap 400, report-only for now; growth of the exemption list against `origin/main` blocks even in report-only mode, unless a `Baseline-growth: s2w#<N>` commit trailer authorizes it. Over the cap today: `s2w_log` (564) and `s2w_app::query::view` (420), both queued for [#66](https://github.com/daveremy/stream2worlds/issues/66).
- **Research 0007: decision models for System 1 and System 2** ([research](research/0007-decision-models.md)). Jev and the at least a dozen open models that speak its `/v1/systemone` format, read latency first: only Blink-tiny fits per-event at 1,000 events/s; the text-reading classifiers are sampled or asynchronous rungs. Routing beyond latency (cascades, learned routers, bandits over engines) and a ranked spike shortlist. Refreshed weekly (lifeos#1121).

**Learned**
- **Codex was walled mid-sprint on both accounts; Claude Opus 5.5 implemented both PRs.** Dave made Opus 5.5 a peer implementer alongside Codex astra rather than a fallback. The two-independent-reviewer rule held as it did in Sprints 59 and 60.
- **A Codex run left a truncated file on disk** — `module_size.rs` was 1 byte after the run. Clippy caught it, and the file was rebuilt byte-exact from the run log before review. The build gates, not the author, are what noticed.
- **The deepseek reviewer fails on prompts over about 30–40 KB.** Split the diff; a review that never ran is not a review.

**Changed course**
- **Persist verdicts, then embeddings, then measure H** ([#63](https://github.com/daveremy/stream2worlds/issues/63) → [#64](https://github.com/daveremy/stream2worlds/issues/64) → [#56](https://github.com/daveremy/stream2worlds/issues/56)). Local embeddings were the planned second engine; they now wait on the verdict log, because a model file is an input the log does not capture. H is measured only once embeddings are part of it, as the contract's B1 and [decision 0010](docs/decisions/0010-gate3-h-arm.md) define the arm.
- **Gate 3's obfuscated stream folds `wiki` into the title and revision hash domains** ([#17](https://github.com/daveremy/stream2worlds/issues/17), Dave). Cross-wiki composite-key discovery is reported as its own unfloored sub-metric rather than through the floored hash domains. The contract's B2 and B3 text stands as signed; the ruling is recorded there as a dated pointer note.
- **The scale fitness function's scope was cut** ([#32](https://github.com/daveremy/stream2worlds/issues/32), accepted).
- **System 1 as a learning layer is now a thesis, not a row in a table** ([#71](https://github.com/daveremy/stream2worlds/issues/71)): an engine adapter, a router over judgment kind and latency, and System 2 feedback into System 1. Dave: differentiating. Research 0007's deferred implications land there.

**Next**
- The verdict store in two PRs ([#63](https://github.com/daveremy/stream2worlds/issues/63)), then the local embeddings engine ([#64](https://github.com/daveremy/stream2worlds/issues/64)); `watch` hardening ([#39](https://github.com/daveremy/stream2worlds/issues/39)); wiring the bridge into a serving command so a live stream reaches the query API and MCP ([#10](https://github.com/daveremy/stream2worlds/issues/10)); the bridge follow-ups from review ([#74](https://github.com/daveremy/stream2worlds/issues/74)). Also filed: operator-supplied domain context ([#61](https://github.com/daveremy/stream2worlds/issues/61)), the remaining fitness-function slices ([#65](https://github.com/daveremy/stream2worlds/issues/65)–[#69](https://github.com/daveremy/stream2worlds/issues/69)), and the lifeos-side tooling epic ([#62](https://github.com/daveremy/stream2worlds/issues/62)).

---

## Sprint 60 — sources become adapters and presets (2026-09-27, 13:00–15:00)

Wikipedia stopped being special. The single `Source` trait research 0006 built the group-commit
API for now has three real transports behind it, and Wikipedia is one named configuration of one
of them, not its own module.

**Shipped**
- **Kafka, by explicit partition assignment** ([#7](https://github.com/daveremy/stream2worlds/issues/7), [decision 0007](docs/decisions/0007-kafka-client.md)). `s2w watch kafka://broker/topic` reads the topic's partitions from cluster metadata, resolves one start offset per partition, and runs one fetch loop per partition — never a consumer group, never an offset commit. Each partition is its own log source with its own stored offset; a resume offset deleted by retention or past the partition's end is a loud, fatal error, never a silent skip. A record is stored as a byte-deterministic JSON envelope so the log's content-hash dedupe collapses redeliveries but never distinct records.
- **The registry resolves `s2w watch <uri>` by scheme** ([#49](https://github.com/daveremy/stream2worlds/issues/49)): `kafka://`, `sse://`/`https://`/`http://`, `-` for stdin, or an exact preset name. `s2w-app` no longer has a line of per-source code — `WatchWikipediaArgs`, `WIKIPEDIA_SOURCE` and `watch_wikipedia` are gone, replaced by one `s2w_app::watch(WatchArgs)` over any `Source`.
- **The SSE transport generalized; Wikipedia became a preset over it** ([decision 0008](docs/decisions/0008-generic-sse-adapter.md), a dated amendment to [decision 0003](docs/decisions/0003-wikipedia-sse-client.md)). `sse/{mod,connect,frame}.rs` carry the connection, backpressure and reconnect-backoff logic every SSE stream shares; `sse/dialect.rs`'s `SseDialect` trait carries what only one stream knows — how an `id:` becomes a cursor, how to ask for a start time, which frames to keep. `Wikimedia` (now under `presets/`) implements the existing cursor-arbitration and canary/`examplewiki` filtering; `Opaque` is the default for a bare `sse://`/`https://`/`http://` target: the `id:` verbatim as the cursor, no `--since` support, every payload kept. A frame with no `id:` cannot be resumed from, so three in a row force a reconnect — forever, not a crash or a hang, because the transport cannot know whether an arbitrary stream was ever meant to carry ids.
- **stdin NDJSON** joined the same seam: one raw-line-per-event adapter, no `--since` support, ending at end of input.
- **A read-only MCP server** ([#52](https://github.com/daveremy/stream2worlds/issues/52), [decision 0009](docs/decisions/0009-mcp-server.md)). `s2w mcp` serves five tools over stdio (`world_view`, `world_diff`, `entity_history`, `branches`, `time`) that return the same JSON bytes as the HTTP query routes, because both now call the same `QueryState` methods. Every tool is annotated read-only; the world is empty until the live bridge (#51) lands.
- **`--json` on the CLI** ([#53](https://github.com/daveremy/stream2worlds/issues/53)): `--version`, `--help` and top-level errors print JSON with `--json`. `watch --json` is deferred to [#79](https://github.com/daveremy/stream2worlds/issues/79) (NDJSON progress design, dated exception recorded by #39); `s2w mcp` refuses extra arguments so nothing but MCP messages ever reaches its stdout.
- **What the gate-3 heuristics arm H contains, decided** ([#4](https://github.com/daveremy/stream2worlds/issues/4), [decision 0010](docs/decisions/0010-gate3-h-arm.md)): research 0002's seven-stage design, with Rebmann, Rehse and van der Aa (BPM 2022) as a component of H rather than a fourth arm. Measuring H-min is [#56](https://github.com/daveremy/stream2worlds/issues/56). The signed evaluation contract gets a dated pointer note, not an in-place edit.
- **Generic SSE keeps distinct events distinct.** The `Opaque` dialect stores `{"data","id"}` as a byte-deterministic envelope, so two events with different ids and identical data no longer collapse under the log's dedupe; the `wikipedia` preset still stores raw `data:` bytes, so logs from Sprint 59 resume unchanged (checked: 268 → 908 events, contiguous).
- **`sse/mod.rs` split to stay under the 400-line cap** ([#44](https://github.com/daveremy/stream2worlds/issues/44)): the HTTP connection, request-building and backoff moved to `sse/connect.rs`; `mod.rs` keeps the `Source` impl and the read loop.

**Learned**
- **Codex astra was walled on both ChatGPT accounts for the first hour.** The Kafka and adapter chunks were implemented on Claude Opus instead (Dave-approved fallback); astra implemented the MCP server and `--json` after its reset and reviewed the adapter PR. Its four review rounds found five real bugs (two Kafka source-id collisions, an SSE source-id collision, a timestamp-fallback skip race, same-data SSE events collapsing). The two-independent-reviewer rule held regardless of who wrote the code.
- **"Kafka never emits `Skipped`" needed a new error variant, not a workaround.** Kafka's fetch errors are always retryable from the same offset, never a decode failure, so they needed their own non-fatal `SourceError::Retrying` rather than overloading `Skipped`, which now means only "a malformed frame, safe to drop."
- **A dialect that assumes JSON is the wrong shape for a *generic* transport.** The plan's first `SseDialect::is_filtered(&Value) -> bool` baked in a JSON assumption a bare `https://` target does not share. It became `accept(&str) -> Result<bool, String>`: the dialect decides whether and how to parse, and a parse failure is a reported `Skipped`, not a panic.

**Changed course**
- **None.** Both plan-review rounds (fable + opus, `full` mode's 2-round cap) converged on APPROVE with fixes folded in before implementation, rather than a course change mid-build.

**Next**
- The live bridge from the log into query state ([#51](https://github.com/daveremy/stream2worlds/issues/51), plan reviewed), in-crate fitness functions ([#44](https://github.com/daveremy/stream2worlds/issues/44), plan written), `watch` hardening and `watch --json` ([#39](https://github.com/daveremy/stream2worlds/issues/39)), measuring H-min ([#56](https://github.com/daveremy/stream2worlds/issues/56)), and the evidence view ([#10](https://github.com/daveremy/stream2worlds/issues/10)).

---

## Sprint 59 — the world becomes queryable (2026-09-27, 11:00–13:00)

Five merges in two hours. The sprint opened with a stored log and a source, and closed with a command that runs for hours, a fold that replays byte-for-byte, and an HTTP surface over the folded world.

**Shipped**
- **`s2w watch wikipedia` is a real command** ([#29](https://github.com/daveremy/stream2worlds/issues/29), [#25](https://github.com/daveremy/stream2worlds/issues/25)). It resumes from the stored cursor, replays history with `--since` into a fresh log, and the log collapses redelivered events. The live proof: 47,282 real edits over 38.5 minutes, 0 duplicates, 0 gaps across a restart, and exactly 300 forced redeliveries collapsed.
- **The pure fold, with golden replay** ([#9](https://github.com/daveremy/stream2worlds/issues/9), [decision 0005](docs/decisions/0005-pure-fold.md)). Events in, world out, no I/O, clock or randomness. An entity id is never reused: a merge aliases, a revoke splits. Relationships into an entity past an in-degree of 10^4 become an attribute plus counters, so one hub page cannot swamp the world. `cargo xtask check` folds the golden log twice and from every prefix save/reload and requires byte-identical output.
- **A world query API in `s2w-app`** ([#36](https://github.com/daveremy/stream2worlds/issues/36), [decision 0006](docs/decisions/0006-world-query-api.md)). `/world?at=&branch=&lod=&focus=&hops=` returns the world at a fold offset in the d3 shape; SSE deltas arrive one per offset so `Last-Event-ID` resume is unambiguous; `/branches`, `/diff`, `/entity/:id/history` and `/time` complete the contract. Only the actual world is served: another `branch` is `501 branch_not_yet`, `lod=cluster` is `501 lod_not_yet`. A hub's own relationship into another hub shows at `lod=entity` too ([#42](https://github.com/daveremy/stream2worlds/issues/42)).
- **The slice-1 scale envelope, decided** ([#37](https://github.com/daveremy/stream2worlds/issues/37), [decision 0004](docs/decisions/0004-scale-envelope.md), Dave approved). One process on a 4-core, 16 GB laptop: 1,000 events/s, 10^6 live entities in 1 GB, 20 forks in under 100 ms. `synchronous=FULL` everywhere; throughput comes from group commit, not from a weaker durability setting. Not a distributed system.
- **Research 0005 (3D exploration) and 0006 (scaling)** ([research](research/)), both with every design implication dispositioned. 0006 found where `s2w` breaks first as a stream grows and the cheapest step past each wall; the envelope above and the hub cap are its first adopted implications.
- **The README now calls this a research project**, pre-alpha, with the gates as pre-registered questions that can fail. A hosted direction is filed for later: a dedicated machine per user, log and snapshots in object storage ([#35](https://github.com/daveremy/stream2worlds/issues/35)).
- **Cold builds about 9% faster** with `[profile.dev] debug = 1` ([#41](https://github.com/daveremy/stream2worlds/issues/41)): line tables stay, variable and type debuginfo goes.

**Learned**
- **Codex ran out by 11:15 on both accounts.** Four legs were implemented on Claude Opus instead. The two-independent-reviewers rule held; what changed was who wrote the code.
- **Two plan reviews hit the two-round cap.** Both times the right move was to apply the small fixes the reviewers had converged on and proceed, not to run a third round.
- **`s2w-app` is Wikipedia-shaped.** Wiring the first source straight into the app was fast, but the wiring knows it is Wikipedia. The second source exposes that as a layering finding, not a style nit.

**Changed course**
- **Every source goes behind one `Source` trait and `s2w watch <source-uri>`** ([#7](https://github.com/daveremy/stream2worlds/issues/7), in progress). The group-commit batch append API from research 0006 is already built for it; leg C moves Wikipedia, Kafka and stdin behind the same seam.
- **Fitness functions move into the crates** ([#44](https://github.com/daveremy/stream2worlds/issues/44)): small modules, small functions, visible public APIs, tests that bite, checked next to the code they judge rather than only from `xtask`.

**Next**
- #7 leg C (Kafka and stdin behind the `Source` trait), #44 (in-crate fitness functions), and the evidence view ([#10](https://github.com/daveremy/stream2worlds/issues/10)), now unblocked by the query API.

---

## Sprint 58 — first live stream (2026-09-27, 09:00–11:00)

The first sprint in the main loop, with Stream2Worlds as its full focus. It ended with live Wikipedia edits landing in a durable log: the first time `s2w` code touched a real stream.

**Shipped**
- **Live Wikipedia edits land in a durable log.** The Wikipedia EventStreams source ([#6](https://github.com/daveremy/stream2worlds/issues/6)) and the append-only event log ([#8](https://github.com/daveremy/stream2worlds/issues/8)) merged within twenty minutes of each other. The sprint demo streamed 561 live edits (1.5 MB) into the log in 30 seconds; a restart found them all and appended 310 more. The command that wires the two together, `s2w watch wikipedia`, is next.
- **An event log that cannot be rewritten.** Each append saves the event and advances its source cursor in one SQLite transaction, so a failed append means "not stored" and a retry is safe. Triggers refuse UPDATE, DELETE and INSERT OR REPLACE. A crash test kills the writer mid-batch and checks nothing is half-written. An in-memory implementation passes the same test suite, so the seam has two implementations from the start ([decision 0002](docs/decisions/0002-event-log-storage.md)).
- **A source that survives Wikimedia's disconnects.** Wikimedia drops every connection within 15 minutes. The source reconnects with the exact `Last-Event-ID` it last parsed, never advances past a half-received frame, and drops test-wiki and canary events ([decision 0003](docs/decisions/0003-wikipedia-sse-client.md)).
- **A licence and security-advisory gate.** CI fails when a dependency's licence is not on the permissive list, or when it has a RustSec advisory ([#16](https://github.com/daveremy/stream2worlds/issues/16)).

**Learned**
- **Independent reviewers earn their keep.** On the Wikipedia source, round 2 found that a fix for untestable assertions still raced a background task (opus), and that the test-wiki filter checked a field the real stream does not use: 1,615 of 1,615 recorded frames carry `wiki_id`, not `database` (Fable). Neither reviewer wrote the code.
- **The TLS stack widened the licence list.** `reqwest` with `rustls` pulls in more than 20 crates under ISC, BSD-3-Clause and Unicode-3.0. All are permissive, so the gate now allows them by name instead of by per-crate exception.
- **A long-lived branch can go silent in CI.** The source's PR drifted behind main and GitHub queued no check runs at all, which reads as "not started yet". The fix was merging main, which surfaced the licence finding above.

**Changed course**
- **The event log starts on SQLite.** The first design, our own segment files plus `redb` (research 0003's first choice), was blocked at plan review twice on crash-atomicity gaps between two stores. SQLite in WAL mode, the runner-up, removes all four findings by construction. The custom format waits for a measured need ([#19](https://github.com/daveremy/stream2worlds/issues/19)).
- **Deduplication on resume belongs to the log.** Resuming from a timestamp can redeliver an event, so dedup by event id moves to the log layer ([#25](https://github.com/daveremy/stream2worlds/issues/25)).
- **Implementation moved to the most capable coding model.** Dave: *"i want code to be high, high quality."* From the next leg, Codex `gpt-6-astra` writes the code, every review round has two independent reviewers that are never the model that wrote it, and the pure core and every seam run the full workflow with a design consult.
- **The explorer will be 3D.** Research 0005 settled on three.js for a world explorer ([#21](https://github.com/daveremy/stream2worlds/issues/21)), with the web view reading the world through the same query API as MCP.

**Next**
- `s2w watch wikipedia` ([#29](https://github.com/daveremy/stream2worlds/issues/29)): the source wired into the log behind a command, resuming from the stored cursor; then the fold with golden replay ([#9](https://github.com/daveremy/stream2worlds/issues/9)) and the evidence view ([#10](https://github.com/daveremy/stream2worlds/issues/10)).

---

## Foundation — pair mode (2026-09-27, before 09:00)

Dave and karpathy built the base the rest stands on, working together live before the project entered the sprint loop.

**Shipped**
- **Gate 1: the evaluation contract, signed** ([contract](docs/evaluation-contract.md)). It says how we will know whether `s2w` works, written before any code: the forecast question, which edits count, the baselines, and the pass thresholds. Five Codex review rounds took it from 17 findings to "sign".
- **Gate 2's skeleton, reviewed and approved.** Nine crates whose boundaries are the architecture, plus `cargo xtask check`, which fails the build when a crate reaches across a layer, adds an unlisted dependency, or drops its `AGENTS.md`. Four review rounds ([decision 0001](docs/decisions/0001-workspace-layers.md)).
- **Four research notes** ([research](research/)): prior art, structure discovery without an LLM, the Rust substrate, and a live revert pilot. Every design implication is adopted or filed as an issue.
- **A roadmap you can follow:** one milestone and one epic per gate.

**Learned**
- **The forecast question is well posed.** A 30-minute pilot on English Wikipedia: 1,854 eligible edits, 3.8% reverted within 30 minutes, and Wikimedia's own revert-risk model at ROC AUC 0.888 on that question, but with raw scores far above the base rate, so it must be recalibrated ([0004](research/0004-revert-pilot.md)).
- **The closest prior art is Zep's Graphiti**, which puts an LLM on every event. `s2w`'s claim is the combination: the LLM never touches an event, the world replays from its log, and every forecast is graded ([0001](research/0001-prior-art.md)).
- **The heuristics arm may be very strong.** Structure discovery without an LLM might score above 0.90 on Wikipedia, which would leave System 2 no room to win by gate 3's margin. So we build and measure the heuristics first ([0002](research/0002-structure-without-llm.md), [#4](https://github.com/daveremy/stream2worlds/issues/4)).

**Changed course**
- **No text-scanning ratchet.** The first skeleton counted `unwrap` and `#[allow]` in source text. Review kept finding ways around it, so it was replaced by compiler lints set to `forbid`, which cannot be bypassed from inside a file.
- **"Possible worlds" gets a definition.** In databases the phrase means uncertainty about the present; ours means sampled futures. The README now defines it once and borrows the database field's Monte Carlo semantics.

## Design (before the repository)

The idea went through the [design document](docs/design/stream2worlds-design.html) and two rounds of critic reviews from Codex and Claude ([reviews](docs/reviews/)) before the first commit. It drew on its predecessors: PredictStream, which proved that forecasting on a stream works and that an LLM on the event path is too slow, and three Rust prototypes (worldcraft, timely_worlds, strema) that explored world models over streams.
