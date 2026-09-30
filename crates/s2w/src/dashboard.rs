//! Argument parsing and output for `s2w dashboard show` (decision 0029): the world's effective
//! dashboard manifest, read from the proposal store without creating it; and for `s2w dashboard
//! propose` (s2w#301): the deterministic proposer's manifest, filed with a policy decision.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use s2w_app::dashboard::{ProposeReport, propose_fallback};
use s2w_app::query::{DashboardView, QueryError, read_dashboard};

use crate::output::{self, Format};
use crate::{DEFAULT_LOG_DIR, usage_error, valid_world_name};

/// Arguments accepted after `s2w dashboard show`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShowArgs {
    log_dir: PathBuf,
    world: String,
    json: bool,
}

/// Arguments accepted after `s2w dashboard propose`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProposeArgs {
    log_dir: PathBuf,
    world: String,
    json: bool,
    dry_run: bool,
}

pub(crate) fn dispatch(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("show") => dispatch_show(&args[1..]),
        Some("propose") => dispatch_propose(&args[1..]),
        Some(other) => usage_error(
            Format::Human,
            format!(
                "unknown 'dashboard' subcommand '{other}'. Try: dashboard show, dashboard propose"
            ),
        ),
        None => usage_error(
            Format::Human,
            "missing 'dashboard' subcommand. Try: dashboard show, dashboard propose".to_owned(),
        ),
    }
}

fn dispatch_show(args: &[String]) -> ExitCode {
    let args = match parse_show(args) {
        Ok(args) => args,
        Err(message) => return usage_error(Format::Human, message),
    };
    let format = if args.json {
        Format::Json
    } else {
        Format::Human
    };
    match read_dashboard(&args.log_dir, &args.world) {
        Ok(view) => {
            println!("{}", render(&args.world, &view, args.json));
            ExitCode::SUCCESS
        }
        Err(error) => {
            output::print_error(format, &error.to_string());
            ExitCode::FAILURE
        }
    }
}

fn dispatch_propose(args: &[String]) -> ExitCode {
    let args = match parse_propose(args) {
        Ok(args) => args,
        Err(message) => return usage_error(Format::Human, message),
    };
    match propose_fallback(&args.log_dir, &args.world, args.dry_run) {
        Ok(report) => {
            println!("{}", render_report(&report, args.json));
            ExitCode::SUCCESS
        }
        Err(error) => propose_failure(args.json, &args.log_dir, &error),
    }
}

fn propose_failure(json: bool, log_dir: &Path, error: &QueryError) -> ExitCode {
    if json {
        eprintln!("{}", error.json_body());
        return ExitCode::FAILURE;
    }
    let hint = match error {
        QueryError::StoreLocked => Some(format!(
            "retry once the other writer on {} is done",
            log_dir.display()
        )),
        _ => None,
    };
    output::print_failure(error.code(), &error.to_string(), hint.as_deref())
}

/// Parses `[--log-dir <path>] [--world <name>] [--json]`, each at most once.
fn parse_show(args: &[String]) -> Result<ShowArgs, String> {
    let parsed = parse_flags(args, false)?;
    Ok(ShowArgs {
        log_dir: parsed
            .log_dir
            .map_or_else(|| PathBuf::from(DEFAULT_LOG_DIR), PathBuf::from),
        world: parsed.world,
        json: parsed.json,
    })
}

/// Parses `--log-dir <path> [--world <name>] [--dry-run] [--json]`, each at most once. The log
/// directory is required: this command writes.
fn parse_propose(args: &[String]) -> Result<ProposeArgs, String> {
    let parsed = parse_flags(args, true)?;
    let log_dir = parsed
        .log_dir
        .ok_or_else(|| "--log-dir is required: --log-dir <path>".to_owned())?;
    Ok(ProposeArgs {
        log_dir: PathBuf::from(log_dir),
        world: parsed.world,
        json: parsed.json,
        dry_run: parsed.dry_run,
    })
}

struct Flags {
    log_dir: Option<String>,
    world: String,
    json: bool,
    dry_run: bool,
}

