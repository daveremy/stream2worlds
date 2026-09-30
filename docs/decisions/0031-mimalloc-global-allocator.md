# 0031: mimalloc is the `s2w` binary's global allocator

Date: 2026-09-30 · Status: accepted · Gate 2 · Issue #283 · Builds on [0026](0026-bounded-timeline-history.md)

## Decision

**The `s2w` binary allocates through mimalloc.** `crates/s2w/src/main.rs` sets
`#[global_allocator] static GLOBAL: mimalloc::MiMalloc`, with `mimalloc = "=0.1.52"` pinned in
`crates/s2w/Cargo.toml`. There is one binary, so the allocator covers every subcommand (`serve`,
`watch`, `mcp`, `proposals`, `presentation`, `dashboard`). No library crate sets an allocator.
The crate's default features are used, without `override`, so SQLite's C heap (rusqlite
`bundled`) stays on glibc malloc. The measurement target below makes the same split.

## Context

s2w#220 PR C (#280) added the `backfill_memory_mimalloc` target: `backfill_memory.rs` compiled
with mimalloc as its global allocator. At history cap 2 on `main` @ f6900a3 it measured
mimalloc 84 MiB lower on the main thread and 102 MiB lower on the blocking pool, against a
≥40 MiB bar for a swap (0026, the dated #220 PR C block). The ruling waited on s2w#282, which
settled the world size and the history cap (50,000) on 2026-09-30. This record re-measures on
that settled world and rules.

## Measured

`main` @ cbae35e, the recorded fixture cycled to 150,000 raw events (14,264,996 world events),
history cap 50,000, batch 250, release build. Shared hub host, load average 6-14, with another
Rust leg building alongside. The targets alternated round by round. Whole-process peak
(`VmHWM`), median [min-max] MiB:

| Topology | glibc malloc (`backfill_memory`) | glibc `+trim` | glibc `+arena2` | **mimalloc** (`backfill_memory_mimalloc`) | mimalloc − glibc |
|---|---|---|---|---|---|
| `bridge` (main thread), 3 runs | 773.5 [773.4-774.1] | | | **689.3 [686.9-690.0]** | −84 MiB |
| `bridge-run` (blocking pool, as `Bridge::run` in serve), 3 runs | 800.2 [795.9-800.4] | 792.9 [770.4-798.5] | 771.6 [771.5-798.1] | **694.6 [691.2-712.3]** | −106 MiB |
| `viewer` (one reader, 5 s tick), 2 runs | 1,121.9 [1,118.9-1,125.0] | | | **1,053.3 [1,044.4-1,062.3]** | −69 MiB |

`+trim` sets `MALLOC_TRIM_THRESHOLD_=131072` and `MALLOC_TOP_PAD_=0`, and `+arena2` sets
`MALLOC_ARENA_MAX=2`, on the child only (`ALLOCATORS` in `tests/backfill_memory.rs`).

Seven other runs were discarded, not averaged in: a second invocation ran alongside the first
batch, and two invocations of one target share its `CARGO_TARGET_TMPDIR` log directory, so
they deleted and repopulated each other's log mid-run (one failed with `Locked`). Every row
above is from invocations that ran alone on their target; the discarded runs were re-run
sequentially.

## Retention

Resident 2 s after the backfill's state is dropped (`rss_after_drop`), MiB, every run:

| Topology | glibc malloc | glibc `+trim` | glibc `+arena2` | mimalloc |
|---|---|---|---|---|
| `bridge-run` | 723.6, 743.9, 774.9 | 741.6, 734.4, 708.3 | 737.1, 736.4, 711.5 | 688.1, **35.4, 80.2** |
| `bridge` | 766.6, 766.7, 767.1 | | | 686.2, 686.8, 682.5 |
| `viewer` | 800.2, 801.6 | | | 691.6, 697.1 |

On the blocking pool, mimalloc returned memory to the OS in 2 of 3 runs, the same split as
#220 PR C (2 of 3 under 83 MiB). glibc kept at least 708 MiB resident in every run, with or
without the tunings, which matches the demo box holding 906 MiB with no world loaded. On the
main thread and under a viewer, neither allocator returned memory within 2 s. The return is
therefore bimodal and is not a guarantee. The ruling rests on the peak, which is lower on
every topology with no overlap between the ranges. The return is an additional benefit when
it happens.

**Ruling rule, applied.** Adopt only if (1) mimalloc's median peak is below glibc's on
`bridge`, `bridge-run` and `viewer`, with non-overlapping ranges, and (2) mimalloc's median
`rss_after_drop` on `bridge-run` is below glibc's, and no glibc tuning also meets both. (1)
holds on all three topologies. (2) holds: 80.2 MiB against 743.9 MiB. Neither tuning meets
(1): both `bridge-run` ranges (770-799 MiB) sit above mimalloc's worst run (712 MiB).

## Dependency cost

- **Crates:** `mimalloc` 0.1.52 and `libmimalloc-sys` 0.1.49, both MIT. Both were already in
  `Cargo.lock` as the s2w-app dev dependency behind the measurement target, so `cargo deny`
  and the lockfile gain no new package. The only new edge is `s2w → mimalloc` (normal).
- **C build:** `libmimalloc-sys` compiles mimalloc's C source through `cc`. Every build host
  already needs a C compiler for rusqlite `bundled`, so this adds no toolchain requirement.
  A clean release build of the two crates took 11.2 s on the loaded host.
- **Binary size:** the release `s2w` grew from 22,666,000 to 22,819,528 bytes (+150 KiB, +0.7%).
- **`unsafe`:** none. The static is safe code, so `unsafe_code = "forbid"` still holds.

## Alternatives considered

- **glibc with `malloc_trim` or arena tuning, as environment variables.** Measured above:
  neither closes the peak gap. Each would also be a `MALLOC_*` line that every host running
  `s2w serve` must carry, with no check that it is set.
- **glibc with `mallopt` / `malloc_trim` called in code.** Not measured. It needs libc FFI,
  which is `unsafe`, and the workspace forbids `unsafe_code`. The environment-variable
  equivalents above already failed to match mimalloc.
- **jemalloc.** Not measured. mimalloc cleared the bar with a dependency the workspace already
  locks, so there was no reason to add a second allocator to compare.
- **Keep glibc.** Rejected: it gives up about 85-100 MiB of peak on the load that decides the
  demo box's `MemoryMax` (0026, s2w#282).

## Limits, accepted

- **The asserted limits stay glibc numbers.** `SERVE_PEAK_LIMIT` (810 MiB) and
  `VIEWER_PEAK_LIMIT` (1,340 MiB) are asserted by the glibc `backfill_memory` default sweep,
  nightly. Neither memory target links the `s2w` crate, so this swap changes no test's
  number. mimalloc peaks lower on every topology measured here, so the glibc assertion is an
  upper bound on the shipped binary. Moving the assertion to the mimalloc target is a separate
  change.
- **The product "after" is the box, measured after deploy.** These runs measure the same
  `s2w-app` code under each allocator, not the `s2w` binary. Neither covers serve's snapshot
  encode, HTTP server or SSE.
- **No `MIMALLOC_*` tuning.** The measurement target refuses `MIMALLOC_*` variables. The box
  must not set any either, so the box runs the configuration measured here.

## Revisit when

A box measurement after deploy shows mimalloc resident above what `backfill_memory_mimalloc`
predicts, a mimalloc release changes its default purge behaviour, or SQLite's C heap becomes
a large share of serve's peak.

verify: grep -q '^static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;' crates/s2w/src/main.rs && grep -q '^mimalloc = "=0.1.52"' crates/s2w/Cargo.toml
