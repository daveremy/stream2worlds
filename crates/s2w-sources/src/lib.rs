//! Stream sources. Adapters (`kafka`, `sse`, `stdin`) are transports that implement
//! [`source::Source`]; presets (`wikipedia`) are data over an adapter; [`registry::resolve`] <!-- vocabulary: allow -->
//! maps a `s2w watch` URI to one. None joins a consumer group or commits offsets.

mod hash;
mod kafka;
mod ndjson;
mod presets;
pub mod registry;
mod since;
pub mod source;
mod sse;
mod stdin;
