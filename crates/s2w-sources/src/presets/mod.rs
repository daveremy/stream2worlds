//! Presets: named real streams, each an adapter plus its configuration, as data.

pub(crate) mod wikimedia;

use std::sync::Arc;

use crate::source::Source;
use crate::sse::{SseConfig, SseSource, USER_AGENT};

/// Builds a preset's source. `wiki` is only meaningful to presets that read one wiki's stream
/// (today, just `wikipedia`); a preset that ignores it must say so at the call site, not here —
/// see `registry::resolve`'s usage check.
pub(crate) type PresetFn = fn(Option<&str>) -> Box<dyn Source>;

/// Every preset, by the exact (case-sensitive) name `s2w watch` accepts.
pub(crate) const PRESETS: &[(&str, PresetFn)] = &[("wikipedia", wikipedia)];

/// The preset named `name`, if there is one. `wiki` restricts ingestion to one wiki (e.g.
/// `enwiki`) where the preset supports it.
#[must_use]
pub(crate) fn preset(name: &str, wiki: Option<&str>) -> Option<Box<dyn Source>> {
    PRESETS
        .iter()
        .find(|(preset, _)| *preset == name)
        .map(|(_, build)| build(wiki))
}

/// Wikipedia page changes: Wikimedia EventStreams over the `sse` adapter.
fn wikipedia(wiki: Option<&str>) -> Box<dyn Source> {
    let dialect = match wiki {
        Some(wiki) => wikimedia::Wikimedia::filtered(wiki.to_owned()),
        None => wikimedia::Wikimedia::new(),
    };
    Box::new(SseSource::new(SseConfig {
        name: "wikipedia",
        url: wikimedia::ENDPOINT.to_owned(),
        dialect: Arc::new(dialect),
        source_id: wikimedia::SOURCE_ID.to_owned(),
        user_agent: USER_AGENT,
    }))
}
