//! `discover_volume` with dhat as the global allocator (s2w#392): each fold child prints the
//! world's exact heap bytes (dhat's live bytes after the fold minus before it) in its JSON line.
//! `cargo xtask discover-volume` runs the `fresh` child and gates that figure against
//! `[discover_volume]` in `xtask/scale-baseline.toml`. By hand:
//! `S2W_DISCOVER_VOLUME_VARIANT=fresh cargo test --release -p s2w-app --test discover_volume_heap
//! -- --ignored --exact discover_volume::volume::fold_child --nocapture`.
//! Only `heap_bytes` is a measurement here: dhat's own bookkeeping inflates this target's
//! resident figures. The measurement itself lives in `discover_volume.rs` (its module doc).

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[path = "discover_volume.rs"]
mod discover_volume;
