//! Renders a manifest's sentence templates over a decoded payload (decision 0029). Pure: the
//! same template and payload always give the same text, and a value the template cannot show
//! gives no sentence rather than a broken one.

use serde_json::Value;

use super::display::{display_text, is_display_space};
use super::{DeltaField, EventSentence, Sentence, SentenceField, TruncateField};

/// The characters a `truncate` field keeps before its ellipsis.
pub const TRUNCATE_CHARS: usize = 120;

/// The sentence for one event of `source`: the manifest's entries for that source whose `when`
/// matches the payload, in manifest order, then its entries with no `when`. The first entry
/// that renders wins; `None` when none does.
#[must_use]
pub fn sentence_for(events: &[EventSentence], source: &str, payload: &Value) -> Option<String> {
    let of_source = || events.iter().filter(|e| e.source == source);
    let matched = of_source().filter(|e| {
        e.when.as_ref().is_some_and(|when| {
            when.path.lookup(payload).and_then(scalar_text).as_deref() == Some(&when.equals)
        })
    });
    let unconditional = of_source().filter(|e| e.when.is_none());
    matched
        .chain(unconditional)
        .find_map(|e| render_sentence(&e.sentence, payload))
}

/// Fills `sentence`'s `{n}` placeholders from `payload`. A path shows a string as its
/// [`display_text`] and a number or bool as its JSON text; `delta` shows the signed difference
/// of two integers (`+3`, `-12`, `+0`); `truncate` shows at most [`TRUNCATE_CHARS`] characters
/// of the display text, then `…`.
///
/// A field whose shown text is empty is absent, with one exception: when it is the last thing
/// in the template (only whitespace follows), it is dropped together with the whitespace and
/// separator punctuation (`: ; , - – —`) before it, so `X edited Y: {2}` with an empty `{2}`
/// reads `X edited Y`.
///
/// `None` when a shown field is absent, empty (except as above), null, an array or an object,
/// when a `delta` operand is not an integer or the difference overflows, when nothing is left
/// to show, or when the text breaks the placeholder grammar a valid manifest keeps.
#[must_use]
pub fn render_sentence(sentence: &Sentence, payload: &Value) -> Option<String> {
    let mut out = String::with_capacity(sentence.text.len());
    let mut chars = sentence.text.chars();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut digits = String::new();
                loop {
                    match chars.next()? {
                        '}' => break,
                        d if d.is_ascii_digit() && digits.len() < 3 => digits.push(d),
                        _ => return None,
                    }
                }
                let index: usize = digits.parse().ok()?;
                let value = field(sentence.fields.get(index)?, payload)?;
                if value.is_empty() {
                    if !chars.as_str().chars().all(is_display_space) {
                        return None;
                    }
                    let kept = out.trim_end_matches(is_trailing_separator).len();
                    out.truncate(kept);
                    return (!out.is_empty()).then_some(out);
                }
                out.push_str(&value);
            }
            '}' => return None,
            c => out.push(c),
        }
    }
    Some(out)
}

/// Whitespace or separator punctuation left before a dropped last field.
fn is_trailing_separator(c: char) -> bool {
    is_display_space(c) || matches!(c, ':' | ';' | ',' | '-' | '–' | '—')
}

fn field(field: &SentenceField, payload: &Value) -> Option<String> {
    match field {
        SentenceField::Path(path) => path.lookup(payload).and_then(shown_text),
        SentenceField::Delta(DeltaField { delta: [a, b] }) => {
            let a = a.lookup(payload)?.as_i64()?;
            let b = b.lookup(payload)?.as_i64()?;
            Some(format!("{:+}", a.checked_sub(b)?))
        }
        SentenceField::Truncate(TruncateField { truncate }) => {
            let text = truncate.lookup(payload).and_then(shown_text)?;
            Some(match text.char_indices().nth(TRUNCATE_CHARS) {
                Some((cut, _)) => format!("{}…", &text[..cut]),
                None => text,
            })
        }
    }
}

/// A scalar as a sentence shows it: a string as its [`display_text`], a number or bool as its
/// JSON text.
fn shown_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(display_text(text)),
        other => scalar_text(other),
    }
}

