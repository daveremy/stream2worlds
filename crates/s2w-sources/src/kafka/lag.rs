//! Per-partition lag for the Kafka adapter (s2w#168): one [`Watermark`] per assigned
//! partition, advanced by the fetch tasks (the head) and the adapter's stream (delivery).

use std::collections::BTreeMap;
use std::sync::Arc;

use s2w_model::{RawEvent, SourceId};

use super::KafkaEvent;
use crate::source::SourceError;
use crate::watermark::{Watermark, Watermarks};

/// Converts `event` with [`super::raw`], then marks it delivered on its partition's watermark.
pub(super) fn delivered(
    sources: &BTreeMap<i32, SourceId>,
    marks: &BTreeMap<i32, Arc<Watermark>>,
    event: KafkaEvent,
) -> Result<RawEvent, SourceError> {
    let (partition, offset) = (event.partition, event.offset);
    let raw = super::raw(sources, event)?;
    if let Some(mark) = marks.get(&partition) {
        mark.delivered(offset);
    }
    Ok(raw)
}

/// One watermark per assigned partition, keyed by partition for the fetch tasks and the
/// stream, and the same watermarks labelled `p<N>` for the app's status line (s2w#168).
pub(super) fn partition_watermarks(
    sources: &BTreeMap<i32, SourceId>,
) -> (BTreeMap<i32, Arc<Watermark>>, Watermarks) {
    let marks: BTreeMap<i32, Arc<Watermark>> = sources
        .keys()
        .map(|&partition| (partition, Arc::new(Watermark::unknown())))
        .collect();
    let labelled = sources
        .iter()
        .filter_map(|(partition, source)| {
            marks
                .get(partition)
                .map(|mark| (source.clone(), format!("p{partition}"), Arc::clone(mark)))
        })
        .collect();
    (marks, Watermarks::tracked(labelled))
}
