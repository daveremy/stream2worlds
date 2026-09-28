//! Argument parsing and exit status for the streaming HTTP command.

use std::path::PathBuf;
use std::process::ExitCode;

use s2w_app::query::{QueryState, Timeline};
use s2w_app::serve::ServeArgs;
use s2w_app::{AppError, DEFAULT_HUB_IN_DEGREE_CAP};

use crate::{DEFAULT_LOG_DIR, output, usage_error};

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    match parse(args) {
        Ok(args) => {
            let state = QueryState::new(Timeline::new(DEFAULT_HUB_IN_DEGREE_CAP));
            exit_code(s2w_app::serve::run_serve(state, args))
        }
        Err(message) => usage_error(message),
    }
}

fn exit_code(result: Result<(), AppError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            output::print_error(output::Format::Human, &error.to_string());
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
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        let slot = match flag {
            "--log-dir" => &mut log_dir,
            "--port" => &mut port,
            "--wiki" => &mut wiki,
            other => {
                return Err(format!(
                    "unexpected argument '{other}': expected --log-dir, --port or --wiki"
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
                log_dir: PathBuf::from("./s2w-data"),
                port: 4310,
                wiki: None,
            })
        );
        assert_eq!(
            parse(&args(&["-", "--port", "0", "--log-dir", "data"])),
            Ok(ServeArgs {
                uri: "-".to_owned(),
                log_dir: PathBuf::from("data"),
                port: 0,
                wiki: None,
            })
        );
        assert_eq!(
            parse(&args(&["wikipedia", "--wiki", "enwiki"])),
            Ok(ServeArgs {
                uri: "wikipedia".to_owned(),
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
    fn unknown_source_and_json_prefix_are_usage_errors() {
        assert_eq!(dispatch(&args(&["unknown-source"])), ExitCode::from(2));
        assert_eq!(
            crate::dispatch(args(&["--json", "serve", "-"])),
            ExitCode::from(2)
        );
    }

    #[test]
    fn lock_usage_errors_exit_two() {
        // App tests exercise the actual open-to-Usage mapping for both locks.
        assert_eq!(
            exit_code(Err(AppError::Usage("event log already open".to_owned()))),
            ExitCode::from(2)
        );
    }
}