/// A scalar's text: a string as it is, a number or bool as its JSON text. `when` matches on it.
fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::dashboard::When;
    use crate::{FieldPath, Segment};

    fn p(keys: &[&str]) -> FieldPath {
        FieldPath(keys.iter().map(|k| Segment::Key((*k).to_owned())).collect())
    }

    fn s(text: &str, fields: Vec<SentenceField>) -> Sentence {
        Sentence {
            text: text.to_owned(),
            fields,
        }
    }

    #[test]
    fn paths_show_strings_numbers_and_bools() {
        let payload = json!({"a": "Ada", "n": 42, "f": 1.5, "b": true, "x": {"y": "deep"}});
        let sentence = s(
            "{0} has {1} ({2}, {3}) at {4}; {0} again",
            vec![
                SentenceField::Path(p(&["a"])),
                SentenceField::Path(p(&["n"])),
                SentenceField::Path(p(&["f"])),
                SentenceField::Path(p(&["b"])),
                SentenceField::Path(p(&["x", "y"])),
            ],
        );
        assert_eq!(
            render_sentence(&sentence, &payload).as_deref(),
            Some("Ada has 42 (1.5, true) at deep; Ada again")
        );
    }

    #[test]
    fn a_missing_null_or_composite_value_gives_no_sentence() {
        let payload = json!({"z": null, "arr": [1], "obj": {}});
        for key in ["missing", "z", "arr", "obj"] {
            let sentence = s("{0}", vec![SentenceField::Path(p(&[key]))]);
            assert_eq!(render_sentence(&sentence, &payload), None, "{key}");
        }
    }

    #[test]
    fn delta_is_a_signed_integer_difference() {
        let delta = |a: Value, b: Value| {
            let sentence = s(
                "{0}",
                vec![SentenceField::Delta(DeltaField {
                    delta: [p(&["a"]), p(&["b"])],
                })],
            );
            render_sentence(&sentence, &json!({"a": a, "b": b}))
        };
        assert_eq!(delta(json!(10), json!(7)).as_deref(), Some("+3"));
        assert_eq!(delta(json!(7), json!(19)).as_deref(), Some("-12"));
        assert_eq!(delta(json!(5), json!(5)).as_deref(), Some("+0"));
        assert_eq!(delta(json!(1.5), json!(1)), None);
        assert_eq!(delta(json!("3"), json!(1)), None);
        assert_eq!(delta(json!(i64::MIN), json!(1)), None);
    }

    #[test]
    fn truncate_cuts_at_120_characters_on_a_char_boundary() {
        let sentence = s(
            "[{0}]",
            vec![SentenceField::Truncate(TruncateField {
                truncate: p(&["t"]),
            })],
        );
        let short = "é".repeat(TRUNCATE_CHARS);
        let long = "é".repeat(TRUNCATE_CHARS + 1);
        assert_eq!(
            render_sentence(&sentence, &json!({"t": short})),
            Some(format!("[{short}]"))
        );
        assert_eq!(
            render_sentence(&sentence, &json!({"t": long})),
            Some(format!("[{}…]", "é".repeat(TRUNCATE_CHARS)))
        );
        assert_eq!(
            render_sentence(&sentence, &json!({"t": 7})).as_deref(),
            Some("[7]")
        );
    }

    #[test]
    fn a_broken_placeholder_gives_no_sentence() {
        let fields = || vec![SentenceField::Path(p(&["a"]))];
        let payload = json!({"a": "x"});
        for text in ["{1}", "{a}", "{0", "0}", "{0000}", "{}"] {
            assert_eq!(
                render_sentence(&s(text, fields()), &payload),
                None,
                "{text}"
            );
        }
    }

    #[test]
    fn a_matching_when_goes_first_then_the_unconditional_entries_in_order() {
        let entry = |when: Option<&str>, text: &str, key: &str| EventSentence {
            source: "s1".to_owned(),
            when: when.map(|equals| When {
                path: p(&["kind"]),
                equals: equals.to_owned(),
            }),
            sentence: s(text, vec![SentenceField::Path(p(&[key]))]),
        };
        let events = vec![
            entry(None, "plain {0}", "missing"),
            entry(None, "fallback {0}", "a"),
            entry(Some("edit"), "edited {0}", "a"),
            entry(Some("create"), "created {0}", "a"),
        ];
        let at = |kind: Value| json!({"kind": kind, "a": "A"});
        let got = |payload: &Value, source: &str| sentence_for(&events, source, payload);
        assert_eq!(
            got(&at(json!("create")), "s1").as_deref(),
            Some("created A")
        );
        assert_eq!(got(&at(json!("edit")), "s1").as_deref(), Some("edited A"));
        assert_eq!(
            got(&at(json!("other")), "s1").as_deref(),
            Some("fallback A")
        );
        assert_eq!(got(&at(json!("edit")), "s2"), None);
    }

    #[test]
    fn shown_strings_are_display_text_and_when_matches_the_raw_value() {
        let trunc = |key: &str| {
            SentenceField::Truncate(TruncateField {
                truncate: p(&[key]),
            })
        };
        let sentence = s(
            "{0} edited {1}: {2}",
            vec![
                SentenceField::Path(p(&["who"])),
                SentenceField::Path(p(&["title"])),
                trunc("c"),
            ],
        );
        let payload =
            json!({"who": "Ada", "title": "Tucson,_Arizona", "c": "/* History */ fixed  it"});
        assert_eq!(
            render_sentence(&sentence, &payload).as_deref(),
            Some("Ada edited Tucson, Arizona: fixed it")
        );
        let events = vec![EventSentence {
            source: "s1".to_owned(),
            when: Some(When {
                path: p(&["kind"]),
                equals: "a_b".to_owned(),
            }),
            sentence: s("{0}", vec![SentenceField::Path(p(&["kind"]))]),
        }];
        assert_eq!(
            sentence_for(&events, "s1", &json!({"kind": "a_b"})).as_deref(),
            Some("a b")
        );
        assert_eq!(sentence_for(&events, "s1", &json!({"kind": "a b"})), None);
    }

    #[test]
    fn an_empty_last_field_drops_with_its_separator_and_an_empty_other_field_gives_none() {
        let trunc = |key: &str| {
            SentenceField::Truncate(TruncateField {
                truncate: p(&[key]),
            })
        };
        let tail = s(
            "{0} edited {1}: {2}  ",
            vec![
                SentenceField::Path(p(&["who"])),
                SentenceField::Path(p(&["title"])),
                trunc("c"),
            ],
        );
        for empty in ["", "   ", "/* Only a section */"] {
            let payload = json!({"who": "Ada", "title": "Page", "c": empty});
            assert_eq!(
                render_sentence(&tail, &payload).as_deref(),
                Some("Ada edited Page"),
                "{empty:?}"
            );
        }
        let dashes = s(
            "{0} :; —- {1}",
            vec![
                SentenceField::Path(p(&["a"])),
                SentenceField::Path(p(&["b"])),
            ],
        );
        assert_eq!(
            render_sentence(&dashes, &json!({"a": "X", "b": ""})).as_deref(),
            Some("X")
        );
        let middle = s(
            "{0} edited {1}",
            vec![
                SentenceField::Path(p(&["who"])),
                SentenceField::Path(p(&["title"])),
            ],
        );
        assert_eq!(
            render_sentence(&middle, &json!({"who": "", "title": "Page"})),
            None
        );
        let only = s("{0}", vec![SentenceField::Path(p(&["a"]))]);
        assert_eq!(render_sentence(&only, &json!({"a": ""})), None);
    }

    #[test]
    fn an_entry_with_an_empty_field_falls_through_to_the_next() {
        let trunc = |key: &str| {
            SentenceField::Truncate(TruncateField {
                truncate: p(&[key]),
            })
        };
        let events = vec![EventSentence {
            source: "s1".to_owned(),
            when: None,
            sentence: s(
                "{0} said {1}",
                vec![SentenceField::Path(p(&["who"])), trunc("c")],
            ),
        }];
        assert_eq!(
            sentence_for(&events, "s1", &json!({"who": "Ada", "c": ""})).as_deref(),
            Some("Ada said")
        );
        let fallthrough = vec![
            EventSentence {
                source: "s1".to_owned(),
                when: None,
                sentence: s(
                    "{1} by {0}",
                    vec![SentenceField::Path(p(&["who"])), trunc("c")],
                ),
            },
            EventSentence {
                source: "s1".to_owned(),
                when: None,
                sentence: s("{0} acted", vec![SentenceField::Path(p(&["who"]))]),
            },
        ];
        assert_eq!(
            sentence_for(&fallthrough, "s1", &json!({"who": "Ada", "c": ""})).as_deref(),
            Some("Ada acted")
        );
    }
}
