//! The `s2w` command-line tool.

mod mcp;
pub mod output;
mod reporter;
mod serve;

use output::Format;
use std::path::PathBuf;
use std::process::ExitCode;

use reporter::JsonReporter;
use s2w_app::{AppError, HumanReporter, WatchArgs};

/// Where `s2w watch` keeps its event log when `--log-dir` is absent, relative to the working
/// directory.
const DEFAULT_LOG_DIR: &str = "./s2w-data";

const USAGE: &str = "s2w: point it at an event stream and a world model forms.\n\nUsage:\n  s2w watch <source> [--since <value>] [--log-dir <path>] [--filter <spec>]... [--json]\n      Stream events into the event log (default ./s2w-data). Restarts resume from\n      the log's stored cursor; --since sets where a log with no cursor starts,\n      and is refused once a cursor exists. --filter <path>[!]=<value> drops any\n      event whose payload does not match (repeatable, ANDed together; a preset's\n      own default filters, if any, are ANDed with these). --json prints one\n      progress object per flush on stdout (e.g. {\"appended\":1,\"duplicates\":0,\n      \"reconnects\":0,\"cursor\":\"...\",\"at\":1700000000000}), one {\"note\":\"...\"}\n      object per benign startup note, and one {\"error\":\"...\",\"fatal\":bool}\n      object per source error, both on stderr, instead of the human status\n      lines.\n\n  s2w serve <source> [--log-dir <path>] [--port <port>] [--world <name>] [--filter <spec>]...\n      Ingest and serve the live query API on 127.0.0.1 (default port 4310; 0 picks\n      a free port). Default log directory: ./s2w-data.\n      --world <name>                    world id (default: default)\n      --filter <path>[!]=<value>        as in watch, above (repeatable)\n\nSources:\n  wikipedia                            Wikipedia page changes (a named URL preset over\n                                       sse); --since takes RFC 3339 or epoch ms\n  kafka://<broker>[,<broker>...]/<topic>\n                                       every partition, no consumer group, no commits;\n                                       --since takes RFC 3339 or epoch ms\n  sse://<host>/<path>                  any Server-Sent Events stream over https\n  https://<url> | http://<url>         the same, with an explicit scheme; ids are\n                                       stored verbatim, no --since\n  -                                    newline-delimited JSON from stdin, until end\n                                       of input; no --since\n  s2w mcp [--log-dir <path>] [--world <name>]\n      Serve the read-only MCP server over stdio (add it to an MCP client with\n      `claude mcp add s2w -- s2w mcp`). Serves an empty world by default;\n      --log-dir PATH serves a one-shot snapshot of the real on-disk world at PATH,\n      read-only while s2w serve may keep writing there. --world NAME selects the\n      world id (default \"default\").\n\n  s2w --version\n  --json: JSON for --version, --help, and errors; also `s2w watch <source> --json`\n      for NDJSON progress (see above). Before `serve` or `mcp`, it selects JSON\n      rendering (`s2w --json serve ...`, `s2w --json mcp`); for mcp, only startup\n      and usage errors are affected because stdout is JSON-RPC-only once serving.\n      For serve and mcp this flag is prefix-only; unlike watch, it is not accepted\n      among the subcommand's own arguments."; // vocabulary: allow

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
        Some("watch") if format == Format::Json => output::print_error(
            format,
            "--json before 'watch' is unavailable; try: s2w watch <source> --json",
        ),
        Some("watch") => watch(&args[1..]),
        Some("serve") => serve::dispatch(&args[1..], format),
        Some("mcp") => mcp::dispatch(&args[1..], format),
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
        Err(message) => usage_error(Format::Human, message),
    }
}

