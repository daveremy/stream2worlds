//! The committed prompt and the one encoder every untrusted value goes through.

use std::fmt::Write as _;

use s2w_model::{Fnv64, ManifestInput, MappingInput, RawMappingInput};
use serde::Serialize;

/// The manifest prompt (decision 0029). It holds no domain examples; check 9 reads it.
const MANIFEST: &str = include_str!("../prompts/manifest.txt");

/// Appended to the prompt for the one repair call of an attempt.
const REPAIR: &str = include_str!("../prompts/manifest-repair.txt");

/// The "H plus System 2" mapping prompt (decision 0032). It holds no domain examples.
const MAPPING: &str = include_str!("../prompts/mapping.txt");

/// The B3 arm's mapping prompt: raw events only.
const MAPPING_RAW: &str = include_str!("../prompts/mapping-raw.txt");

/// The reply rules both mapping prompts share, filled into their `{{FORMAT}}`, so the two arms
/// are held to identical output rules.
const MAPPING_FORMAT: &str = include_str!("../prompts/mapping-format.txt");

/// Appended to either mapping prompt for the one repair call of an attempt.
const MAPPING_REPAIR: &str = include_str!("../prompts/mapping-repair.txt");

/// The hash of both prompt files, 16 lowercase hex digits. It folds into `input_hash`, so an
/// edit to either file is a new input.
pub(crate) fn prompt_hash() -> String {
    files_hash(&[MANIFEST, REPAIR])
}

/// The hash of the files behind [`mapping_prompt`] and its repair, 16 lowercase hex digits.
pub(crate) fn mapping_prompt_files_hash() -> String {
    files_hash(&[MAPPING, MAPPING_FORMAT, MAPPING_REPAIR])
}

/// The hash of the files behind [`raw_mapping_prompt`] and its repair, 16 lowercase hex digits.
pub(crate) fn raw_mapping_prompt_files_hash() -> String {
    files_hash(&[MAPPING_RAW, MAPPING_FORMAT, MAPPING_REPAIR])
}

fn files_hash(files: &[&str]) -> String {
    let mut hasher = Fnv64::new();
    for file in files {
        hasher.write_field(file.as_bytes());
    }
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
pub(crate) fn data_line<T: Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
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

/// The first call's prompt for the "H plus System 2" arm.
pub(crate) fn mapping_prompt(input: &MappingInput) -> Result<String, serde_json::Error> {
    Ok(fill(
        MAPPING,
        &[("FORMAT", MAPPING_FORMAT), ("DATA", &data_line(input)?)],
    ))
}

/// The first call's prompt for the B3 arm.
pub(crate) fn raw_mapping_prompt(input: &RawMappingInput) -> Result<String, serde_json::Error> {
    Ok(fill(
        MAPPING_RAW,
        &[("FORMAT", MAPPING_FORMAT), ("DATA", &data_line(input)?)],
    ))
}

/// The repair call's prompt: the first prompt, then the previous reply and its fault as data.
pub(crate) fn repair_prompt(
    first: &str,
    reply: &str,
    fault: &str,
) -> Result<String, serde_json::Error> {
    repair_with(REPAIR, first, reply, fault)
}

/// A mapping repair call's prompt, for either arm.
pub(crate) fn mapping_repair_prompt(
    first: &str,
    reply: &str,
    fault: &str,
) -> Result<String, serde_json::Error> {
    repair_with(MAPPING_REPAIR, first, reply, fault)
}

fn repair_with(
    template: &str,
    first: &str,
    reply: &str,
    fault: &str,
) -> Result<String, serde_json::Error> {
    let tail = fill(
        template,
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
