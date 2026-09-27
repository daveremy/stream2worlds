//! Resolves a `s2w watch` URI to a [`Source`]: the one place that knows every adapter and
//! preset.

use std::sync::Arc;

use crate::kafka::KafkaAdapter;
use crate::presets::preset;
use crate::source::{Source, SourceError};
use crate::sse::{Opaque, SseConfig, SseSource, USER_AGENT};
use crate::stdin::StdinSource;

/// The forms [`resolve`] accepts, for usage messages.
pub const FORMS: &str = "wikipedia | kafka://<broker>[,<broker>...]/<topic> | sse://<host>/<path> (https) | https://… | http://… | -";

/// A URI no adapter or preset claims, or one an adapter claims but cannot use.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// The adapter for this scheme refused the URI.
    #[error(transparent)]
    Invalid(#[from] SourceError),
    /// Neither a preset name, `-`, nor a known scheme.
    #[error("unknown stream {uri:?}. Try: {forms}")]
    Unknown {
        /// The URI given.
        uri: String,
        /// The accepted forms.
        forms: &'static str,
    },
}

/// Resolves `uri`: `-` is stdin, then an exact preset name, then the scheme before `://`.
///
/// # Errors
///
/// [`ResolveError::Unknown`] listing the accepted forms, or [`ResolveError::Invalid`] when the
/// scheme's adapter refuses the rest of the URI.
pub fn resolve(uri: &str) -> Result<Box<dyn Source>, ResolveError> {
    if uri == "-" {
        return Ok(Box::new(StdinSource::from_stdin()));
    }
    if let Some(source) = preset(uri) {
        return Ok(source);
    }
    match uri.split_once("://") {
        Some(("kafka", _)) => Ok(Box::new(KafkaAdapter::parse(uri)?)),
        Some(("sse", rest)) => Ok(Box::new(sse(format!("https://{rest}"))?)),
        Some(("https" | "http", _)) => Ok(Box::new(sse(uri.to_owned())?)),
        _ => Err(ResolveError::Unknown {
            uri: uri.to_owned(),
            forms: FORMS,
        }),
    }
}

/// A non-cryptographic 64-bit hash (FNV-1a). Used only to give two distinct SSE endpoints
/// distinct source ids; not a security boundary.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn sanitize(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A generic SSE source for `url` with the [`Opaque`] dialect, filed under
/// `sse.<host>[_<port>][.<path>].<hash>`.
///
/// The leading part is for readability only. The trailing 16-hex-digit `<hash>` is an FNV-1a
/// hash of the full request (scheme, host, port, path, query — everything but the fragment,
/// which is never sent to the server) and alone determines identity, so two URLs that sanitize
/// to the same readable prefix (different query strings, or path punctuation that collapses to
/// the same underscores) still get distinct source ids and never share a stored cursor.
fn sse(url: String) -> Result<SseSource, SourceError> {
    let invalid = |reason: String| SourceError::InvalidTarget {
        name: "sse",
        uri: url.clone(),
        reason,
    };
    let parsed = reqwest::Url::parse(&url).map_err(|error| invalid(error.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| invalid("the URL has no host".to_owned()))?;
    let mut readable = format!("sse.{}", sanitize(host));
    if let Some(port) = parsed.port() {
        readable.push_str(&format!("_{port}"));
    }
    let path = parsed.path().trim_matches('/');
    if !path.is_empty() {
        readable.push('.');
        readable.push_str(&sanitize(path));
    }
    // `parsed` is already percent-encoded, so every character here is ASCII and a byte-offset
    // truncation is always a char boundary.
    const HASH_HEX_LEN: usize = 16;
    const MAX_READABLE: usize = 128 - HASH_HEX_LEN - 1; // leave room for ".<hash>"
    readable.truncate(MAX_READABLE.min(readable.len()));
    // Canonical identity: everything but the fragment, which the server never sees.
    let canonical = parsed.as_str().split('#').next().unwrap_or(parsed.as_str());
    let source_id = format!("{readable}.{:016x}", fnv1a64(canonical.as_bytes()));
    Ok(SseSource::new(SseConfig {
        name: "sse",
        url,
        dialect: Arc::new(Opaque),
        source_id,
        user_agent: USER_AGENT,
    }))
}

#[cfg(test)]
mod tests {
    use super::{FORMS, resolve};

    fn name(uri: &str) -> Result<&'static str, String> {
        resolve(uri)
            .map(|source| source.name())
            .map_err(|error| error.to_string())
    }

    #[test]
    fn dash_is_stdin() {
        assert_eq!(name("-"), Ok("stdin"));
    }

    #[test]
    fn kafka_scheme_is_the_kafka_adapter() {
        assert_eq!(name("kafka://b:9092/t"), Ok("kafka"));
        match name("kafka://b:9092") {
            Err(message) => assert!(message.contains("invalid target"), "{message}"),
            Ok(other) => panic!("a topicless kafka URI must be refused, got {other}"),
        }
    }

    #[test]
    fn presets_resolve_by_exact_name() {
        assert_eq!(name("wikipedia"), Ok("wikipedia"));
    }

    #[test]
    fn sse_schemes_are_the_generic_sse_adapter() {
        for uri in [
            "sse://stream.example.org/v2/recent",
            "https://stream.example.org/v2/recent",
            "http://localhost:8080/events",
        ] {
            assert_eq!(name(uri), Ok("sse"), "{uri}");
        }
    }

    #[test]
    fn sse_source_ids_are_derived_from_host_and_path() {
        let id = |uri: &str| super::sse(uri.to_owned()).map(|source| source.source_id().to_owned());
        let a = id("https://stream.example.org/v2/recent%20changes").expect("valid");
        assert!(
            a.starts_with("sse.stream.example.org.v2_recent_20changes."),
            "got {a:?}"
        );
        let b = id("http://localhost:8080/").expect("valid");
        assert!(b.starts_with("sse.localhost_8080."), "got {b:?}");
        let long = format!("https://h/{}", "p".repeat(300));
        assert_eq!(id(&long).ok().map(|id| id.len()), Some(128));
    }

    #[test]
    fn sse_source_ids_never_collide_on_query_or_sanitized_punctuation() {
        let id = |uri: &str| {
            super::sse(uri.to_owned())
                .expect("valid")
                .source_id()
                .to_owned()
        };
        // Different query strings on the same path must not share an id.
        assert_ne!(
            id("https://h/events?stream=a"),
            id("https://h/events?stream=b")
        );
        // Path punctuation that sanitizes to the same readable prefix must not share an id.
        assert_ne!(id("https://h/a/b"), id("https://h/a_b"));
    }

    #[test]
    fn unknown_forms_list_what_is_accepted() {
        for uri in ["kafka", "ftp://x/y", "", "Wikipedia"] {
            match name(uri) {
                Err(message) => assert!(message.contains(FORMS), "{uri:?}: {message}"),
                Ok(other) => panic!("{uri:?} must not resolve, got {other}"),
            }
        }
    }
}
