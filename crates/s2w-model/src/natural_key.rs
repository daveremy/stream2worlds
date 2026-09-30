//! Natural keys and the one text format a mapped key uses (decision 0021).
//!
//! A mapped key is the entity type label, then each key part, joined by [`KEY_SEPARATOR`]. A
//! string part is JSON-encoded exactly as `serde_json::to_string` would encode it (so a string
//! `"7"` and an integer `7` stay distinct keys), an integer part is its decimal text and a bool
//! part is `true` or `false`. [`NaturalKey::from_parts`] builds this text and
//! [`NaturalKey::parts`] reads it back; nothing else in the workspace builds or splits it but
//! the viewer's measurement script, `crates/s2w-app/web/scripts/measure-labels.mjs`.
//!
//! The format is versioned by [`KEY_FORMAT`] (decision 0023). Key text is persisted in stored
//! verdicts and world snapshots; both are keyed, through the mapping identity
//! ([`crate::StreamMapping::identity`]), on this version, so a bump renames every mapping
//! engine and forces one full rebuild instead of mixing two key formats in one world.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// Separates a natural key's components: the type label, then each JSON-encoded key part.
/// A structural separator, so a replay can split a key back into its parts.
pub const KEY_SEPARATOR: char = '\u{1f}';

/// The version of the key text format this module builds and reads. Bump it whenever
/// [`NaturalKey::from_parts`] or [`NaturalKey::parts`] change what bytes a key holds: it feeds
/// every mapping identity, so stored mapping verdicts and snapshots stop matching (decision
/// 0023). A test pins a known-answer key, so an encoding change without a bump fails the build.
pub const KEY_FORMAT: u32 = 1;

/// A source-defined identity (a page title, a user name) that the fold maps to a fold-assigned entity id.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NaturalKey(String);

/// One component of a mapped natural key. Distinct from [`crate::AttrValue`] although the
/// shapes match today: a key part is identity, and its encoding is frozen by this module.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyPart {
    /// Text, encoded as a JSON string.
    Str(String),
    /// A signed integer, encoded as its decimal text.
    Int(i64),
    /// A bool, encoded as `true` or `false`.
    Bool(bool),
}

/// Why a natural key could not be built or read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum KeyError {
    /// The type label contains [`KEY_SEPARATOR`].
    #[error("a key's type label contains the key separator")]
    SeparatorInLabel,
    /// A part is neither a JSON string, a canonical `i64` nor a bool.
    #[error("key part {index} is not a JSON string, a canonical integer or a bool")]
    BadPart {
        /// The part's position, counted from 0 after the label.
        index: usize,
    },
}

impl NaturalKey {
    /// Wraps a source-defined key.
    #[must_use]
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    /// The key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Builds a mapped key from a type label and its parts.
    ///
    /// # Errors
    /// [`KeyError::SeparatorInLabel`] when `type_label` contains [`KEY_SEPARATOR`].
    pub fn from_parts(type_label: &str, parts: &[KeyPart]) -> Result<Self, KeyError> {
        if type_label.contains(KEY_SEPARATOR) {
            return Err(KeyError::SeparatorInLabel);
        }
        let mut key = type_label.to_owned();
        for part in parts {
            key.push(KEY_SEPARATOR);
            match part {
                KeyPart::Str(text) => encode_json_string(&mut key, text),
                KeyPart::Int(number) => {
                    let _ = write!(key, "{number}");
                }
                KeyPart::Bool(flag) => {
                    let _ = write!(key, "{flag}");
                }
            }
        }
        Ok(Self(key))
    }

    /// Reads a mapped key back into its type label and parts.
    ///
    /// A key without [`KEY_SEPARATOR`] (a free-form key from [`NaturalKey::new`]) reads as the
    /// whole text with no parts; the empty key reads as an empty label with no parts. Only the
    /// text [`NaturalKey::from_parts`] writes is accepted as a part: a leading `+`, a leading
    /// zero, `-0`, a raw control character or a lone surrogate is an error.
    ///
    /// # Errors
    /// [`KeyError::BadPart`] for the first part that is not in that form.
    pub fn parts(&self) -> Result<(&str, Vec<KeyPart>), KeyError> {
        let mut pieces = self.0.split(KEY_SEPARATOR);
        let label = pieces.next().unwrap_or_default();
        let parts = pieces
            .enumerate()
            .map(|(index, text)| parse_part(text).ok_or(KeyError::BadPart { index }))
            .collect::<Result<_, _>>()?;
        Ok((label, parts))
    }
}

/// Appends `text` as a JSON string, byte-identical to `serde_json::to_string`: `"` and `\`
/// escaped, `\b \t \n \f \r` in short form, other control characters below U+0020 as `\u00xx`
/// with lowercase hex, and everything else (including `/`, DEL and non-ASCII) as is.
fn encode_json_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{0c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn parse_part(text: &str) -> Option<KeyPart> {
    match text {
        "true" => Some(KeyPart::Bool(true)),
        "false" => Some(KeyPart::Bool(false)),
        _ if text.starts_with('"') => decode_json_string(text).map(KeyPart::Str),
        _ => {
            let number: i64 = text.parse().ok()?;
            // Canonical only: the text `from_parts` would have written.
            (number.to_string() == text).then_some(KeyPart::Int(number))
        }
    }
}

/// Reads one complete JSON string literal, accepting every JSON escape. `None` for anything
/// `serde_json` would reject: a raw control character, a bad escape, a lone surrogate,
/// a missing closing quote or text after it.
fn decode_json_string(text: &str) -> Option<String> {
    let mut chars = text.strip_prefix('"')?.chars();
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return chars.next().is_none().then_some(out),
            '\\' => out.push(match chars.next()? {
                '"' => '"',
                '\\' => '\\',
                '/' => '/',
                'b' => '\u{08}',
                'f' => '\u{0c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'u' => decode_unicode_escape(&mut chars)?,
                _ => return None,
            }),
            c if u32::from(c) < 0x20 => return None,
            c => out.push(c),
        }
    }
}

/// The character after a `\u`, pairing a high surrogate with the `\uXXXX` low surrogate that
/// must follow it.
fn decode_unicode_escape(chars: &mut std::str::Chars<'_>) -> Option<char> {
    let first = hex4(chars)?;
    match first {
        0xD800..=0xDBFF => {
            if chars.next()? != '\\' || chars.next()? != 'u' {
                return None;
            }
            let second = hex4(chars)?;
            if !(0xDC00..=0xDFFF).contains(&second) {
                return None;
            }
            char::from_u32(0x1_0000 + ((first - 0xD800) << 10) + (second - 0xDC00))
        }
        0xDC00..=0xDFFF => None,
        _ => char::from_u32(first),
    }
}

fn hex4(chars: &mut std::str::Chars<'_>) -> Option<u32> {
    (0..4).try_fold(0, |acc, _| Some(acc * 16 + chars.next()?.to_digit(16)?))
}

#[cfg(test)]
mod tests;
