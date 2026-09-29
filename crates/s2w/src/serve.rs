//! Argument parsing and exit status for the streaming HTTP command.

use std::path::PathBuf;
use std::process::ExitCode;

use s2w_app::query::{QueryState, Timeline};
use s2w_app::serve::{DEFAULT_EVERY, ServeArgs, SnapshotConfig};
use s2w_app::{AppError, DEFAULT_HUB_IN_DEGREE_CAP, HumanReporter};

use crate::output::Format;
use crate::reporter::JsonReporter;
use crate::{DEFAULT_LOG_DIR, output, usage_error, valid_world_name};

pub(super) fn dispatch(args: &[String], format: Format) -> ExitCode {
    match parse(args) {
        Ok(args) => {
            let state = QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP))
                .with_world(args.world.clone());
            let result = match format {
                Format::Human => s2w_app::serve::run_serve(state, args, &mut HumanReporter),
                Format::Json => s2w_app::serve::run_serve(state, args, &mut JsonReporter),
            };
            exit_code(result, format)
        }
        Err(message) => usage_error(format, message),
    }
}

fn exit_code(result: Result<(), AppError>, format: Format) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // A fatal `serve --json` failure matches `watch --json`'s shape (s2w#79):
            // `{"error": ..., "fatal": true}`, not the plain `{"error": ...}` object
            // `print_error` renders for a usage/parse failure.
            match format {
                Format::Json => eprintln!("{}", output::render_stream_error(&error.to_string())),
                Format::Human => {
                    output::print_error(Format::Human, &error.to_string());
                }
            }
            match error {
                AppError::Usage(_) => ExitCode::from(2),
                _ => ExitCode::FAILURE,
            }
        }
    }
}

fn parse(args: &[String]) -> Result<ServeArgs, String> {
    let uri = match args.first() {
        Some(uri) if !uri.is_empty() && !uri.starts_with("--") => uri.clone(),
        _ => return Err("missing source after 'serve'. Try: s2w serve wikipedia".to_owned()), // vocabulary: allow
    };
    let mut log_dir = None;
    let mut port = None;
    let mut world = None;
    let mut filters = Vec::new();
    let mut every = None;
    let mut no_snapshot = false;
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        if flag == "--no-snapshot" {
            if std::mem::replace(&mut no_snapshot, true) {
                return Err("--no-snapshot was given more than once".to_owned());
            }
            index += 1;
            continue;
        }
        if flag == "--filter" {
            let value = args
                .get(index + 1)
                .filter(|v| !v.trim().is_empty() && !v.starts_with("--"))
                .ok_or_else(|| "--filter needs a value: --filter <path>[!]=<value>".to_owned())?;
            filters.push(value.clone());
            index += 2;
            continue;
        }
        let slot = match flag {
            "--log-dir" => &mut log_dir,
            "--port" => &mut port,
            "--world" => &mut world,
            "--snapshot-every" => &mut every,
            other => {
                return Err(format!(
                    "unexpected argument '{other}': expected --log-dir, --port, --world, --filter, --snapshot-every or --no-snapshot"
                ));
            }
        };
        if slot.is_some() {
            return Err(format!("{flag} was given more than once"));
        }
        let value = args
            .get(index + 1)
            .filter(|v| !v.trim().is_empty() && !v.starts_with("--"))
            .ok_or_else(|| format!("{flag} needs a value: {flag} <value>"))?;
        *slot = Some(value.clone());
        index += 2;
    }
    Ok(ServeArgs {
        uri,
        world: world_name(world)?,
        log_dir: log_dir.map_or_else(|| PathBuf::from(DEFAULT_LOG_DIR), PathBuf::from),
        port: port_number(port)?,
        filters,
        snapshots: snapshot_config(every, no_snapshot)?,
    })
}

fn port_number(port: Option<String>) -> Result<u16, String> {
    port.map_or(Ok(4310), |p| {
        p.parse::<u16>()
            .map_err(|_| "--port needs an integer from 0 to 65535".to_owned())
    })
}

fn world_name(world: Option<String>) -> Result<String, String> {
    let world = world.unwrap_or_else(|| "default".to_owned());
    if !valid_world_name(&world) {
        return Err("--world must contain only ASCII letters, digits, '.', '_' or '-'".to_owned());
    }
    Ok(world)
}

