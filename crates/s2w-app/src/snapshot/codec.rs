//! The snapshot file format, pure: `MAGIC | u32 LE payload length | payload | u64 LE FNV-1a of
//! the payload`, where the payload is a postcard-encoded [`SnapshotV1`]. No paths or host
//! details are inside, so the bytes are portable (a later object-store backend reuses these
//! functions unchanged).

use s2w_model::fnv1a64;

use super::{Invalid, SNAPSHOT_FORMAT, SnapshotError, SnapshotRefV1, SnapshotV1};

/// The first eight bytes of every snapshot file.
pub const MAGIC: [u8; 8] = *b"S2WSNAP1";

const HEADER: usize = MAGIC.len() + 4;
const TRAILER: usize = 8;

/// The file bytes for `snapshot`.
///
/// # Errors
/// [`SnapshotError::Encode`] if the payload does not serialize or exceeds `u32::MAX` bytes.
pub fn encode(snapshot: &SnapshotV1) -> Result<Vec<u8>, SnapshotError> {
    encode_ref(&snapshot.as_ref_v1())
}

/// The file bytes for a borrowed snapshot, identical to [`encode`] of the owned one. The payload
/// is serialized straight into the output buffer, so the only large allocation is the file
/// itself (#179).
///
/// # Errors
/// [`SnapshotError::Encode`] if the payload does not serialize or exceeds `u32::MAX` bytes.
pub fn encode_ref(snapshot: &SnapshotRefV1<'_>) -> Result<Vec<u8>, SnapshotError> {
    let mut out = Vec::with_capacity(HEADER + TRAILER);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&[0; 4]);
    let mut out =
        postcard::to_extend(snapshot, out).map_err(|e| SnapshotError::Encode(e.to_string()))?;
    let payload_len = out.len().saturating_sub(HEADER);
    let len = u32::try_from(payload_len).map_err(|_| {
        SnapshotError::Encode(format!("payload of {payload_len} bytes exceeds u32"))
    })?;
    if let Some(slot) = out.get_mut(MAGIC.len()..HEADER) {
        slot.copy_from_slice(&len.to_le_bytes());
    }
    let checksum = fnv1a64(out.get(HEADER..).unwrap_or_default());
    out.extend_from_slice(&checksum.to_le_bytes());
    Ok(out)
}

/// Validity rules 1 and 2: the bytes are one whole snapshot file with an intact checksum, of
/// this format, and decode completely.
///
/// # Errors
/// [`Invalid::Corrupt`] for a wrong magic, a length that disagrees with the file, a checksum
/// mismatch or a payload that does not decode; [`Invalid::Format`] for another format.
pub fn decode(bytes: &[u8]) -> Result<SnapshotV1, Invalid> {
    let corrupt = |why: &str| Invalid::Corrupt(why.to_owned());
    let magic = bytes
        .get(..MAGIC.len())
        .ok_or_else(|| corrupt("too short"))?;
    if magic != MAGIC {
        return Err(corrupt("wrong magic"));
    }
    let len = bytes
        .get(MAGIC.len()..HEADER)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| corrupt("too short"))?;
    let len = usize::try_from(len).map_err(|_| corrupt("length does not fit"))?;
    let expected = HEADER.saturating_add(len).saturating_add(TRAILER);
    if bytes.len() != expected {
        return Err(Invalid::Corrupt(format!(
            "length says {expected} bytes, file has {} (truncated or trailing bytes)",
            bytes.len()
        )));
    }
    let payload = bytes
        .get(HEADER..HEADER + len)
        .ok_or_else(|| corrupt("too short"))?;
    let checksum = bytes
        .get(HEADER + len..)
        .and_then(|b| <[u8; 8]>::try_from(b).ok())
        .map(u64::from_le_bytes)
        .ok_or_else(|| corrupt("too short"))?;
    if checksum != fnv1a64(payload) {
        return Err(corrupt("checksum mismatch"));
    }
    // `format` is the payload's first field: refuse another format before decoding the rest,
    // whose layout it may not share.
    let (format, _) = postcard::take_from_bytes::<u32>(payload)
        .map_err(|e| Invalid::Corrupt(format!("payload: {e}")))?;
    if format != SNAPSHOT_FORMAT {
        return Err(Invalid::Format { found: format });
    }
    let (snapshot, rest) = postcard::take_from_bytes::<SnapshotV1>(payload)
        .map_err(|e| Invalid::Corrupt(format!("payload: {e}")))?;
    if !rest.is_empty() {
        return Err(corrupt("payload has trailing bytes"));
    }
    Ok(snapshot)
}
