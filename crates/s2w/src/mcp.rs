//! Argument parsing and startup for the read-only MCP command.

use std::path::PathBuf;
use std::process::ExitCode;

use s2w_app::query::{QueryState, Timeline};
use s2w_app::{AppError, DEFAULT_HUB_IN_DEGREE_CAP};

use crate::output::Format;
use crate::{output, usage_error, valid_world_name};

/// Arguments accepted after `s2w mcp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpArgs {
    /// Existing directory whose event and verdict databases are replayed once at startup.
    pub log_dir: Option<PathBuf>,
    /// The world identifier MCP callers must supply.
    pub world: String,
}

/// Parses `--log-dir` and `--world`, each at most once.
pub(crate) fn parse(args: &[String]) -> Result<McpArgs, String> {
    let mut log_dir = None;
    let mut world = None;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let slot = match flag {
            "--log-dir" => &mut log_dir,
            "--world" => &mut world,
            other => {
                return Err(format!(
                    "unexpected argument '{other}': expected --log-dir or --world"
                ));
            }
        };
        if slot.is_some() {
            return Err(format!("{flag} was given more than once"));
        }
        let value = args
            .get(index + 1)
            .filter(|value| !value.trim().is_empty() && !value.starts_with("--"))
            .ok_or_else(|| format!("{flag} needs a value: {flag} <value>"))?;
        *slot = Some(value.clone());
        index += 2;
    }
    let world = world.unwrap_or_else(|| "default".to_owned());
    if !valid_world_name(&world) {
        return Err("--world must contain only ASCII letters, digits, '.', '_' or '-'".to_owned());
    }
    Ok(McpArgs {
        log_dir: log_dir.map(PathBuf::from),
        world,
    })
}

/// Parses, constructs the selected snapshot, and serves it over stdio.
pub(crate) fn dispatch(args: &[String], format: Format) -> ExitCode {
    let args = match parse(args) {
        Ok(args) => args,
        Err(message) => return usage_error(format, message),
    };
    match state(args).and_then(s2w_app::mcp::run_mcp) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => exit_code(error, format),
    }
}

/// Renders one fatal mcp failure (a snapshot that could not be built, or a server that stopped)
/// and picks its exit code — the same split `serve::exit_code` makes.
fn exit_code(error: AppError, format: Format) -> ExitCode {
    let message = error.to_string();
    match format {
        // A fatal `mcp --json` failure matches `watch --json`'s shape (s2w#79):
        // `{"error": ..., "fatal": true}`, not the plain `{"error": ...}` object
        // `print_error` renders for a usage/parse failure.
        Format::Json => eprintln!("{}", output::render_stream_error(&message)),
        Format::Human => {
            output::print_error(Format::Human, &message);
        }
    }
    match error {
        AppError::Usage(_) => ExitCode::from(2),
        _ => ExitCode::FAILURE,
    }
}

fn state(args: McpArgs) -> Result<QueryState, AppError> {
    match args.log_dir {
        Some(log_dir) => {
            let hub_cap = usize::try_from(DEFAULT_HUB_IN_DEGREE_CAP).map_err(|error| {
                AppError::Usage(format!(
                    "the default hub cap does not fit this platform: {error}"
                ))
            })?;
            Ok(s2w_app::mcp::replay::read_only_world(
                &log_dir, args.world, hub_cap,
            )?)
        }
        None => {
            Ok(QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP)).with_world(args.world))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::args;

    #[test]
    fn defaults_and_explicit_flags() {
        assert_eq!(
            parse(&[]),
            Ok(McpArgs {
                log_dir: None,
                world: "default".to_owned(),
            })
        );
        assert_eq!(
            parse(&args(&[
                "--log-dir",
                "/tmp/world",
                "--world",
                "research.v2_test-1",
            ])),
            Ok(McpArgs {
                log_dir: Some(PathBuf::from("/tmp/world")),
                world: "research.v2_test-1".to_owned(),
            })
        );
    }

    #[test]
    fn rejects_missing_duplicate_unknown_and_invalid_flags() {
        for tail in [
            vec!["--log-dir"],
            vec!["--log-dir", ""],
            vec!["--log-dir", "a", "--log-dir", "b"],
            vec!["--world"],
            vec!["--world", ""],
            vec!["--world", "a", "--world", "b"],
            vec!["--world", "bad/world"],
            vec!["--world", "é"],
            vec!["foo"],
            vec!["--port", "1"],
        ] {
            assert!(parse(&args(&tail)).is_err(), "accepted {tail:?}");
        }
    }

    #[test]
    fn missing_directory_is_fatal_and_names_path_in_both_output_formats() {
        let missing = std::env::temp_dir().join(format!(
            "s2w-mcp-missing-{}-definitely-absent",
            std::process::id()
        ));
        let error = state(McpArgs {
            log_dir: Some(missing.clone()),
            world: "default".to_owned(),
        })
        .err()
        .expect("a missing read-only world must fail");
        let message = error.to_string();
        assert!(
            message.contains(&missing.display().to_string()),
            "{message}"
        );
        assert!(!message.contains("CANTOPEN"), "{message}");
        // What `exit_code` renders per format: `print_error`'s object for human usage output,
        // the fatal stream-error shape for --json.
        for rendered in [
            output::render_error(Format::Human, &message),
            output::render_stream_error(&message),
        ] {
            assert!(
                rendered.contains(&missing.display().to_string()),
                "{rendered}"
            );
            assert!(!rendered.contains("CANTOPEN"), "{rendered}");
        }
        assert_eq!(exit_code(error, Format::Human), ExitCode::FAILURE);
    }

    #[test]
    fn usage_errors_exit_two_and_other_failures_exit_one() {
        assert_eq!(
            exit_code(AppError::Usage("bad flag".to_owned()), Format::Human),
            ExitCode::from(2)
        );
        assert_eq!(
            exit_code(
                AppError::ReadOnlyWorld(s2w_app::mcp::replay::ReadOnlyWorldError::Corrupt(
                    "store ahead of log".to_owned()
                )),
                Format::Human
            ),
            ExitCode::FAILURE
        );
    }
}
