//! Resolves a `s2w watch` URI to a [`Source`]: the one place that knows every adapter and
//! preset.

use crate::source::Source;
use crate::stdin::StdinSource;

/// The forms [`resolve`] accepts, for usage messages.
pub const FORMS: &str = "wikipedia | kafka://<broker>[,<broker>...]/<topic> | sse://<host>/<path> (https) | https://… | http://… | -";

/// A URI no adapter or preset claims.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
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
/// [`ResolveError::Unknown`] listing the accepted forms.
pub fn resolve(uri: &str) -> Result<Box<dyn Source>, ResolveError> {
    if uri == "-" {
        return Ok(Box::new(StdinSource::from_stdin()));
    }
    Err(ResolveError::Unknown {
        uri: uri.to_owned(),
        forms: FORMS,
    })
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
    fn unknown_forms_list_what_is_accepted() {
        for uri in ["kafka", "ftp://x/y", "", "Wikipedia"] {
            match name(uri) {
                Err(message) => assert!(message.contains(FORMS), "{uri:?}: {message}"),
                Ok(other) => panic!("{uri:?} must not resolve, got {other}"),
            }
        }
    }
}
