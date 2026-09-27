//! The `s2w` command-line tool.

use std::path::PathBuf;
use std::process::ExitCode;

use s2w_app::query::{QueryState, Timeline};
use s2w_app::{AppError, DEFAULT_HUB_IN_DEGREE_CAP, WatchArgs};

/// Where `s2w watch` keeps its event log when `--log-dir` is absent, relative to the working
/// directory.
const DEFAULT_LOG_DIR: &str = "./s2w-data";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("s2w {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            println!(
                "s2w: point it at an event stream and a world model forms.\n\nUsage:\n  s2w watch <source> [--since <value>] [--log-dir <path>]\n      Stream events into the event log (default ./s2w-data). Restarts resume from\n      the log's stored cursor; --since sets where a log with no cursor starts,\n      and is refused once a cursor exists.\n\nSources:\n  wikipedia                            Wikipedia page changes (a preset over sse);\n                                       --since takes ISO-8601\n  kafka://<broker>[,<broker>...]/<topic>\n                                       every partition, no consumer group, no commits;\n                                       --since takes RFC 3339 or epoch ms\n  sse://<host>/<path>                  any Server-Sent Events stream over https\n  https://<url> | http://<url>         the same, with an explicit scheme; ids are\n                                       stored verbatim, no --since\n  -                                    newline-delimited JSON from stdin, until end\n                                       of input; no --since\n  s2w mcp\n      Serve the read-only MCP server over stdio (add it to an MCP client with\n      `claude mcp add s2w -- s2w mcp`). Serves an empty world until the live\n      event-log bridge lands.\n\n  s2w --version"
            );
            ExitCode::SUCCESS
        }
        Some("watch") => watch(&args[1..]),
        Some("mcp") => run_mcp(),
        Some(other) => {
            eprintln!("s2w: unknown argument '{other}'. Try: s2w --help");
            ExitCode::from(2)
        }
    }
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
            eprintln!("s2w: {error}");
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
            eprintln!("s2w: {error}");
            match error {
                AppError::Usage(_) => ExitCode::from(2),
                _ => ExitCode::FAILURE,
            }
        }
    }
}

/// Prints one usage message and returns the usage exit code.
fn usage_error(message: String) -> ExitCode {
    eprintln!("s2w: {message}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
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
