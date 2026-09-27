//! Stream sources. Adapters (`kafka`, `sse`, `stdin`) are transports that implement
//! [`source::Source`]; presets (`wikipedia`) are data over an adapter; [`registry::resolve`]
//! maps a `s2w watch` URI to one. None joins a consumer group or commits offsets.

pub mod kafka;
pub mod ndjson;
pub mod presets;
pub mod registry;
pub mod source;
pub mod sse;
pub mod stdin;
