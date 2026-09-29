//! `backfill_memory` with mimalloc as the global allocator (s2w#220): the same variants and
//! `result {json}` lines, to compare the bridge's whole-process peak with glibc malloc's.
//! Ignored; run with
//! `cargo test --release -p s2w-app --test backfill_memory_mimalloc -- --ignored --nocapture`.
//! Every Rust allocation goes to mimalloc; SQLite's C heap stays on glibc malloc. The
//! measurement itself lives in `backfill_memory.rs` (its module doc).

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[path = "backfill_memory.rs"]
mod backfill_memory;
