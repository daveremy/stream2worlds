//! The replicate key's three uses (contract B2.2): keyed value hashes, the field order and the
//! timestamp shift. Every output is `HMAC-SHA256(key, label, parts)`, so the same key and the same
//! inputs give the same bytes. Built on `sha2`, which xtask already depends on.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

/// Bytes of a value hash kept in the output (64 bits).
const WIDTH: usize = 8;

/// Seconds in the shift's range above its one-day floor: five 365-day years.
const SHIFT_RANGE: u64 = 5 * 365 * 86_400;

/// The shift's floor, so no replicate keeps its real dates.
const SHIFT_FLOOR: u64 = 86_400;

/// The replicate key and the run's collision register.
pub(super) struct Keyed {
    key: [u8; 32],
    /// Bytes of a value hash kept; [`WIDTH`] except in the collision test.
    width: usize,
    /// Every truncated hash handed out this run, with its full digest: two different inputs
    /// sharing a truncated hash is a collision, and the run fails.
    seen: BTreeMap<Vec<u8>, [u8; 32]>,
}

impl Keyed {
    pub(super) fn new(key: [u8; 32]) -> Self {
        Self {
            key,
            width: WIDTH,
            seen: BTreeMap::new(),
        }
    }

    /// A keyed hasher that keeps only `width` bytes, so a test can force a collision.
    #[cfg(test)]
    pub(super) fn narrow(key: [u8; 32], width: usize) -> Self {
        Self {
            key,
            width,
            seen: BTreeMap::new(),
        }
    }

    /// Lower-case hex sha256 of the key: names the key in the metadata, never reveals it.
    pub(super) fn fingerprint(&self) -> String {
        crate::sha256(&self.key)
    }

    fn prf(&self, label: &str, parts: &[&[u8]]) -> [u8; 32] {
        let mut message = Vec::new();
        for part in std::iter::once(label.as_bytes()).chain(parts.iter().copied()) {
            // Length-prefixed, so no choice of part bytes can make two inputs read the same.
            message.extend_from_slice(&(part.len() as u64).to_le_bytes());
            message.extend_from_slice(part);
        }
        hmac(&self.key, &message)
    }

    /// `h` and the hex of the keyed hash of `parts` in `domain`. The domain is hashed in, so one
    /// value in two domains gets two hashes, and one value in one domain gets one hash at every
    /// path. A truncated hash already handed out for a different input is an error.
    pub(super) fn value(&mut self, domain: &str, parts: &[&str]) -> Result<String, String> {
        let mut input: Vec<&[u8]> = vec![domain.as_bytes()];
        input.extend(parts.iter().map(|part| part.as_bytes()));
        let digest = self.prf("value", &input);
        let short = digest[..self.width].to_vec();
        let shown = format!("h{}", crate::hex(&short));
        match self.seen.insert(short, digest) {
            Some(old) if old != digest => Err(format!(
                "two different values hash to {shown}: a {}-bit collision. Rerun with a new replicate key",
                self.width * 8
            )),
            _ => Ok(shown),
        }
    }

    /// The replicate's timestamp shift in seconds: one day plus up to five years, either
    /// direction.
    pub(super) fn shift(&self) -> i64 {
        let digest = self.prf("shift", &[]);
        let mut first = [0; 8];
        first.copy_from_slice(&digest[..8]);
        let draw = u64::from_le_bytes(first);
        let magnitude = SHIFT_FLOOR + (draw >> 1) % SHIFT_RANGE;
        // Far below 2^63 by construction (SHIFT_FLOOR + SHIFT_RANGE is about 1.6e8).
        let magnitude = i64::try_from(magnitude).unwrap_or(i64::MAX);
        if draw & 1 == 0 { magnitude } else { -magnitude }
    }

    /// `f1…fN` for `paths`, ranked by each path's keyed hash: the order is random per key and
    /// fixed by it.
    pub(super) fn field_names(
        &self,
        paths: &BTreeSet<Vec<String>>,
    ) -> BTreeMap<Vec<String>, String> {
        let mut ranked: Vec<([u8; 32], &Vec<String>)> = paths
            .iter()
            .map(|path| {
                let parts: Vec<&[u8]> = path.iter().map(String::as_bytes).collect();
                (self.prf("field", &parts), path)
            })
            .collect();
        ranked.sort();
        ranked
            .into_iter()
            .enumerate()
            .map(|(rank, (_, path))| (path.clone(), format!("f{}", rank + 1)))
            .collect()
    }
}

/// HMAC-SHA256 (RFC 2104) with a 32-byte key, which fits one 64-byte block unhashed.
fn hmac(key: &[u8; 32], message: &[u8]) -> [u8; 32] {
    let mut inner_pad = [0x36_u8; 64];
    let mut outer_pad = [0x5c_u8; 64];
    for (i, byte) in key.iter().enumerate() {
        inner_pad[i] ^= byte;
        outer_pad[i] ^= byte;
    }
    let inner = Sha256::new()
        .chain_update(inner_pad)
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(outer_pad)
        .chain_update(inner)
        .finalize()
        .into()
}

/// The key bytes from a key file's text: 64 hex digits, surrounding whitespace ignored.
pub(super) fn parse_key(text: &str) -> Result<[u8; 32], String> {
    let text = text.trim();
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(
            "a replicate key is 64 hex digits (32 bytes); make one with `head -c32 /dev/urandom | xxd -p -c64`"
                .to_owned(),
        );
    }
    let mut key = [0; 32];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).map_err(|e| e.to_string())?;
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::hmac;

    /// RFC 4231 test case 1: a 20-byte key of 0x0b. HMAC zero-pads a key to the block size, so
    /// the same key zero-padded to 32 bytes gives the RFC's digest.
    #[test]
    fn hmac_matches_rfc_4231() {
        let mut key = [0; 32];
        key[..20].fill(0x0b);
        let digest = hmac(&key, b"Hi There");
        let shown: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            shown,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }
}
