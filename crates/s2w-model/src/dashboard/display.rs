//! Display text for a value a sentence or a label shows (decision 0029, 2026-09-30
//! amendment). Generic text rules, never a field name or a domain: a `/* … */` comment span is
//! markup, not content; a value that is one token joined by underscores reads as words; runs of
//! whitespace are one space. The web viewer applies the same rules to labels it reads from
//! nodes (`web/src/manifest.ts`).

/// The whitespace the display rules know: ASCII whitespace and the no-break space. Kept narrow
/// on purpose so the viewer's copy of the rules can match it exactly.
#[must_use]
pub const fn is_display_space(c: char) -> bool {
    c.is_ascii_whitespace() || c == '\u{a0}'
}

/// `raw` as a person should read it: every `/* … */` span removed (an unterminated `/*` stays),
/// whitespace runs collapsed to one space and trimmed, and a value that is a single token
/// containing `_` shown with spaces for its underscores (`Tucson,_Arizona` reads
/// `Tucson, Arizona`; text that already has spaces keeps its underscores). May be empty.
#[must_use]
pub fn display_text(raw: &str) -> String {
    let stripped = strip_comment_spans(raw);
    let words: Vec<&str> = stripped
        .split(is_display_space)
        .filter(|w| !w.is_empty())
        .collect();
    if let [word] = words.as_slice()
        && word.contains('_')
    {
        return word
            .split('_')
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
    }
    words.join(" ")
}

/// `raw` with each `/* … */` span replaced by a space.
fn strip_comment_spans(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(open) = rest.find("/*") {
        let Some(close) = rest[open + 2..].find("*/") else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push(' ');
        rest = &rest[open + 2 + close + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_spans_are_removed_and_whitespace_collapsed() {
        assert_eq!(
            display_text("/* Early life */ fixed a typo"),
            "fixed a typo"
        );
        assert_eq!(display_text("a/*x*/b /* y */  c"), "a b c");
        assert_eq!(display_text("/* only a span */"), "");
        assert_eq!(display_text("  two\t\nlines\u{a0} "), "two lines");
        assert_eq!(display_text("open /* never closed"), "open /* never closed");
        assert_eq!(display_text(""), "");
    }

    #[test]
    fn one_token_with_underscores_reads_as_words() {
        assert_eq!(display_text("Tucson,_Arizona"), "Tucson, Arizona");
        assert_eq!(
            display_text("Draft:Battle_of_the_Wall"),
            "Draft:Battle of the Wall"
        );
        assert_eq!(display_text("_lead__trail_"), "lead trail");
        assert_eq!(display_text("___"), "");
        assert_eq!(display_text("keep snake_case here"), "keep snake_case here");
        assert_eq!(display_text("/* s */ Some_Title"), "Some Title");
        assert_eq!(display_text("plain"), "plain");
    }
}
