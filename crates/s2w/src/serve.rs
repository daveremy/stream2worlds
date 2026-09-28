//! Argument parsing and exit status for the streaming HTTP command.

use std::path::PathBuf;
use std::process::ExitCode;

use s2w_app::query::{QueryState, Timeline};
use s2w_app::serve::ServeArgs;
use s2w_app::{AppError, DEFAULT_HUB_IN_DEGREE_CAP, HumanReporter};

use crate::output::Format;
use crate::reporter::JsonReporter;
use crate::{DEFAULT_LOG_DIR, output, usage_error};

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
        _ => return Err("missing source after 'serve'. Try: s2w serve wikipedia".to_owned()),
    };
    let mut log_dir = None;
    let mut port = None;
    let mut wiki = None;
    let mut world = None;
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        let slot = match flag {
            "--log-dir" => &mut log_dir,
            "--port" => &mut port,
            "--wiki" => &mut wiki,
            "--world" => &mut world,
            other => {
                return Err(format!(
                    "unexpected argument '{other}': expected --log-dir, --port, --wiki or --world"
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
    let world = world.unwrap_or_else(|| "default".to_owned());
    if !world
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("--world must contain only ASCII letters, digits, '.', '_' or '-'".to_owned());
    }
    Ok(ServeArgs {
        uri,
        world,
        log_dir: log_dir.map_or_else(|| PathBuf::from(DEFAULT_LOG_DIR), PathBuf::from),
        port: port.map_or(Ok(4310), |p| {
            p.parse::<u16>()
                .map_err(|_| "--port needs an integer from 0 to 65535".to_owned())
        })?,
        wiki,
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
                wiki: None,
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
                wiki: None,
            })
        );
        assert_eq!(
            parse(&args(&["wikipedia", "--wiki", "enwiki"])),
            Ok(ServeArgs {
                uri: "wikipedia".to_owned(),
                world: "default".to_owned(),
                log_dir: PathBuf::from("./s2w-data"),
                port: 4310,
                wiki: Some("enwiki".to_owned()),
            })
        );
        assert_eq!(
            parse(&args(&[
                "wikipedia",
                "--wiki",
                "enwiki",
                "--world",
                "research.v2_test-1"
            ])),
            Ok(ServeArgs {
                uri: "wikipedia".to_owned(),
                world: "research.v2_test-1".to_owned(),
                log_dir: PathBuf::from("./s2w-data"),
                port: 4310,
                wiki: Some("enwiki".to_owned()),
            })
        );
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
            vec!["-", "--wiki"],
            vec!["-", "--wiki", ""],
            vec!["-", "--wiki", "  "],
            vec!["-", "--wiki", "enwiki", "--wiki", "dewiki"],
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
                "unexpected argument '--json': expected --log-dir, --port, --wiki or --world"
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
