//! The `s2w` command-line tool.

use std::path::PathBuf;
use std::process::ExitCode;

use s2w_app::{AppError, WatchWikipediaArgs};

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
                "s2w: point it at an event stream and a world model forms.\n\nNothing runs yet: the harness is being built (gate 2).\n\nUsage: s2w --version"
            );
            ExitCode::SUCCESS
        }
        Some("watch") => watch(&args[1..]),
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
