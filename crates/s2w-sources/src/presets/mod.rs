//! Presets: named real streams, each an adapter plus its configuration, as data.

use std::sync::Arc;

use crate::source::Source;
use crate::sse::{SinceQueryParam, SseConfig, SseSource, USER_AGENT};

/// Every preset as the exact name accepted by `s2w watch`, its URL, and stored source id.
pub(crate) const PRESETS: &[(&str, &str, &str)] = &[(
    "wikipedia",                                                       // vocabulary: allow
    "https://stream.wikimedia.org/v2/stream/mediawiki.page_change.v1", // vocabulary: allow
    "wikipedia.page_change",                                           // vocabulary: allow
)];

/// The preset named `name`, if there is one.
#[must_use]
pub(crate) fn preset(name: &str) -> Option<Box<dyn Source>> {
    PRESETS
        .iter()
        .find(|(preset, _, _)| *preset == name)
        .map(|(name, url, source_id)| {
            Box::new(SseSource::new(SseConfig {
                name,
                url: (*url).to_owned(),
                dialect: Arc::new(SinceQueryParam { param: "since" }),
                source_id: (*source_id).to_owned(),
                user_agent: USER_AGENT,
            })) as Box<dyn Source>
        })
}
