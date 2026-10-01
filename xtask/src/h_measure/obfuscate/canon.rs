//! Canonicalization formats for identifiers embedded in URLs (contract B2.2): a URL path tail
//! after a marker, and `?name=value` query parameters, both percent-decoded (RFC 3986). Formats
//! only; which URLs and which parameters is data in the rules file.

/// `text` percent-decoded as UTF-8; with `plus`, `+` is a space (form encoding, as in a query).
/// `None` for a `%` not followed by two hex digits, or bytes that are not UTF-8.
pub(super) fn percent_decode(text: &str, plus: bool) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let pair = text.get(i + 1..i + 3)?;
                if !pair.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                out.push(u8::from_str_radix(pair, 16).ok()?);
                i += 3;
            }
            b'+' if plus => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// The identifier in `value` when it is `base`, then `marker`, then a tail: the tail cut at `?`
/// or `#`, percent-decoded, then each `(from, to)` replacement applied in order. `None` when
/// `value` has another shape or the tail does not decode.
pub(super) fn url_tail(
    value: &str,
    base: &str,
    marker: &str,
    replace: &[(String, String)],
) -> Option<String> {
    let tail = value.strip_prefix(base)?.strip_prefix(marker)?;
    let tail = tail.split(['?', '#']).next().unwrap_or_default();
    if tail.is_empty() {
        return None;
    }
    let mut decoded = percent_decode(tail, false)?;
    for (from, to) in replace {
        decoded = decoded.replace(from.as_str(), to);
    }
    Some(decoded)
}

/// The query parameters of `url` in order, names and values percent-decoded with `+` as a space.
/// A parameter with no `=` has an empty value. `None` when no `?` is present or any part does not
/// decode.
pub(super) fn query(url: &str) -> Option<Vec<(String, String)>> {
    let (_, rest) = url.split_once('?')?;
    let rest = rest.split('#').next().unwrap_or_default();
    rest.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            Some((percent_decode(name, true)?, percent_decode(value, true)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{percent_decode, query, url_tail};

    #[test]
    fn percent_decoding_reads_utf8_and_refuses_bad_escapes() {
        assert_eq!(
            percent_decode("Ca%C3%B1a_x", false).as_deref(),
            Some("Caña_x")
        );
        assert_eq!(percent_decode("a+b", true).as_deref(), Some("a b"));
        assert_eq!(percent_decode("a+b", false).as_deref(), Some("a+b"));
        assert_eq!(percent_decode("50%", false), None);
        assert_eq!(percent_decode("%zz", false), None);
        assert_eq!(percent_decode("%FF", false), None);
    }

    #[test]
    fn url_tail_needs_the_base_and_marker() {
        let swap = [("_".to_owned(), " ".to_owned())];
        let tail = url_tail(
            "https://h.example/d/Big_%C3%B1?x=1",
            "https://h.example",
            "/d/",
            &swap,
        );
        assert_eq!(tail.as_deref(), Some("Big ñ"));
        assert_eq!(
            url_tail("https://other/d/Big", "https://h.example", "/d/", &swap),
            None
        );
        assert_eq!(
            url_tail("https://h.example/e/Big", "https://h.example", "/d/", &swap),
            None
        );
        assert_eq!(
            url_tail("https://h.example/d/", "https://h.example", "/d/", &swap),
            None
        );
    }

    #[test]
    fn query_reads_every_parameter_in_order() {
        let found = query("https://h.example/x?b=2&a=1+1&flag#frag");
        let expected = [("b", "2"), ("a", "1 1"), ("flag", "")]
            .map(|(n, v)| (n.to_owned(), v.to_owned()))
            .to_vec();
        assert_eq!(found, Some(expected));
        assert_eq!(query("https://h.example/x"), None);
    }
}