/// The forms `s2w watch` accepts, for usage errors.
const STREAMS: &str =
    "s2w watch wikipedia | kafka://<broker>/<topic> | sse://<host>/<path> | https://<url> | -"; // vocabulary: allow

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
/// one value at most once; `--json` (s2w#79), a value-less flag, at most once; `--filter
/// <path>[!]=<value>` (s2w#131), repeatable.
fn parse_watch_flags(uri: String, args: &[String]) -> Result<WatchArgs, String> {
    let mut since = None;
    let mut log_dir = None;
    let mut json = false;
    let mut filters = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let name = match flag.strip_prefix("--") {
            Some(name) => name,
            None => {
                return Err(format!(
                    "unexpected argument '{flag}': expected --since, --log-dir, --filter or --json"
                ));
            }
        };
        if name == "json" {
            if json {
                return Err("--json was given more than once".to_owned());
            }
            json = true;
            index += 1;
            continue;
        }
        if name == "filter" {
            let Some(value) = args.get(index + 1) else {
                return Err("--filter needs a value: --filter <path>[!]=<value>".to_owned());
            };
            filters.push(value.clone());
            index += 2;
            continue;
        }
        let slot = match name {
            "since" => &mut since,
            "log-dir" => &mut log_dir,
            other => {
                return Err(format!(
                    "unknown flag '--{other}'. Try: --since <value>, --log-dir <path>, --filter <path>[!]=<value> or --json"
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
        json,
        filters,
    })
}

/// Runs a parsed `s2w watch` command.
fn run_watch(args: WatchArgs) -> ExitCode {
    let json = args.json;
    let outcome = if json {
        s2w_app::watch(args, &mut JsonReporter)
    } else {
        s2w_app::watch(args, &mut HumanReporter)
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if json {
                eprintln!("{}", output::render_stream_error(&error.to_string()));
            } else {
                output::print_error(Format::Human, &error.to_string());
            }
            match error {
                AppError::Usage(_) => ExitCode::from(2),
                _ => ExitCode::FAILURE,
            }
        }
    }
}

/// Prints one usage message and returns the usage exit code.
fn usage_error(format: Format, message: String) -> ExitCode {
    output::print_error(format, &message)
}

fn valid_world_name(world: &str) -> bool {
    world
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
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
    fn mcp_rejects_unknown_arguments_before_starting_the_server() {
        for extra in ["--json", "foo"] {
            assert_eq!(dispatch(args(&["mcp", extra])), ExitCode::from(2));
        }
        assert_eq!(dispatch(args(&["--json", "mcp", "foo"])), ExitCode::from(2));
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
                log_dir: PathBuf::from("./s2w-data"),
                json: false,
                filters: Vec::new()
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
                log_dir: PathBuf::from("/tmp/s2w"),
                json: false,
                filters: Vec::new()
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
                log_dir: PathBuf::from("./s2w-data"),
                json: false,
                filters: Vec::new()
            })
        );
        assert_eq!(
            parse_watch(&args(&["wikipedia", "--log-dir", "data/dir"])),
            Ok(WatchArgs {
                uri: "wikipedia".to_owned(),
                since: None,
                log_dir: PathBuf::from("data/dir"),
                json: false,
                filters: Vec::new()
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
                log_dir: PathBuf::from("k"),
                json: false,
                filters: Vec::new()
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
                log_dir: PathBuf::from("s"),
                json: false,
                filters: Vec::new()
            })
        );
    }

    #[test]
    fn watch_rejects_unknown_flags_and_bare_arguments() {
        assert!(
            parse_watch(&args(&["wikipedia", "--bogus"]))
                .is_err_and(|message| message.contains("unknown flag '--bogus'"))
        );
        assert!(
            parse_watch(&args(&["wikipedia", "extra"]))
                .is_err_and(|message| message.contains("unexpected argument 'extra'"))
        );
    }

    #[test]
    fn watch_accepts_json_alone_and_alongside_other_flags() {
        assert_eq!(
            parse_watch(&args(&["wikipedia", "--json"])),
            Ok(WatchArgs {
                uri: "wikipedia".to_owned(),
                since: None,
                log_dir: PathBuf::from("./s2w-data"),
                json: true,
                filters: Vec::new()
            })
        );
        assert_eq!(
            parse_watch(&args(&[
                "wikipedia",
                "--since",
                "123",
                "--json",
                "--log-dir",
                "data/dir"
            ])),
            Ok(WatchArgs {
                uri: "wikipedia".to_owned(),
                since: Some("123".to_owned()),
                log_dir: PathBuf::from("data/dir"),
                json: true,
                filters: Vec::new()
            })
        );
    }

    #[test]
    fn watch_rejects_json_given_more_than_once() {
        assert!(
            parse_watch(&args(&["wikipedia", "--json", "--json"]))
                .is_err_and(|message| message.contains("--json was given more than once"))
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
