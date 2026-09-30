//! The source-lag segment of the progress line (s2w#168): how many records each source has
//! that the app has not pulled off the stream yet, per partition. Records already pulled but
//! still waiting in the group-commit buffer count as read.
//!
//! A source whose protocol has no head position says so ("lag not reported"), and a partition
//! whose position is not known yet prints `?`: an absent number is never shown as zero.

use std::fmt::Write as _;

use s2w_sources::watermark::LagReading;

/// Partitions shown by name before the rest collapse into `+N more`.
const SHOWN: usize = 8;

/// Renders the lag segment, e.g. `lag p0 12, p1 0, p2 ?`.
///
/// `None` (the source reports no watermark) renders `lag not reported`. Up to [`SHOWN`]
/// partitions print in the adapter's order; past that, the [`SHOWN`] furthest behind print (unknown
/// last, ties in the adapter's order), then `+N more`.
pub(crate) fn render_lag(readings: Option<&[LagReading]>) -> String {
    let Some(readings) = readings else {
        return "lag not reported".to_owned();
    };
    if readings.is_empty() {
        return "lag ?".to_owned();
    }
    let mut shown: Vec<&LagReading> = readings.iter().collect();
    if shown.len() > SHOWN {
        // Stable sort: equal lags keep the adapter's order. `None` sorts after every number.
        shown.sort_by_key(|reading| std::cmp::Reverse(reading.behind));
        shown.truncate(SHOWN);
    }
    let mut line = "lag".to_owned();
    for (index, reading) in shown.iter().enumerate() {
        let separator = if index == 0 { " " } else { ", " };
        let _ = match reading.behind {
            Some(behind) => write!(line, "{separator}{} {behind}", reading.label),
            None => write!(line, "{separator}{} ?", reading.label),
        };
    }
    if readings.len() > SHOWN {
        let _ = write!(line, ", +{} more", readings.len() - SHOWN);
    }
    line
}

#[cfg(test)]
mod tests {
    use s2w_model::SourceId;
    use s2w_sources::watermark::LagReading;

    use super::render_lag;

    fn reading(partition: usize, behind: Option<u64>) -> LagReading {
        let name = format!("k.t.p{partition}");
        let source = match SourceId::new(name.as_str()) {
            Ok(source) => source,
            Err(error) => panic!("{name:?} should be a valid source id: {error}"),
        };
        LagReading {
            source,
            label: format!("p{partition}"),
            high_watermark: behind.map(|_| 100),
            behind,
        }
    }

    #[test]
    fn a_source_without_a_watermark_says_so_never_zero() {
        assert_eq!(render_lag(None), "lag not reported");
    }

    #[test]
    fn every_partition_prints_with_unknown_as_a_question_mark() {
        let readings = [reading(0, Some(12)), reading(1, Some(0)), reading(2, None)];
        assert_eq!(render_lag(Some(&readings)), "lag p0 12, p1 0, p2 ?");
    }

    #[test]
    fn past_eight_partitions_the_furthest_behind_print_then_a_count() {
        let readings: Vec<_> = (0..11)
            .map(|partition| {
                let behind = match partition {
                    3 => None,
                    9 => Some(500),
                    _ => Some(partition as u64),
                };
                reading(partition, behind)
            })
            .collect();
        assert_eq!(
            render_lag(Some(&readings)),
            "lag p9 500, p10 10, p8 8, p7 7, p6 6, p5 5, p4 4, p2 2, +3 more"
        );
    }

    #[test]
    fn a_tracked_source_with_no_partitions_prints_unknown() {
        assert_eq!(render_lag(Some(&[])), "lag ?");
    }
}
