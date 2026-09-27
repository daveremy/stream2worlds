//! The `s2w` command-line tool.

pub mod output;

use output::Format;
use std::path::PathBuf;
use std::process::ExitCode;

use s2w_app::query::{QueryState, Timeline};
use s2w_app::{AppError, DEFAULT_HUB_IN_DEGREE_CAP, WatchWikipediaArgs};

/// Where `s2w watch` keeps its event log when `--log-dir` is absent, relative to the working
/// directory.
const DEFAULT_LOG_DIR: &str = "./s2w-data";

const USAGE: &str = "s2w: point it at an event stream and a world model forms.\n\nUsage:\n  s2w watch wikipedia [--since <ISO-8601>] [--log-dir <path>]\n      Stream Wikipedia page changes into the event log (default ./s2w-data).\n      Restarts resume from the log's stored cursor; --since replays history\n      into a log that has no cursor yet.\n  s2w mcp\n      Serve the read-only MCP server over stdio (add it to an MCP client with\n      `claude mcp add s2w -- s2w mcp`). Serves an empty world until the live\n      event-log bridge lands.\n  s2w --version\n  --json: JSON for --version, --help, and errors; unavailable for watch/mcp.";

fn main() -> ExitCode {
    dispatch(std::env::args().skip(1).collect())
}

fn dispatch(mut args: Vec<String>) -> ExitCode {
    let format = take_output_format(&mut args);
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            output::print_version(format, env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            output::print_usage(format, USAGE);
            ExitCode::SUCCESS
        }
        Some("watch" | "mcp") if format == Format::Json => output::print_error(
            format,
            "--json is unavailable for watch/mcp. Try: s2w --help",
        ),
        Some("watch") => watch(&args[1..]),
        Some("mcp") => match args.get(1) {
            Some(other) => output::print_error(
                format,
                &format!("unexpected argument '{other}': mcp takes no arguments. Try: s2w mcp"),
            ),
            None => run_mcp(),
        },
        Some(other) => output::print_error(
            format,
            &format!("unknown argument '{other}'. Try: s2w --help"),
        ),
    }
}

/// Removes top-level --json flags, leaving watch/mcp tails entirely untouched.
fn take_output_format(args: &mut Vec<String>) -> Format {
    if let Some(index) = args.iter().position(|arg| arg != "--json")
        && matches!(args[index].as_str(), "watch" | "mcp")
    {
        // A prefix requests JSON, but the subcommand's own arguments stay opaque.
        args.drain(..index);
        return if index == 0 {
            Format::Human
        } else {
            Format::Json
        };
    }
    let mut format = Format::Human;
    args.retain(|arg| {
        if arg == "--json" {
            format = Format::Json;
            false
        } else {
            true
        }
    });
    format
}

/// Dispatches `s2w watch <stream> [flags]`.
fn watch(args: &[String]) -> ExitCode {
    match parse_watch(args) {
        Ok(parsed) => run_watch(parsed),
        Err(message) => usage_error(message),
    }
}

/// The parsed `s2w watch` arguments, shaped for `s2w_app`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WatchArgs {
    since: Option<String>,
    log_dir: PathBuf,
}

/// Parses everything after `s2w watch`: `wikipedia [--since <value>] [--log-dir <path>]`.
///
/// Pure — no filesystem or network access — so every usage error is unit-testable.
fn parse_watch(args: &[String]) -> Result<WatchArgs, String> {
    match args.first().map(String::as_str) {
        Some("wikipedia") => parse_watch_flags(&args[1..]),
        Some(other) => Err(format!(
            "unknown stream '{other}'. Try: s2w watch wikipedia"
        )),
        None => Err("missing stream name after 'watch'. Try: s2w watch wikipedia".to_owned()),
    }
}

/// Parses the flag tail of `s2w watch wikipedia`: `--since` and `--log-dir`, each with exactly
/// one value, at most once each.
fn parse_watch_flags(args: &[String]) -> Result<WatchArgs, String> {
    let mut since = None;
    let mut log_dir = None;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let name = match flag.strip_prefix("--") {
            Some(name) => name,
            None => {
                return Err(format!(
                    "unexpected argument '{flag}': expected --since or --log-dir"
                ));
            }
        };
        let slot = match name {
            "since" => &mut since,
            "log-dir" => &mut log_dir,
            other => {
                return Err(format!(
                    "unknown flag '--{other}'. Try: --since <value> or --log-dir <path>"
                ));
            }
        };
        if slot.is_some() {
            return Err(format!("--{name} was given more than once"));
        }
        let Some(value) = args.get(index + 1) else {
            return Err(format!("--{name} needs a value: --{name} <value>"));
        };
        *slot = Some(value.clone());
        index += 2;
    }
    Ok(WatchArgs {
        since,
        log_dir: log_dir.map_or_else(|| PathBuf::from(DEFAULT_LOG_DIR), PathBuf::from),
    })
}

/// Runs a parsed `s2w watch wikipedia` command.
fn run_watch(args: WatchArgs) -> ExitCode {
    let args = WatchWikipediaArgs {
        since: args.since,
        log_dir: args.log_dir,
    };
    match s2w_app::watch_wikipedia(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            output::print_error(Format::Human, &error.to_string());
            match error {
                AppError::Usage(_) => ExitCode::from(2),
                _ => ExitCode::FAILURE,
            }
        }
    }
}

