//! FNV-1a 64: the one small, dependency-free, non-cryptographic hash the workspace uses for
//! checksums, fingerprints, digests and derived ids. Not a security boundary. Every value it
//! has produced is persisted somewhere (log content hashes, source ids, snapshot checksums,
//! fixture hashes), so the algorithm never changes.

/// A streaming FNV-1a 64 hasher.
#[derive(Clone, Copy, Debug)]
pub struct Fnv64(u64);

impl Default for Fnv64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Fnv64 {
    /// A hasher at the FNV-1a 64 offset basis.
    #[must_use]
    pub const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    /// Feeds `bytes`.
    pub fn write(&mut self, bytes: &[u8]) -> &mut Self {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
        self
    }

    /// Writes `bytes` preceded by their length, so adjacent fields cannot run together.
    pub fn write_field(&mut self, bytes: &[u8]) -> &mut Self {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        self.write(&len.to_le_bytes()).write(bytes)
    }

    /// The hash of everything written so far.
    #[must_use]
    pub const fn finish(&self) -> u64 {
        self.0
    }
}

/// FNV-1a 64 of `bytes`.
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    Fnv64::new().write(bytes).finish()
}

/// FNV-1a 64 of `bytes` as 16 lowercase hex digits.
#[must_use]
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}

/// Whether `value` is exactly 16 lowercase hex digits, the shape `fnv1a64_hex` produces.
#[must_use]
pub fn is_hex16(value: &str) -> bool {
    value.len() == 16
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
        assert_eq!(fnv1a64_hex(b"a"), "af63dc4c8601ec8c");
    }

    #[test]
    fn streaming_matches_one_shot_and_fields_are_length_prefixed() {
        assert_eq!(
            Fnv64::new().write(b"foo").write(b"bar").finish(),
            fnv1a64(b"foobar")
        );
        let ab_c = Fnv64::new().write_field(b"ab").write_field(b"c").finish();
        let a_bc = Fnv64::new().write_field(b"a").write_field(b"bc").finish();
        assert_ne!(ab_c, a_bc);
    }
}