/// The flags `show` and `propose` share, plus `--dry-run` when `dry_run_allowed`.
fn parse_flags(args: &[String], dry_run_allowed: bool) -> Result<Flags, String> {
    let mut log_dir = None;
    let mut world = None;
    let mut json = false;
    let mut dry_run = false;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let switch = match flag {
            "--json" => Some(&mut json),
            "--dry-run" if dry_run_allowed => Some(&mut dry_run),
            _ => None,
        };
        if let Some(switch) = switch {
            if *switch {
                return Err(format!("{flag} was given more than once"));
            }
            *switch = true;
            index += 1;
            continue;
        }
        let slot = match flag {
            "--log-dir" => &mut log_dir,
            "--world" => &mut world,
            other => {
                let expected = if dry_run_allowed {
                    "--log-dir, --world, --dry-run or --json"
                } else {
                    "--log-dir, --world or --json"
                };
                return Err(format!(
                    "unexpected argument '{other}': expected {expected}"
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
    Ok(Flags {
        log_dir,
        world,
        json,
        dry_run,
    })
}

/// `--json` prints the report as JSON; otherwise one line per fact.
fn render_report(report: &ProposeReport, json: bool) -> String {
    if json {
        return serde_json::to_string(report)
            .unwrap_or_else(|error| format!("{{\"error\": \"{error}\"}}"));
    }
    let action = serde_json::to_value(report.action)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut out = format!(
        "dashboard propose: world '{}', proposer {}, input {}: {action}",
        report.world, report.actor, report.input_hash
    );
    if let (Some(id), Some(attempt)) = (&report.proposal_id, report.attempt) {
        let _ = write!(out, "\n  proposal {id}, attempt {attempt}");
    }
    if let (Some(decision), Some(basis)) = (&report.decision, &report.basis) {
        let _ = write!(out, "\n  policy {decision}: {basis}");
    }
    if let Some(reason) = &report.reason {
        let _ = write!(out, "\n  {reason}");
    }
    if report.envelope.is_some() {
        let _ = write!(
            out,
            "\n  dry run: nothing written; --json prints the envelope"
        );
    }
    out
}

/// `--json` prints the query API's exact bytes; otherwise a short summary.
fn render(world: &str, view: &DashboardView, json: bool) -> String {
    if json {
        return serde_json::to_string(view)
            .unwrap_or_else(|error| format!("{{\"error\": \"{error}\"}}"));
    }
    let mut out = String::new();
    match (&view.manifest, &view.proposal_id, &view.identity) {
        (Some(manifest), Some(proposal_id), Some(identity)) => {
            let actor = view
                .actor
                .as_ref()
                .map_or_else(|| "unknown".to_owned(), crate::proposals::actor_text);
            let default_role = manifest
                .roles
                .iter()
                .find(|role| role.default)
                .map_or("none", |role| role.name.as_str());
            let _ = writeln!(
                out,
                "dashboard: world '{world}' uses proposal {proposal_id} by {actor}, identity \
                 {identity}"
            );
            let _ = writeln!(
                out,
                "  domain: {}: {}",
                manifest.domain.name, manifest.domain.summary
            );
            let _ = writeln!(
                out,
                "  projection: {}; {} role(s), default '{default_role}'; {} type row(s)",
                manifest.quintessential_projection.template.as_str(),
                manifest.roles.len(),
                manifest.types.len()
            );
            if view.stale {
                let _ = writeln!(out, "  stale: the current mappings lack:");
                for entry in &view.stale_entries {
                    let _ = writeln!(out, "    {entry}");
                }
            }
        }
        _ => {
            let _ = writeln!(out, "dashboard: no manifest in effect for world '{world}'");
        }
    }
    for row in &view.excluded {
        let _ = writeln!(
            out,
            "dashboard: proposal {} is excluded: {}",
            row.proposal_id, row.reason
        );
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn show_parses_defaults_and_refuses_bad_arguments() {
        assert_eq!(
            parse_show(&[]),
            Ok(ShowArgs {
                log_dir: PathBuf::from(DEFAULT_LOG_DIR),
                world: "default".to_owned(),
                json: false,
            })
        );
        let parsed = parse_show(&args(&["--log-dir", "d", "--world", "w", "--json"]));
        assert_eq!(
            parsed,
            Ok(ShowArgs {
                log_dir: PathBuf::from("d"),
                world: "w".to_owned(),
                json: true,
            })
        );
        for (bad, expect) in [
            (&["--json", "--json"][..], "more than once"),
            (&["--world", "a", "--world", "b"][..], "more than once"),
            (&["--world"][..], "needs a value"),
            (&["--world", "a b"][..], "--world must contain"),
            (&["--file", "x"][..], "unexpected argument"),
            (&["--dry-run"][..], "unexpected argument"),
        ] {
            let error = parse_show(&args(bad)).expect_err(expect);
            assert!(error.contains(expect), "{bad:?}: {error}");
        }
    }

    #[test]
    fn propose_needs_a_log_dir_and_takes_dry_run_once() {
        assert_eq!(
            parse_propose(&args(&["--log-dir", "d", "--dry-run"])),
            Ok(ProposeArgs {
                log_dir: PathBuf::from("d"),
                world: "default".to_owned(),
                json: false,
                dry_run: true,
            })
        );
        for (bad, expect) in [
            (&[][..], "--log-dir is required"),
            (
                &["--log-dir", "d", "--dry-run", "--dry-run"][..],
                "more than once",
            ),
            (
                &["--log-dir", "d", "--file", "x"][..],
                "--dry-run or --json",
            ),
        ] {
            let error = parse_propose(&args(bad)).expect_err(expect);
            assert!(error.contains(expect), "{bad:?}: {error}");
        }
    }

    #[test]
    fn an_empty_view_says_no_manifest_and_json_is_the_route_body() {
        let view = DashboardView::default();
        assert_eq!(
            render("w", &view, false),
            "dashboard: no manifest in effect for world 'w'"
        );
        assert_eq!(
            render("w", &view, true),
            r#"{"manifest":null,"proposal_id":null,"actor":null,"identity":null,"stale":false,"stale_entries":[],"excluded":[]}"#
        );
    }
}