/// Serves the read-only query tools over stdio until the MCP client disconnects.
fn run_mcp() -> ExitCode {
    let state = QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP));
    match s2w_app::mcp::run_mcp(state) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            output::print_error(Format::Human, &error.to_string());
            match error {
                AppError::Usage(_) => ExitCode::from(2),
                _ => ExitCode::FAILURE,
            }
        }
    }
}

/// Prints one usage message and returns the usage exit code.
fn usage_error(message: String) -> ExitCode {
    output::print_error(Format::Human, &message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn json_before_or_after_version_selects_json() {
        for values in [["--json", "--version"], ["--version", "--json"]] {
            let mut arguments = args(&values);
            assert_eq!(take_output_format(&mut arguments), Format::Json);
            assert_eq!(arguments, args(&["--version"]));
        }
    }

    #[test]
    fn json_help_and_top_level_errors_select_json() {
        for values in [
            vec!["--json"],
            vec!["--help", "--json"],
            vec!["--json", "-h"],
            vec!["unknown", "--json"],
            vec!["--json", "unknown"],
        ] {
            let mut arguments = args(&values);
            assert_eq!(take_output_format(&mut arguments), Format::Json);
            assert!(!arguments.iter().any(|arg| arg == "--json"));
        }
        let mut arguments = args(&["--json"]);
        take_output_format(&mut arguments);
        assert!(arguments.is_empty()); // Dispatches to the same usage branch as --help.
    }

    #[test]
    fn json_detection_preserves_subcommand_tails() {
        for values in [vec!["watch", "wikipedia", "--json"], vec!["mcp", "--json"]] {
            let mut arguments = args(&values);
            assert_eq!(take_output_format(&mut arguments), Format::Human);
            assert_eq!(arguments, args(&values));
            arguments.insert(0, "--json".to_owned());
            assert_eq!(take_output_format(&mut arguments), Format::Json);
            assert_eq!(arguments, args(&values));
        }
    }

    #[test]
    fn mcp_rejects_all_arguments_before_starting_the_server() {
        // The extra-argument branch only calls print_error (stderr); run_mcp and
        // every stdout writer are in separate branches, so no protocol can start.
        for extra in ["--json", "foo"] {
            assert_eq!(dispatch(args(&["mcp", extra])), ExitCode::from(2));
        }
        assert_eq!(dispatch(args(&["--json", "mcp"])), ExitCode::from(2));
    }

    #[test]
    fn json_prefix_is_unavailable_for_watch() {
        assert_eq!(
            dispatch(args(&["--json", "watch", "wikipedia"])),
            ExitCode::from(2)
        );
    }

    #[test]
    fn watch_defaults_to_the_local_data_directory() {
        assert_eq!(
            parse_watch(&args(&["wikipedia"])),
            Ok(WatchArgs {
                since: None,
                log_dir: PathBuf::from("./s2w-data")
            })
        );
    }

    #[test]
    fn watch_accepts_since_and_log_dir_together() {
        assert_eq!(
            parse_watch(&args(&[
                "wikipedia",
                "--since",
                "2026-09-27T12:00:00Z",
                "--log-dir",
                "/tmp/s2w"
            ])),
            Ok(WatchArgs {
                since: Some("2026-09-27T12:00:00Z".to_owned()),
                log_dir: PathBuf::from("/tmp/s2w")
            })
        );
    }

    #[test]
    fn watch_accepts_each_flag_alone() {
        assert_eq!(
            parse_watch(&args(&["wikipedia", "--since", "123"])),
            Ok(WatchArgs {
                since: Some("123".to_owned()),
                log_dir: PathBuf::from("./s2w-data")
            })
        );
        assert_eq!(
            parse_watch(&args(&["wikipedia", "--log-dir", "data/dir"])),
            Ok(WatchArgs {
                since: None,
                log_dir: PathBuf::from("data/dir")
            })
        );
    }

    #[test]
    fn watch_rejects_unknown_and_missing_stream_names() {
        assert_eq!(
            parse_watch(&args(&["kafka"])),
            Err("unknown stream 'kafka'. Try: s2w watch wikipedia".to_owned())
        );
        assert_eq!(
            parse_watch(&[]),
            Err("missing stream name after 'watch'. Try: s2w watch wikipedia".to_owned())
        );
    }

    #[test]
    fn watch_rejects_unknown_flags_and_bare_arguments() {
        assert!(
            parse_watch(&args(&["wikipedia", "--json"]))
                .is_err_and(|message| message.contains("unknown flag '--json'"))
        );
        assert!(
            parse_watch(&args(&["wikipedia", "extra"]))
                .is_err_and(|message| message.contains("unexpected argument 'extra'"))
        );
    }

    #[test]
    fn watch_rejects_flags_without_values_or_given_twice() {
        assert!(
            parse_watch(&args(&["wikipedia", "--since"]))
                .is_err_and(|message| message.contains("--since needs a value"))
        );
        assert!(
            parse_watch(&args(&["wikipedia", "--log-dir"]))
                .is_err_and(|message| message.contains("--log-dir needs a value"))
        );
        assert!(
            parse_watch(&args(&["wikipedia", "--since", "1", "--since", "2"]))
                .is_err_and(|message| message.contains("--since was given more than once"))
        );
    }
}
