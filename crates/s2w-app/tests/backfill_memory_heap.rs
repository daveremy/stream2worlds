//! `backfill_memory`'s `bridge` child with dhat as the global allocator (s2w#220): prints the
//! Rust heap's peak (`max_bytes`) beside the whole-process `VmHWM`, to split the bridge's
//! overhead into heap and non-heap. Ignored; run with
//! `cargo test --release -p s2w-app --test backfill_memory_heap -- --ignored --nocapture`.
//! Only its `max_bytes` is a measurement: dhat's own bookkeeping inflates this target's resident
//! figures. The measurement itself lives in `backfill_memory.rs` (its module doc).

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[path = "backfill_memory.rs"]
mod backfill_memory;
