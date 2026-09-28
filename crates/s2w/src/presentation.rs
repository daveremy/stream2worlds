//! Argument parsing and startup for `s2w presentation set|show` (stream2worlds#144).

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use s2w_log::{ReadOnlySqliteEventLog, SqliteEventLog, WorldPresentation, WorldPresentationInput};
use s2w_model::Timestamp;

use crate::output::{self, Format};
use crate::{DEFAULT_LOG_DIR, usage_error, valid_world_name};

/// Arguments accepted after `s2w presentation set`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SetArgs {
    log_dir: PathBuf,
    world: String,
    file: PathBuf,
}

/// Arguments accepted after `s2w presentation show`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShowArgs {
    log_dir: PathBuf,
    world: String,
    json: bool,
}

/// Dispatches `s2w presentation set|show`.
pub(crate) fn dispatch(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("set") => dispatch_set(&args[1..]),
        Some("show") => dispatch_show(&args[1..]),
        Some(other) => usage_error(
            Format::Human,
            format!(
                "unknown 'presentation' subcommand '{other}'. Try: presentation set | presentation show"
            ),
        ),
        None => usage_error(
            Format::Human,
            "missing 'presentation' subcommand. Try: presentation set | presentation show"
                .to_owned(),
        ),
    }
}

fn dispatch_set(args: &[String]) -> ExitCode {
    let args = match parse_set(args) {
        Ok(args) => args,
        Err(message) => return usage_error(Format::Human, message),
    };
    if let Err(message) = run_set(&args) {
        output::print_error(Format::Human, &message);
        return ExitCode::FAILURE;
    }
    println!("presentation set for world '{}'", args.world);
    ExitCode::SUCCESS
}

