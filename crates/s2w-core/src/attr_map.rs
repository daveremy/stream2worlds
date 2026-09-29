//! [`AttrMap`]: one entity's attributes, stored as a sorted vector (s2w#172).

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::event::AttrValue;

/// One entity's attributes: each name holds its latest observed value, iterated in name order.
///
/// Stored as a `Vec` of `(name, value)` pairs, sorted by name with no repeated name, at exact
/// capacity. That is 48 B per attribute, where a `BTreeMap` allocates a 544 B leaf node however
/// few entries it holds (s2w#172 measured the leaf as 65% of the world's heap per entity).
///
/// It serializes exactly as a `BTreeMap<String, AttrValue>` does, a map in name order, so the
/// postcard snapshot bytes, `world_hash` and JSON are the same as before the change, and it
/// deserializes through one, so duplicate and unsorted names are handled as a map handles them.
///
/// Lookup is a binary search. Inserting a new name shifts the names after it, which is cheap for
/// the few attributes an entity carries and linear in the attribute count for an entity with many.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct AttrMap(Vec<(String, AttrValue)>);

impl AttrMap {
    /// The number of attributes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the map holds no attributes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The value held under `name`, if any.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&AttrValue> {
        let index = self.position(name).ok()?;
        self.0.get(index).map(|(_, value)| value)
    }

    /// Sets every attribute in `attrs`, replacing the value of a name already held, as inserting
    /// each pair into a map would. Allocates at most once, to exact capacity (`reserve_exact`:
    /// collecting a `BTreeMap` iterator would round a 1-3 entry `Vec` up to 4).
    pub fn extend_from_map(&mut self, attrs: &BTreeMap<String, AttrValue>) {
        if self.0.is_empty() {
            // The common case, an entity's first observation: `attrs` is already sorted.
            self.0.reserve_exact(attrs.len());
            self.0
                .extend(attrs.iter().map(|(k, v)| (k.clone(), v.clone())));
            return;
        }
        let added = attrs
            .keys()
            .filter(|name| self.position(name).is_err())
            .count();
        self.0.reserve_exact(added);
        for (name, value) in attrs {
            match self.position(name) {
                Ok(index) => {
                    if let Some((_, slot)) = self.0.get_mut(index) {
                        value.clone_into(slot);
                    }
                }
                Err(index) => self.0.insert(index, (name.clone(), value.clone())),
            }
        }
    }

    /// The attributes as `(name, value)`, in name order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&String, &AttrValue)> {
        self.0.iter().map(|(name, value)| (name, value))
    }

    fn position(&self, name: &str) -> Result<usize, usize> {
        self.0.binary_search_by(|(key, _)| key.as_str().cmp(name))
    }
}

impl From<BTreeMap<String, AttrValue>> for AttrMap {
    fn from(map: BTreeMap<String, AttrValue>) -> Self {
        let mut pairs = Vec::with_capacity(map.len());
        pairs.extend(map);
        Self(pairs)
    }
}

impl std::fmt::Debug for AttrMap {
    /// Formats as a map, `{"name": value}`, the way the `BTreeMap` it replaced did.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl Serialize for AttrMap {
    /// A map in name order: the same call, and so the same bytes, as `BTreeMap`'s own impl. The
    /// iterator must stay exact-size (a slice map, never a filter): postcard writes the length
    /// first and refuses a map whose length it is not told.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.iter())
    }
}

impl<'de> Deserialize<'de> for AttrMap {
    /// Through a `BTreeMap`, deliberately: a snapshot is untrusted input, and the map sorts it and
    /// keeps the last value of a repeated name, which the binary search relies on. The transient
    /// map per entity on restore is the price of not re-implementing that.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        BTreeMap::<String, AttrValue>::deserialize(deserializer).map(Self::from)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::AttrMap;
    use crate::event::AttrValue;

    fn map(pairs: &[(&str, AttrValue)]) -> BTreeMap<String, AttrValue> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect()
    }

    fn sample() -> BTreeMap<String, AttrValue> {
        map(&[
            ("lang", AttrValue::Str("en".into())),
            ("bot", AttrValue::Bool(true)),
            ("age", AttrValue::Int(-3)),
        ])
    }

    #[test]
    fn extend_keeps_name_order_and_replaces() {
        let mut attrs = AttrMap::default();
        attrs.extend_from_map(&map(&[("b", AttrValue::Int(1))]));
        attrs.extend_from_map(&map(&[("c", AttrValue::Int(3)), ("a", AttrValue::Int(2))]));
        attrs.extend_from_map(&map(&[("b", AttrValue::Int(9))]));
        let names: Vec<&str> = attrs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
        assert_eq!(attrs.get("b"), Some(&AttrValue::Int(9)));
        assert_eq!(attrs.get("z"), None);
        assert_eq!(attrs.len(), 3);
        assert_eq!(
            format!("{attrs:?}"),
            r#"{"a": Int(2), "b": Int(9), "c": Int(3)}"#
        );
    }

    #[test]
    fn extend_from_map_matches_btreemap_last_write_wins() {
        let first = sample();
        let second = map(&[
            ("bot", AttrValue::Bool(false)),
            ("aaa", AttrValue::Int(1)),
            ("zzz", AttrValue::Str("tail".into())),
        ]);
        let mut attrs = AttrMap::default();
        attrs.extend_from_map(&first);
        assert_eq!(
            attrs.0.capacity(),
            first.len(),
            "first observation is exact"
        );
        attrs.extend_from_map(&second);
        assert_eq!(
            attrs.0.capacity(),
            5,
            "one exact reservation for the new names"
        );

        let mut expected = first;
        expected.extend(second);
        assert_eq!(attrs, AttrMap::from(expected.clone()));
        let pairs: Vec<(&String, &AttrValue)> = attrs.iter().collect();
        let want: Vec<(&String, &AttrValue)> = expected.iter().collect();
        assert_eq!(pairs, want);
    }

    #[test]
    fn json_is_the_btreemap_json() {
        for source in [BTreeMap::new(), sample()] {
            let attrs = AttrMap::from(source.clone());
            assert_eq!(
                serde_json::to_string(&attrs).unwrap(),
                serde_json::to_string(&source).unwrap()
            );
            assert_eq!(
                serde_json::to_string_pretty(&attrs).unwrap(),
                serde_json::to_string_pretty(&source).unwrap()
            );
            let back: AttrMap =
                serde_json::from_str(&serde_json::to_string(&attrs).unwrap()).unwrap();
            assert_eq!(back, attrs);
        }
    }

    #[test]
    fn deserialize_handles_unsorted_and_repeated_names_as_a_map_does() {
        let text = r#"{"b":{"Int":1},"a":{"Int":2},"b":{"Int":3}}"#;
        let attrs: AttrMap = serde_json::from_str(text).unwrap();
        let source: BTreeMap<String, AttrValue> = serde_json::from_str(text).unwrap();
        assert_eq!(attrs, AttrMap::from(source));
        assert_eq!(attrs.get("b"), Some(&AttrValue::Int(3)));
        let names: Vec<&str> = attrs.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }
}
