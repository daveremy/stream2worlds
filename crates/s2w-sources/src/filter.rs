//! A generic ingest-time field filter: `<json-path>[!]=<value>`, applied against a raw event
//! payload before it is stored. Data-driven (decision 0018): this module knows JSON paths and
//! values, never what any particular field or stream means.

/// Whether a filter keeps events whose field equals `value`, or keeps events whose field does
/// not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilterOp {
    /// The field at `path` must equal `value`.
    Eq,
    /// The field at `path` must not equal `value` (a missing field passes: it isn't the
    /// excluded value).
    Ne,
}

/// One `<json-path>[!]=<value>` filter, parsed from a CLI spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldFilter {
    path: Vec<String>,
    op: FilterOp,
    value: serde_json::Value,
}

impl FieldFilter {
    /// Parses `"a.b.c=value"` or `"a.b.c!=value"`. The path is dot-separated; `value` is parsed
    /// as JSON (so `--filter rev=123` compares against the number `123`, `--filter
    /// name="123"` against the string), falling back to a bare string when it isn't valid JSON
    /// (so `--filter wiki_id=enwiki` needs no quoting).
    ///
    /// # Errors
    ///
    /// Why the spec could not be parsed: no `=`, or an empty path.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let (path_part, value_part, op) = if let Some((path, value)) = spec.split_once("!=") {
            (path, value, FilterOp::Ne)
        } else if let Some((path, value)) = spec.split_once('=') {
            (path, value, FilterOp::Eq)
        } else {
            return Err(format!(
                "filter {spec:?}: expected <path>=<value> or <path>!=<value>"
            ));
        };
        if path_part.is_empty() {
            return Err(format!("filter {spec:?}: the path before = must not be empty"));
        }
        let path: Vec<String> = path_part.split('.').map(str::to_owned).collect();
        let value = serde_json::from_str(value_part)
            .unwrap_or_else(|_| serde_json::Value::String(value_part.to_owned()));
        Ok(Self { path, op, value })
    }

    /// Whether `payload`, parsed as JSON, matches this filter. An unparseable payload, or a
    /// payload missing the path, looks up as [`None`]: an `Eq` filter fails, a `Ne` filter
    /// passes.
    #[must_use]
    pub(crate) fn matches(&self, payload: &[u8]) -> bool {
        let found = serde_json::from_slice::<serde_json::Value>(payload)
            .ok()
            .and_then(|value| self.walk(&value));
        match (&self.op, found) {
            (FilterOp::Eq, Some(found)) => found == self.value,
            (FilterOp::Eq, None) => false,
            (FilterOp::Ne, Some(found)) => found != self.value,
            (FilterOp::Ne, None) => true,
        }
    }

    fn walk(&self, value: &serde_json::Value) -> Option<serde_json::Value> {
        let mut current = value;
        for segment in &self.path {
            current = current.get(segment)?;
        }
        Some(current.clone())
    }
}

/// Whether `payload` matches every filter in `filters` (AND). An empty list always matches.
#[must_use]
pub(crate) fn apply_all(filters: &[FieldFilter], payload: &[u8]) -> bool {
    filters.iter().all(|filter| filter.matches(payload))
}

#[cfg(test)]
mod tests {
    use super::{FieldFilter, apply_all};

    #[test]
    fn eq_matches_a_string_value_without_quoting() {
        let filter = FieldFilter::parse("wiki_id=enwiki").expect("valid spec");
        assert!(filter.matches(br#"{"wiki_id":"enwiki"}"#));
        assert!(!filter.matches(br#"{"wiki_id":"dewiki"}"#));
    }

    #[test]
    fn ne_matches_a_missing_field() {
        let filter = FieldFilter::parse("meta.domain!=canary").expect("valid spec");
        assert!(filter.matches(br#"{"other":1}"#));
    }

    #[test]
    fn eq_fails_on_a_missing_field() {
        let filter = FieldFilter::parse("meta.domain=canary").expect("valid spec");
        assert!(!filter.matches(br#"{"other":1}"#));
    }

    #[test]
    fn numeric_value_compares_as_json_number_not_string() {
        let filter = FieldFilter::parse("rev=123").expect("valid spec");
        assert!(filter.matches(br#"{"rev":123}"#));
        assert!(!filter.matches(br#"{"rev":"123"}"#));
    }

    #[test]
    fn quoted_value_compares_as_string() {
        let filter = FieldFilter::parse(r#"rev="123""#).expect("valid spec");
        assert!(filter.matches(br#"{"rev":"123"}"#));
        assert!(!filter.matches(br#"{"rev":123}"#));
    }

    #[test]
    fn null_value_is_addressable() {
        let filter = FieldFilter::parse("x=null").expect("valid spec");
        assert!(filter.matches(br#"{"x":null}"#));
    }

    #[test]
    fn nested_path_walks_dotted_segments() {
        let filter = FieldFilter::parse("meta.domain=canary").expect("valid spec");
        assert!(filter.matches(br#"{"meta":{"domain":"canary"}}"#));
    }

    #[test]
    fn empty_path_is_rejected() {
        assert!(FieldFilter::parse("=value").is_err());
    }

    #[test]
    fn missing_equals_is_rejected() {
        assert!(FieldFilter::parse("no-operator-here").is_err());
    }

    #[test]
    fn apply_all_with_no_filters_always_matches() {
        assert!(apply_all(&[], b"anything, even invalid json"));
    }

    #[test]
    fn apply_all_ands_every_filter() {
        let filters = vec![
            FieldFilter::parse("a=1").expect("valid"),
            FieldFilter::parse("b=2").expect("valid"),
        ];
        assert!(apply_all(&filters, br#"{"a":1,"b":2}"#));
        assert!(!apply_all(&filters, br#"{"a":1,"b":3}"#));
    }
}
