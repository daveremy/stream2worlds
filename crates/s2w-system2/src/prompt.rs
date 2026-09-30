//! The committed prompt and the one encoder every untrusted value goes through.

use std::fmt::Write as _;

use s2w_model::{Fnv64, ManifestInput};
use serde::Serialize;

/// The manifest prompt (decision 0029). It holds no domain examples; check 9 reads it.
const MANIFEST: &str = include_str!("../prompts/manifest.txt");

/// Appended to the prompt for the one repair call of an attempt.
const REPAIR: &str = include_str!("../prompts/manifest-repair.txt");

/// The hash of both prompt files, 16 lowercase hex digits. It folds into `input_hash`, so an
/// edit to either file is a new input.
pub(crate) fn prompt_hash() -> String {
    let mut hasher = Fnv64::new();
    hasher
        .write_field(MANIFEST.as_bytes())
        .write_field(REPAIR.as_bytes());
    format!("{:016x}", hasher.finish())
}

/// `value` as JSON on one line, safe to place between two marker lines.
///
/// `serde_json` escapes every control character below U+0020, so the text holds no `\n` or
/// `\r`. It leaves the C1 controls (U+0080 to U+009F, among them U+0085, next line) and U+2028
/// and U+2029 (line and paragraph separator) as they are, and some readers break lines at
/// those; this rewrites each as a `\u` escape, which decodes to the same value. The result
/// holds no line terminator of any kind, so no text inside it can forge a marker line.
///
/// # Errors
///
/// When `value` cannot be serialized.
pub fn data_line<T: Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
    let json = serde_json::to_string(value)?;
    let mut line = String::with_capacity(json.len());
    for ch in json.chars() {
        if matches!(ch, '\u{80}'..='\u{9f}' | '\u{2028}' | '\u{2029}') {
            // Writing to a String cannot fail.
            let _ = write!(line, "\\u{:04x}", u32::from(ch));
        } else {
            line.push(ch);
        }
    }
    Ok(line)
}

/// The first call's prompt for `input`.
pub(crate) fn manifest_prompt(input: &ManifestInput) -> Result<String, serde_json::Error> {
    Ok(fill(MANIFEST, &[("DATA", &data_line(input)?)]))
}

/// The repair call's prompt: the first prompt, then the previous reply and its fault as data.
pub(crate) fn repair_prompt(
    first: &str,
    reply: &str,
    fault: &str,
) -> Result<String, serde_json::Error> {
    let tail = fill(
        REPAIR,
        &[("REPLY", &data_line(reply)?), ("FAULT", &data_line(fault)?)],
    );
    Ok(format!("{first}{tail}"))
}

/// Replaces each `{{NAME}}` in `template` with its value, in one pass over the template only,
/// so a value that happens to contain `{{NAME}}` is never expanded.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let hit = after.find("}}").and_then(|end| {
            let name = &after[..end];
            values
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (end, *value))
        });
        if let Some((end, value)) = hit {
            out.push_str(value);
            rest = &after[end + 2..];
        } else {
            out.push_str("{{");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests;
