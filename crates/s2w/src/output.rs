//! Human and JSON output for the command-line interface.

use std::process::ExitCode;

/// The requested top-level output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Plain text for terminal users.
    Human,
    /// A single JSON object per output.
    Json,
}

/// Renders the version without a trailing newline.
pub fn render_version(format: Format, version: &str) -> String {
    match format {
        Format::Human => format!("s2w {version}"),
        Format::Json => format!("{{\"version\": {}}}", json_string(version)),
    }
}

/// Renders the supplied help text without a trailing newline.
pub fn render_usage(format: Format, text: &str) -> String {
    match format {
        Format::Human => text.to_owned(),
        Format::Json => format!("{{\"usage\": {}}}", json_string(text)),
    }
}

/// Renders an error, preserving any next-step hint in the message.
pub fn render_error(format: Format, message: &str) -> String {
    match format {
        Format::Human => format!("s2w: {message}"),
        Format::Json => format!("{{\"error\": {}}}", json_string(message)),
    }
}

/// Prints the version to stdout, followed by a newline.
pub fn print_version(format: Format, version: &str) {
    println!("{}", render_version(format, version));
}

/// Prints help to stdout, followed by a newline.
pub fn print_usage(format: Format, text: &str) {
    println!("{}", render_usage(format, text));
}

/// Renders the fatal error that stops `s2w watch <source> --json` (s2w#79): the one place a
/// `watch --json` run's own top-level failure becomes an object, matching the shape its
/// in-stream reports already use (`s2w_app::group_commit`'s `JsonReporter`, a separate crate
/// that cannot reach this seam — see that module's doc comment).
pub fn render_stream_error(message: &str) -> String {
    format!("{{\"error\": {}, \"fatal\": true}}", json_string(message))
}

/// Prints an error to stderr and returns the usage error exit code, 2.
pub fn print_error(format: Format, message: &str) -> ExitCode {
    eprintln!("{}", render_error(format, message));
    ExitCode::from(2)
}

/// Quotes a JSON string, escaping every U+0000 through U+001F control character.
fn json_string(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut quoted = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\u{08}' => quoted.push_str("\\b"),
            '\u{0c}' => quoted.push_str("\\f"),
            '\u{00}'..='\u{1f}' => {
                quoted.push_str("\\u00");
                quoted.push(char::from(HEX[(ch as usize) >> 4]));
                quoted.push(char::from(HEX[(ch as usize) & 0xf]));
            }
            _ => quoted.push(ch),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_version_in_both_formats() {
        assert_eq!(render_version(Format::Human, "1.2.3"), "s2w 1.2.3");
        assert_eq!(
            render_version(Format::Json, "1.2.3"),
            r#"{"version": "1.2.3"}"#
        );
    }

    #[test]
    fn renders_usage_in_both_formats() {
        assert_eq!(render_usage(Format::Human, crate::USAGE), crate::USAGE);
        assert_eq!(
            render_usage(Format::Json, "Usage:"),
            r#"{"usage": "Usage:"}"#
        );
    }

    #[test]
    fn renders_errors_with_the_hint_in_both_formats() {
        let message = "unknown argument 'foo'. Try: s2w --help";
        assert_eq!(
            render_error(Format::Human, message),
            format!("s2w: {message}")
        );
        assert_eq!(
            render_error(Format::Json, message),
            r#"{"error": "unknown argument 'foo'. Try: s2w --help"}"#
        );
        assert_eq!(
            render_error(Format::Json, "bad \"path\" \\ Try:\nhelp"),
            r#"{"error": "bad \"path\" \\ Try:\nhelp"}"#
        );
    }

    #[test]
    fn escapes_quotes_backslashes_controls_and_preserves_unicode() {
        assert_eq!(
            json_string("\"\\\n\r\t\u{08}\u{0c}\0\u{1f} `é🦀`"),
            r#""\"\\\n\r\t\b\f\u0000\u001f `é🦀`""#
        );
        for control in '\0'..='\u{1f}' {
            assert!(!json_string(&control.to_string()).contains(control));
        }
    }

    #[test]
    fn actual_multiline_help_round_trips_as_a_json_string() {
        let rendered = render_usage(Format::Json, crate::USAGE);
        assert!(!rendered.chars().any(|ch| ch <= '\u{1f}'));
        let value = rendered
            .strip_prefix("{\"usage\": \"")
            .and_then(|text| text.strip_suffix("\"}"))
            .expect("usage must be a JSON object containing a quoted string");
        let mut decoded = String::new();
        let mut chars = value.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                decoded.push(match chars.next() {
                    Some('n') => '\n',
                    Some('"') => '"',
                    Some('\\') => '\\',
                    other => panic!("unexpected escape in help text: {other:?}"),
                });
            } else {
                assert_ne!(ch, '"', "JSON string must not contain an unescaped quote");
                decoded.push(ch);
            }
        }
        assert_eq!(decoded, crate::USAGE);
        assert!(decoded.contains('\n'));
        assert!(decoded.contains('`'));
    }
}
