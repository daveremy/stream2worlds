//! The pure fold: events in, world out. No I/O, no async, no wall clock, no randomness, no HashMap iteration order. Time and randomness are passed in.
//!
//! [`fold_one`] takes a [`World`] and a [`WorldEvent`] and returns the next [`World`]. It is
//! total: an event it cannot apply is a documented no-op, never a panic or an error. The same
//! events always fold to the same bytes, and a world serialized mid-log resumes to the same
//! result as folding the whole log (decision 0005; `cargo xtask check` replays a golden log).
#![deny(clippy::print_stdout, clippy::print_stderr)]

mod event;
mod world;

pub use event::{AttrValue, EntityId, NaturalKey, WorldEvent};
pub use world::{
    DEFAULT_HUB_IN_DEGREE_CAP, EntityState, FOLD_VERSION, HubCounters, Relationship, World,
    WorldId, fold, fold_one,
};
