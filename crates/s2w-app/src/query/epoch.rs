//! [`Epoch`]: which history an offset belongs to.

use serde::Serialize;

/// Which history an offset belongs to: the serving registry's feed fingerprint (decision 0023,
/// amended by PR 2b-i of s2w#184). Two timelines with the same epoch are the same deterministic
/// fold of the same log, so `(epoch, offset)` names one world; the same offset under a different
/// epoch may name a different world and answers `stale_epoch` (410).
///
/// Serialized as 16 lowercase hex digits (a string: 64-bit values do not survive JS numbers).
/// `0` is reserved for "no serving registry" (a fresh [`super::Timeline::new`], the standalone
/// `s2w mcp` replay).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Epoch(pub u64);

impl std::fmt::Display for Epoch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

impl std::str::FromStr for Epoch {
    type Err = String;

    /// Exactly 16 hex digits, either case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 16 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("expected 16 hex digits".to_owned());
        }
        u64::from_str_radix(s, 16)
            .map(Self)
            .map_err(|e| e.to_string())
    }
}

impl Serialize for Epoch {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
