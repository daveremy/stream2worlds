//! The `s2w` command-line tool.

pub mod output;
mod serve;

use output::Format;
use std::path::PathBuf;
use std::process::ExitCode;

use s2w_app::query::{QueryState, Timeline};
use s2w_app::{AppError, DEFAULT_HUB_IN_DEGREE_CAP, WatchArgs};

/// Where `s2w watch` keeps its event log when `--log-dir` is absent, relative to the working
/// directory.
const DEFAULT_LOG_DIR: &str = "./s2w-data";

const USAGE: &str = "s2w: point it at an event stream and a world model forms.\n\nUsage:\n  s2w watch <source> [--since <value>] [--log-dir <path>]\n      Stream events into the event log (default ./s2w-data). Restarts resume from\n      the log's stored cursor; --since sets where a log with no cursor starts,\n      and is refused once a cursor exists.\n\n  s2w serve <source> [--log-dir <path>] [--port <port>] [--world <name>] [--wiki <db>]\n      Ingest and serve the live query API on 127.0.0.1 (default port 4310; 0 picks\n      a free port). Default log directory: ./s2w-data.\n      --world <name>                    world id (default: default)\n      --wiki <db>                       restrict ingestion to one wiki (wikipedia only)\n\nSources:\n  wikipedia                            Wikipedia page changes (a preset over sse);\n                                       --since takes RFC 3339 or epoch ms\n  kafka://<broker>[,<broker>...]/<topic>\n                                       every partition, no consumer group, no commits;\n                                       --since takes RFC 3339 or epoch ms\n  sse://<host>/<path>                  any Server-Sent Events stream over https\n  https://<url> | http://<url>         the same, with an explicit scheme; ids are\n                                       stored verbatim, no --since\n  -                                    newline-delimited JSON from stdin, until end\n                                       of input; no --since\n  s2w mcp\n      Serve the read-only MCP server over stdio (add it to an MCP client with\n      `claude mcp add s2w -- s2w mcp`). Serves a separate empty world with id\n      \"default\"; MCP clients must pass world: \"default\" on every tool call.\n      The live bridge feeds HTTP through s2w serve.\n\n  s2w --version\n  --json: JSON for --version, --help, and errors; unavailable for watch/mcp/serve.";

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
        Some("watch" | "mcp" | "serve") if format == Format::Json => output::print_error(
            format,
            "--json is unavailable for watch/mcp/serve. Try: s2w --help",
        ),
        Some("watch") => watch(&args[1..]),
        Some("serve") => serve::dispatch(&args[1..]),
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

/// Removes top-level --json flags, leaving watch/mcp/serve tails entirely untouched.
fn take_output_format(args: &mut Vec<String>) -> Format {
    if let Some(index) = args.iter().position(|arg| arg != "--json")
        && matches!(args[index].as_str(), "watch" | "mcp" | "serve")
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

/// The forms `s2w watch` accepts, for usage errors.
const STREAMS: &str =
    "s2w watch wikipedia | kafka://<broker>/<topic> | sse://<host>/<path> | https://<url> | -";

/// Parses everything after `s2w watch`: a source URI (resolved later by the source registry)
/// with `[--since <value>] [--log-dir <path>]`.
///
/// Pure — no filesystem or network access — so every usage error is unit-testable.
fn parse_watch(args: &[String]) -> Result<WatchArgs, String> {
    let uri = match args.first() {
        Some(uri) if !uri.starts_with("--") => uri.clone(),
        _ => return Err(format!("missing source after 'watch'. Try: {STREAMS}")),
    };
    parse_watch_flags(uri, &args[1..])
}

/// Parses the flag tail of `s2w watch <source>`: `--since` and `--log-dir`, each with exactly
/// one value, at most once each.
fn parse_watch_flags(uri: String, args: &[String]) -> Result<WatchArgs, String> {
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
        uri,
        since,
        log_dir: log_dir.map_or_else(|| PathBuf::from(DEFAULT_LOG_DIR), PathBuf::from),
    })
}

/// Runs a parsed `s2w watch` command.
fn run_watch(args: WatchArgs) -> ExitCode {
    let outcome = s2w_app::watch(args);
    match outcome {
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

    pub(super) fn args(values: &[&str]) -> Vec<String> {
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
        for values in [
            vec!["watch", "wikipedia", "--json"],
            vec!["mcp", "--json"],
            vec!["serve", "-", "--json"],
        ] {
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
                uri: "wikipedia".to_owned(),
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
                uri: "wikipedia".to_owned(),
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
                uri: "wikipedia".to_owned(),
                since: Some("123".to_owned()),
                log_dir: PathBuf::from("./s2w-data")
            })
        );
        assert_eq!(
            parse_watch(&args(&["wikipedia", "--log-dir", "data/dir"])),
            Ok(WatchArgs {
                uri: "wikipedia".to_owned(),
                since: None,
                log_dir: PathBuf::from("data/dir")
            })
        );
    }

    #[test]
    fn watch_rejects_a_missing_source() {
        for missing in [&args(&[])[..], &args(&["--since", "1"])[..]] {
            assert_eq!(
                parse_watch(missing),
                Err(format!("missing source after 'watch'. Try: {STREAMS}"))
            );
        }
    }

    #[test]
    fn watch_accepts_a_kafka_url_with_flags() {
        assert_eq!(
            parse_watch(&args(&[
                "kafka://localhost:9092/orders",
                "--since",
                "1700000000000",
                "--log-dir",
                "k"
            ])),
            Ok(WatchArgs {
                uri: "kafka://localhost:9092/orders".to_owned(),
                since: Some("1700000000000".to_owned()),
                log_dir: PathBuf::from("k")
            })
        );
    }

    #[test]
    fn watch_accepts_stdin_with_log_dir() {
        assert_eq!(
            parse_watch(&args(&["-", "--log-dir", "s"])),
            Ok(WatchArgs {
                uri: "-".to_owned(),
                since: None,
                log_dir: PathBuf::from("s")
            })
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
