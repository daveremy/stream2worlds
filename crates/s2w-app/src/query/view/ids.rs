//! Borrowed wire forms of node ids and links, ordered as their serialized strings are.

use std::cmp::Ordering;

use s2w_core::EntityId;
use serde::Serialize;
use serde::ser::Serializer;

/// The decimal digits of an entity id, ordered as its `e:<id>` node id string is: `e:10` sorts
/// before `e:2`. [`world_view`](super::world_view) orders nodes and links by those strings; a
/// streamed view walking `EntityId` order instead would serve different bytes (#216).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::query) struct IdDigits {
    digits: [u8; 20],
    len: usize,
}

impl IdDigits {
    pub(in crate::query) fn new(id: EntityId) -> Self {
        let mut rev = [0u8; 20];
        let mut n = id.get();
        let mut len = 0;
        loop {
            rev[len] = b"0123456789"[usize::try_from(n % 10).unwrap_or_default()];
            len += 1;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        let mut digits = [0u8; 20];
        for (d, r) in digits.iter_mut().zip(rev[..len].iter().rev()) {
            *d = *r;
        }
        Self { digits, len }
    }

    fn as_bytes(&self) -> &[u8] {
        &self.digits[..self.len]
    }
}

impl Ord for IdDigits {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl PartialOrd for IdDigits {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// An `e:<id>` node id, written straight into the serializer with no owned string.
pub(in crate::query) struct NodeIdRef(pub(in crate::query) EntityId);

impl Serialize for NodeIdRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("e:{}", self.0.get()))
    }
}

/// [`Link`](super::Link)'s wire shape over borrowed parts.
#[derive(Serialize)]
pub(in crate::query) struct LinkRef<'a> {
    pub(in crate::query) source: NodeIdRef,
    pub(in crate::query) target: NodeIdRef,
    pub(in crate::query) kind: &'a str,
    pub(in crate::query) weight: u64,
}