fn dispatch_show(args: &[String]) -> ExitCode {
    let args = match parse_show(args) {
        Ok(args) => args,
        Err(message) => return usage_error(Format::Human, message),
    };
    match run_show(&args) {
        Ok(rendered) => {
            println!("{rendered}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            output::print_error(Format::Human, &message);
            ExitCode::FAILURE
        }
    }
}

/// Parses `--log-dir <path> --world <name> --file <path>`, each at most once, all required.
fn parse_set(args: &[String]) -> Result<SetArgs, String> {
    let mut log_dir = None;
    let mut world = None;
    let mut file = None;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let slot = match flag {
            "--log-dir" => &mut log_dir,
            "--world" => &mut world,
            "--file" => &mut file,
            other => {
                return Err(format!(
                    "unexpected argument '{other}': expected --log-dir, --world or --file"
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
    let log_dir = log_dir.ok_or_else(|| "--log-dir is required: --log-dir <path>".to_owned())?;
    let world = world.unwrap_or_else(|| "default".to_owned());
    if !valid_world_name(&world) {
        return Err("--world must contain only ASCII letters, digits, '.', '_' or '-'".to_owned());
    }
    let file = file.ok_or_else(|| "--file is required: --file <presentation.json>".to_owned())?;
    Ok(SetArgs {
        log_dir: PathBuf::from(log_dir),
        world,
        file: PathBuf::from(file),
    })
}

/// Parses `--log-dir <path> --world <name> [--json]`; `--log-dir` defaults to
/// [`DEFAULT_LOG_DIR`], `--world` to `"default"`.
fn parse_show(args: &[String]) -> Result<ShowArgs, String> {
    let mut log_dir = None;
    let mut world = None;
    let mut json = false;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        if flag == "--json" {
            if json {
                return Err("--json was given more than once".to_owned());
            }
            json = true;
            index += 1;
            continue;
        }
        let slot = match flag {
            "--log-dir" => &mut log_dir,
            "--world" => &mut world,
            other => {
                return Err(format!(
                    "unexpected argument '{other}': expected --log-dir, --world or --json"
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
    Ok(ShowArgs {
        log_dir: log_dir.map_or_else(|| PathBuf::from(DEFAULT_LOG_DIR), PathBuf::from),
        world,
        json,
    })
}

/// Reads `args.file`, validates it against the strict CLI input type, and appends it as the
/// new presentation record for `args.world`. Refuses (via [`WorldPresentation::set`]) a world
/// with no manifest.
fn run_set(args: &SetArgs) -> Result<(), String> {
    let contents = std::fs::read_to_string(&args.file)
        .map_err(|error| format!("failed to read {}: {error}", args.file.display()))?;
    let input: WorldPresentationInput = serde_json::from_str(&contents).map_err(|error| {
        format!(
            "invalid presentation JSON in {}: {error}",
            args.file.display()
        )
    })?;
    let presentation = WorldPresentation::from(input);
    let mut log = SqliteEventLog::open(&args.log_dir).map_err(|error| error.to_string())?;
    WorldPresentation::set(&mut log, &args.world, &presentation, now())
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Loads the latest presentation record for `args.world`, rendering it as pretty JSON
/// (`--json`) or a debug-formatted summary (human, default). `None` (never set) renders
/// distinctly from an empty record either way.
fn run_show(args: &ShowArgs) -> Result<String, String> {
    let log = ReadOnlySqliteEventLog::open(&args.log_dir).map_err(|error| error.to_string())?;
    let presentation = log
        .world_presentation(&args.world)
        .map_err(|error| error.to_string())?;
    Ok(render(presentation.as_ref(), args.json))
}

fn render(presentation: Option<&WorldPresentation>, json: bool) -> String {
    match (presentation, json) {
        (Some(presentation), true) => serde_json::to_string_pretty(presentation)
            .unwrap_or_else(|error| format!("{{\"error\": \"{error}\"}}")),
        (None, true) => "null".to_owned(),
        (Some(presentation), false) => format!("{presentation:#?}"),
        (None, false) => "no presentation set for this world".to_owned(),
    }
}

/// The current time, for [`WorldPresentation::set`]'s `created_at`. Mirrors the
/// `s2w-sources` crate's own now-from-`SystemTime` helper (no shared crate exports one).
fn now() -> Timestamp {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            let millis = i64::try_from(duration.as_millis()).unwrap_or(i64::MAX);
            Timestamp::from_millis(millis)
        }
        Err(error) => {
            let millis = i64::try_from(error.duration().as_millis()).unwrap_or(i64::MAX);
            Timestamp::from_millis(millis.saturating_neg())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::args;

    #[test]
    fn set_requires_log_dir_and_file() {
        assert!(
            parse_set(&args(&["--world", "w"]))
                .is_err_and(|message| message.contains("--log-dir is required"))
        );
        assert!(
            parse_set(&args(&["--log-dir", "d", "--world", "w"]))
                .is_err_and(|message| message.contains("--file is required"))
        );
    }

    #[test]
    fn set_parses_all_flags() {
        assert_eq!(
            parse_set(&args(&[
                "--log-dir",
                "d",
                "--world",
                "w",
                "--file",
                "p.json"
            ])),
            Ok(SetArgs {
                log_dir: PathBuf::from("d"),
                world: "w".to_owned(),
                file: PathBuf::from("p.json"),
            })
        );
    }

    #[test]
    fn set_rejects_invalid_world_name() {
        assert!(
            parse_set(&args(&[
                "--log-dir",
                "d",
                "--world",
                "w/x",
                "--file",
                "p.json"
            ]))
            .is_err_and(|message| message.contains("--world must contain only"))
        );
    }

    #[test]
    fn set_rejects_duplicate_and_unknown_flags() {
        assert!(
            parse_set(&args(&["--log-dir", "d", "--log-dir", "d2"]))
                .is_err_and(|message| message.contains("was given more than once"))
        );
        assert!(
            parse_set(&args(&["--bogus", "x"]))
                .is_err_and(|message| message.contains("unexpected argument '--bogus'"))
        );
    }

    #[test]
    fn show_defaults_log_dir_and_world() {
        assert_eq!(
            parse_show(&[]),
            Ok(ShowArgs {
                log_dir: PathBuf::from(DEFAULT_LOG_DIR),
                world: "default".to_owned(),
                json: false,
            })
        );
    }

    #[test]
    fn show_parses_json_flag_and_explicit_values() {
        assert_eq!(
            parse_show(&args(&["--log-dir", "d", "--world", "w", "--json"])),
            Ok(ShowArgs {
                log_dir: PathBuf::from("d"),
                world: "w".to_owned(),
                json: true,
            })
        );
    }

    #[test]
    fn show_rejects_json_given_more_than_once() {
        assert!(
            parse_show(&args(&["--json", "--json"]))
                .is_err_and(|message| message.contains("--json was given more than once"))
        );
    }

    #[test]
    fn render_distinguishes_absent_from_present() {
        assert_eq!(render(None, false), "no presentation set for this world");
        assert_eq!(render(None, true), "null");
        let presentation = WorldPresentation::default();
        assert!(render(Some(&presentation), true).contains("\"title\": null"));
    }

    #[test]
    fn dispatch_rejects_unknown_and_missing_subcommand() {
        assert_eq!(dispatch(&args(&["bogus"])), ExitCode::from(2));
        assert_eq!(dispatch(&[]), ExitCode::from(2));
    }
}
