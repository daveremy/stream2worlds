//! The Callgrind benchmarks `cargo xtask scale` gates, and the baseline key that judges each:
//! the fold of both event supplies (s2w#32, s2w#174) and System 1's parse (s2w#166).
use super::{Baseline, Supply};

/// One gated instruction count: the fold of an event supply, or the parse of the recorded
/// fixture's raw events into claims by `MappingEngine` with the committed linked mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IrBench {
    Fold(Supply),
    Parse,
}

impl IrBench {
    pub(crate) const ALL: [Self; 3] = [
        Self::Fold(Supply::Synthetic),
        Self::Fold(Supply::Recorded),
        Self::Parse,
    ];

    /// The name every report line uses, so the three numbers are never confused.
    pub(crate) fn name(self) -> String {
        match self {
            Self::Fold(supply) => supply.name("fold Ir"),
            Self::Parse => "parse Ir (recorded)".to_owned(),
        }
    }

    /// The baseline table holding this benchmark's gated number.
    pub(crate) fn table(self) -> &'static str {
        match self {
            Self::Fold(Supply::Synthetic) => "[ir]",
            Self::Fold(Supply::Recorded) => "[ir.recorded]",
            Self::Parse => "[parse]",
        }
    }

    /// The gated key within [`Self::table`].
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Fold(_) => "fold_ir_per_event",
            Self::Parse => "parse_ir_per_event",
        }
    }

    /// The gungraun summary of this `scale_ir` bench: `<group>/<function>.<bench id>`.
    pub(crate) fn summary(self) -> &'static str {
        match self {
            Self::Fold(Supply::Synthetic) => "scale/fold_ir_per_event.events/summary.json",
            Self::Fold(Supply::Recorded) => "scale/fold_ir_per_event_recorded.fixture/summary.json",
            Self::Parse => "scale/parse_ir_per_event.fixture/summary.json",
        }
    }

    /// What to make cheaper when the number regresses.
    pub(crate) fn remedy(self) -> &'static str {
        match self {
            Self::Fold(_) => "make the fold cheaper",
            Self::Parse => "make the engine's evaluate cheaper",
        }
    }
}

impl Baseline {
    /// `(instructions per event, events)` for `bench`.
    pub(crate) fn ir_gate(&self, bench: IrBench) -> (u64, u64) {
        match bench {
            IrBench::Fold(Supply::Synthetic) => (self.ir.fold_ir_per_event, self.ir.events),
            IrBench::Fold(Supply::Recorded) => {
                (self.ir.recorded.fold_ir_per_event, self.ir.recorded.events)
            }
            IrBench::Parse => (self.parse.parse_ir_per_event, self.parse.events),
        }
    }
}