fn snapshot_config(every: Option<String>, no_snapshot: bool) -> Result<SnapshotConfig, String> {
    if no_snapshot && every.is_some() {
        return Err(
            "--snapshot-every has no effect with --no-snapshot; pass one of them".to_owned(),
        );
    }
    let every = match every {
        None => DEFAULT_EVERY,
        Some(value) => match value.parse::<u64>() {
            Ok(n) if n > 0 => n,
            _ => {
                return Err(
                    "--snapshot-every needs a positive whole number of events, e.g. 1000000"
                        .to_owned(),
                );
            }
        },
    };
    Ok(SnapshotConfig {
        enabled: !no_snapshot,
        every,
        ..SnapshotConfig::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::args;

    #[test]
    fn defaults_and_explicit_flags() {
        assert_eq!(
            parse(&args(&["wikipedia"])),
            Ok(ServeArgs {
                uri: "wikipedia".to_owned(),
                world: "default".to_owned(),
                log_dir: PathBuf::from("./s2w-data"),
                port: 4310,
                filters: Vec::new(),
                snapshots: SnapshotConfig::default(),
            })
        );
        assert_eq!(
            parse(&args(&[
                "-",
                "--port",
                "0",
                "--log-dir",
                "data",
                "--world",
                "research.v2_test-1",
            ])),
            Ok(ServeArgs {
                uri: "-".to_owned(),
                world: "research.v2_test-1".to_owned(),
                log_dir: PathBuf::from("data"),
                port: 0,
                filters: Vec::new(),
                snapshots: SnapshotConfig::default(),
            })
        );
    }

    #[test]
    fn snapshot_flags() {
        let every = parse(&args(&["-", "--snapshot-every", "250"])).map(|a| a.snapshots);
        assert_eq!(
            every,
            Ok(SnapshotConfig {
                every: 250,
                ..SnapshotConfig::default()
            })
        );
        let off = parse(&args(&["-", "--no-snapshot"])).map(|a| a.snapshots);
        assert_eq!(
            off,
            Ok(SnapshotConfig {
                enabled: false,
                ..SnapshotConfig::default()
            })
        );
        assert_eq!(SnapshotConfig::default().every, DEFAULT_EVERY);
    }

    #[test]
    fn rejects_missing_duplicate_and_unknown_flags() {
        for tail in [
            vec![],
            vec!["--port", "0"],
            vec!["-", "--log-dir"],
            vec!["-", "--log-dir", "--port", "0"],
            vec!["-", "--log-dir", ""],
            vec!["-", "--port"],
            vec!["-", "--port", "65536"],
            vec!["-", "--port", "no"],
            vec!["-", "--port", "-1"],
            vec!["-", "--port", "1", "--port", "2"],
            vec!["-", "--log-dir", "a", "--log-dir", "b"],
            vec!["-", "--world", "a", "--world", "b"],
            vec!["-", "--world", ""],
            vec!["-", "--world", "a/b"],
            vec!["-", "--world", "a b"],
            vec!["-", "--since", "1"],
            vec!["-", "--json"],
            vec!["-", "extra"],
            vec!["-", "--wiki", "enwiki"],
            vec!["-", "--snapshot-every"],
            vec!["-", "--snapshot-every", "0"],
            vec!["-", "--snapshot-every", "-5"],
            vec!["-", "--snapshot-every", "many"],
            vec!["-", "--snapshot-every", "1", "--snapshot-every", "2"],
            vec!["-", "--no-snapshot", "--no-snapshot"],
            vec!["-", "--no-snapshot", "--snapshot-every", "10"],
        ] {
            assert!(parse(&args(&tail)).is_err(), "accepted {tail:?}");
        }
    }

    #[test]
    fn unknown_source_is_a_usage_error() {
        assert_eq!(
            dispatch(&args(&["unknown-source"]), Format::Human),
            ExitCode::from(2)
        );
    }

    #[test]
    fn json_prefix_reaches_serve_dispatch_and_renders_parse_errors_as_json() {
        let mut command = args(&["--json", "serve"]);
        let format = crate::take_output_format(&mut command);
        assert_eq!(format, Format::Json);
        assert_eq!(command, args(&["serve"]));
        assert_eq!(dispatch(&command[1..], format), ExitCode::from(2));
        let message = parse(&command[1..]).expect_err("missing source must fail");
        assert_eq!(
            output::render_error(format, &message),
            r#"{"error": "missing source after 'serve'. Try: s2w serve wikipedia"}"#
        );
    }

    #[test]
    fn json_suffix_is_rejected_as_an_unrecognized_serve_argument() {
        assert_eq!(
            parse(&args(&["-", "--json"])),
            Err(
                "unexpected argument '--json': expected --log-dir, --port, --world, --filter, --snapshot-every or --no-snapshot"
                    .to_owned()
            )
        );
    }

    #[test]
    fn lock_usage_errors_exit_two() {
        // App tests exercise the actual open-to-Usage mapping for both locks.
        assert_eq!(
            exit_code(
                Err(AppError::Usage("event log already open".to_owned())),
                Format::Human
            ),
            ExitCode::from(2)
        );
    }
}
