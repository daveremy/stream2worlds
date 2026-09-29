use super::*;

fn key(label: &str, parts: &[KeyPart]) -> NaturalKey {
    NaturalKey::from_parts(label, parts).expect("label has no separator")
}

fn text(s: &str) -> KeyPart {
    KeyPart::Str(s.to_owned())
}

#[test]
fn the_format_is_pinned() {
    let built = key(
        "t",
        &[
            text("a\"b"),
            KeyPart::Int(-7),
            KeyPart::Bool(true),
            text("7"),
        ],
    );
    assert_eq!(
        built.as_str(),
        "t\u{1f}\"a\\\"b\"\u{1f}-7\u{1f}true\u{1f}\"7\""
    );
}

#[test]
fn string_parts_match_serde_json_byte_for_byte() {
    let mut samples: Vec<String> = (0u32..=0x100)
        .filter_map(char::from_u32)
        .map(String::from)
        .collect();
    samples.extend(
        [
            "",
            "plain",
            "a/b",
            "\u{2028}\u{2029}",
            "\u{1f600}",
            "\u{7f}",
            "é\\\"\n",
        ]
        .map(str::to_owned),
    );
    for sample in samples {
        let mut ours = String::new();
        encode_json_string(&mut ours, &sample);
        let theirs = serde_json::to_string(&sample).expect("a string encodes");
        assert_eq!(ours, theirs, "{sample:?}");
    }
}

#[test]
fn parts_round_trip() {
    let parts = vec![
        text(""),
        text("\u{0}\u{1f}\"\\/\u{1f600}é"),
        KeyPart::Int(i64::MIN),
        KeyPart::Int(0),
        KeyPart::Int(i64::MAX),
        KeyPart::Bool(false),
        text("true"),
        text("12"),
    ];
    let built = key("type", &parts);
    assert_eq!(built.parts(), Ok(("type", parts)));
}

#[test]
fn a_label_with_the_separator_is_refused() {
    assert_eq!(
        NaturalKey::from_parts("a\u{1f}b", &[]),
        Err(KeyError::SeparatorInLabel)
    );
}

#[test]
fn free_form_and_empty_keys_have_no_parts() {
    assert_eq!(
        NaturalKey::new("a:page:1").parts(),
        Ok(("a:page:1", vec![]))
    );
    assert_eq!(NaturalKey::new("").parts(), Ok(("", vec![])));
}

#[test]
fn any_json_escape_is_read() {
    let built = NaturalKey::new("t\u{1f}\"\\u00e9\\/\\ud83d\\ude00\\U0041\"");
    // `\U` is not a JSON escape: the part is refused rather than guessed at.
    assert_eq!(built.parts(), Err(KeyError::BadPart { index: 0 }));
    let built = NaturalKey::new("t\u{1f}\"\\u00E9\\/\\ud83d\\ude00\"");
    assert_eq!(built.parts(), Ok(("t", vec![text("é/\u{1f600}")])));
}

#[test]
fn non_canonical_parts_are_refused() {
    for bad in [
        "+5",
        "007",
        "-0",
        "",
        "1.5",
        "True",
        "null",
        "\"open",
        "\"a\"b",
        "\"\u{1}\"",
        "\"\\ud83d\"",
        "\"\\ude00\"",
        "\"\\ud83d\\u0041\"",
        "\"\\x\"",
        "\"\\u12\"",
        "99999999999999999999",
    ] {
        let built = NaturalKey::new(format!("t{KEY_SEPARATOR}{bad}"));
        assert_eq!(
            built.parts(),
            Err(KeyError::BadPart { index: 0 }),
            "{bad:?}"
        );
    }
}

#[test]
fn the_first_bad_part_is_named() {
    let built = NaturalKey::new("t\u{1f}1\u{1f}007");
    assert_eq!(built.parts(), Err(KeyError::BadPart { index: 1 }));
}
