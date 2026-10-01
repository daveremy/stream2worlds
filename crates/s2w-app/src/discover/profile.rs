//! Profiling one window for auto-apply: version-1 mappings only (decision 0022, s2w#245 PR 4).

use s2w_discover::Discovery;
use s2w_model::{MAPPING_VERSION, StreamMapping};

use super::Window;
use crate::Reporter;

/// The window's mapping when the profiler proposes a version-1 one; otherwise a note and `None`.
/// A version-2 mapping (links, decision 0027) is never auto-applied (decision 0022, s2w#245).
pub(super) fn version_1(
    window: &Window,
    profiler: &s2w_discover::Config,
    reporter: &mut dyn Reporter,
) -> Option<StreamMapping> {
    let source = window.source.as_str();
    let payloads: Vec<&[u8]> = window.payloads.iter().map(Vec::as_slice).collect();
    match s2w_discover::discover(&payloads, profiler).1 {
        Discovery::Mapping(mapping) if mapping.version == MAPPING_VERSION => Some(mapping),
        Discovery::Mapping(mapping) => {
            reporter.note(&format!(
                "discover: {source}: proposed a version-{} mapping (links); auto-apply files version {MAPPING_VERSION} only, nothing written",
                mapping.version
            ));
            None
        }
        Discovery::Abstain(reason) => {
            reporter.note(&format!(
                "discover: {source}: abstained ({reason}) over {} events",
                payloads.len()
            ));
            None
        }
    }
}
