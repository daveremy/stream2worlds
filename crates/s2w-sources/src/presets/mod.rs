//! Presets: named real streams, each an adapter plus its configuration, as data.

pub mod wikimedia;

use std::sync::Arc;

use crate::source::Source;
use crate::sse::{SseConfig, SseSource, USER_AGENT};

/// Builds a preset's source.
pub type PresetFn = fn() -> Box<dyn Source>;

/// Every preset, by the exact (case-sensitive) name `s2w watch` accepts.
pub const PRESETS: &[(&str, PresetFn)] = &[("wikipedia", wikipedia)];

/// The preset named `name`, if there is one.
#[must_use]
pub fn preset(name: &str) -> Option<Box<dyn Source>> {
    PRESETS
        .iter()
        .find(|(preset, _)| *preset == name)
        .map(|(_, build)| build())
}

/// Wikipedia page changes: Wikimedia EventStreams over the `sse` adapter.
fn wikipedia() -> Box<dyn Source> {
    Box::new(SseSource::new(SseConfig {
        name: "wikipedia",
        url: wikimedia::ENDPOINT.to_owned(),
        dialect: Arc::new(wikimedia::Wikimedia),
        source_id: wikimedia::SOURCE_ID.to_owned(),
        user_agent: USER_AGENT,
    }))
}
