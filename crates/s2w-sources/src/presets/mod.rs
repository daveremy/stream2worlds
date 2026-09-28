//! Presets: named real streams, each an adapter plus its configuration, as data.

use std::sync::Arc;

use crate::filter::FieldFilter;
use crate::source::Source;
use crate::sse::{FilteredDialect, SinceQueryParam, SseConfig, SseSource, USER_AGENT};

/// Every preset as the exact name accepted by `s2w watch`, its URL, its stored source id, and
/// its default `--filter` specs (each `// vocabulary: allow` — stream-specific literal data,
/// consumed generically; decision 0018). One spec per line so the marker applies per string,
/// not to the whole slice literal.
pub(crate) const PRESETS: &[(&str, &str, &str, &[&str])] = &[(
    "wikipedia",                                                       // vocabulary: allow
    "https://stream.wikimedia.org/v2/stream/mediawiki.page_change.v1", // vocabulary: allow
    "wikipedia.page_change",                                           // vocabulary: allow
    &[
        "meta.domain!=canary",   // vocabulary: allow
        "wiki_id!=examplewiki",  // vocabulary: allow
        "database!=examplewiki", // vocabulary: allow
    ],
)];

/// The preset named `name`'s default filter specs, if there is one.
#[must_use]
pub(crate) fn preset_filters(name: &str) -> &'static [&'static str] {
    PRESETS
        .iter()
        .find(|(preset, ..)| *preset == name)
        .map_or(&[], |(_, _, _, filters)| filters)
}

/// The preset named `name`, if there is one. `filters` are ANDed with the preset's own default
/// filters (a preset's default drop cannot be loosened by a caller-supplied filter).
///
/// # Errors
///
/// `Err` names the preset's own default spec that failed to parse — `every_preset_filter_spec_parses`
/// below proves this never happens for a committed `PRESETS` entry, but a future edit to that
/// data is caught loudly here rather than by an `unwrap`/`expect` in this function.
pub(crate) fn preset(
    name: &str,
    filters: &[FieldFilter],
) -> Result<Option<Box<dyn Source>>, String> {
    let Some((name, url, source_id, _)) = PRESETS.iter().find(|(preset, ..)| *preset == name)
    else {
        return Ok(None);
    };
    let mut merged: Vec<FieldFilter> = Vec::new();
    for spec in preset_filters(name) {
        merged.push(FieldFilter::parse(spec).map_err(|e| {
            format!("preset {name:?}'s own default filter {spec:?} does not parse: {e}")
        })?);
    }
    merged.extend(filters.iter().cloned());
    Ok(Some(Box::new(SseSource::new(SseConfig {
        name,
        url: (*url).to_owned(),
        dialect: Arc::new(FilteredDialect::new(
            Arc::new(SinceQueryParam { param: "since" }),
            merged,
        )),
        source_id: (*source_id).to_owned(),
        user_agent: USER_AGENT,
    })) as Box<dyn Source>))
}

#[cfg(test)]
mod tests {
    use crate::filter::FieldFilter;

    use super::{PRESETS, preset_filters};

    #[test]
    fn every_preset_filter_spec_parses() {
        for (name, _, _, specs) in PRESETS {
            for spec in *specs {
                assert!(
                    FieldFilter::parse(spec).is_ok(),
                    "preset {name:?}: filter spec {spec:?} does not parse"
                );
            }
        }
    }

    #[test]
    fn wikipedia_default_filters_drop_canary_and_examplewiki() {
        let specs = preset_filters("wikipedia");
        assert!(specs.contains(&"meta.domain!=canary"));
        assert!(specs.contains(&"wiki_id!=examplewiki"));
        assert!(specs.contains(&"database!=examplewiki"));
    }

    #[test]
    fn unknown_preset_has_no_default_filters() {
        assert!(preset_filters("not-a-preset").is_empty());
    }
}
