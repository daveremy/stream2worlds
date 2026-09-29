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
    /// Registers the `decision_record` write tool; requires `log_dir`.
    pub allow_decisions: bool,
}

/// Parses `--log-dir`, `--world` and `--allow-decisions`, each at most once.
pub(crate) fn parse(args: &[String]) -> Result<McpArgs, String> {
    let mut log_dir = None;
    let mut world = None;
    let mut allow_decisions = false;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let slot = match flag {
            "--log-dir" => &mut log_dir,
            "--world" => &mut world,
            "--allow-decisions" => {
                if allow_decisions {
                    return Err(format!("{flag} was given more than once"));
                }
                allow_decisions = true;
                index += 1;
                continue;
            }
            other => {
                return Err(format!(
                    "unexpected argument '{other}': expected --log-dir, --world or \
                     --allow-decisions"
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
    if allow_decisions && log_dir.is_none() {
        return Err("--allow-decisions needs --log-dir: decisions are appended there".to_owned());
    }
    Ok(McpArgs {
        log_dir: log_dir.map(PathBuf::from),
        world,
        allow_decisions,
    })
}

/// Parses, constructs the selected snapshot, and serves it over stdio — live-refreshing when
/// `--log-dir` names a directory (stream2worlds#128), a fixed one-shot snapshot otherwise.
pub(crate) fn dispatch(args: &[String], format: Format) -> ExitCode {
    let args = match parse(args) {
        Ok(args) => args,
        Err(message) => return usage_error(format, message),
    };
    let allow = args.allow_decisions;
    let outcome = match args.log_dir {
        Some(log_dir) => open_live(&log_dir, args.world, DEFAULT_HUB_IN_DEGREE_CAP)
            .and_then(|(state, live)| s2w_app::mcp::run_mcp_live(state, live, allow)),
        None => {
            let state =
                QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP)).with_world(args.world);
            s2w_app::mcp::run_mcp(state)
        }
    };
    match outcome {
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

/// Opens the read-only stores at `log_dir` and folds every verdict committed so far, returning
/// both the snapshot and the handle `run_mcp_live` polls to keep it current.
fn open_live(
    log_dir: &std::path::Path,
    world: String,
    hub_cap: u64,
) -> Result<(QueryState, s2w_app::mcp::replay::LiveReadOnlyWorld), AppError> {
    Ok(s2w_app::mcp::replay::LiveReadOnlyWorld::open(
        log_dir, world, hub_cap,
    )?)
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
                allow_decisions: false,
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
                allow_decisions: false,
            })
        );
        assert_eq!(
            parse(&args(&["--allow-decisions", "--log-dir", "/tmp/world"])),
            Ok(McpArgs {
                log_dir: Some(PathBuf::from("/tmp/world")),
                world: "default".to_owned(),
                allow_decisions: true,
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
            vec!["--allow-decisions"],
            vec!["--allow-decisions", "--world", "a"],
            vec!["--log-dir", "a", "--allow-decisions", "--allow-decisions"],
            vec!["--log-dir", "--allow-decisions"],
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
        let error = open_live(&missing, "default".to_owned(), DEFAULT_HUB_IN_DEGREE_CAP)
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
