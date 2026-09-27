//! A tiny non-cryptographic hash shared by every adapter that derives a [`s2w_model::SourceId`]
//! from data richer than `SourceId`'s own alphabet (ASCII alnum, `.`, `-`, `_`) allows: broker
//! lists, full request URLs. Not a security boundary — only used so two distinct identities
//! never sanitize down to the same source id.

/// FNV-1a, 64-bit.
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// `bytes` as a 16-character lowercase hex string.
pub(crate) fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}
