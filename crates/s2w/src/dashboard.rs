//! Argument parsing and output for `s2w dashboard show` (decision 0029): the world's effective
//! dashboard manifest, read from the proposal store without creating it; and for `s2w dashboard
//! propose` (s2w#301): the deterministic proposer's manifest, or with `--system2-*` a model
//! command's (s2w#311), filed with a policy decision.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use s2w_app::dashboard::{ProposeReport, System2Command, propose_fallback, propose_system2};
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
    /// The model command; `None` runs the deterministic proposer.
    system2: Option<System2Command>,
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
    let result = match &args.system2 {
        Some(command) => propose_system2(&args.log_dir, &args.world, command, args.dry_run),
        None => propose_fallback(&args.log_dir, &args.world, args.dry_run),
    };
    match result {
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

/// Parses `--log-dir <path> [--world <name>] [--dry-run] [--json]`, each at most once, plus the
/// `--system2-*` flags (see [`split_system2`]). The log directory is required: this command
/// writes.
fn parse_propose(args: &[String]) -> Result<ProposeArgs, String> {
    let (rest, system2) = split_system2(args)?;
    let parsed = parse_flags(&rest, true)?;
    let log_dir = parsed
        .log_dir
        .ok_or_else(|| "--log-dir is required: --log-dir <path>".to_owned())?;
    Ok(ProposeArgs {
        log_dir: PathBuf::from(log_dir),
        world: parsed.world,
        json: parsed.json,
        dry_run: parsed.dry_run,
        system2,
    })
}

/// Takes `--system2-cmd <program> [<arg>...] [--]`, `--system2-model <model>/<version>` and
/// `--system2-env <name>` (repeatable) out of `args`, returning the other arguments.
///
/// `--system2-cmd` takes every token after it up to a standalone `--` or the end, so the
/// command's own flags are never read as ours; it therefore cannot pass a literal `--`. The
/// model is split at its last `/` (a model name may hold one). `--system2-cmd` and
/// `--system2-model` go together, and `--system2-env` needs them.
fn split_system2(args: &[String]) -> Result<(Vec<String>, Option<System2Command>), String> {
    let mut rest = Vec::new();
    let mut flags = System2Flags::default();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        index += match flag {
            "--system2-cmd" => flags.command(&args[index + 1..])?,
            "--system2-model" | "--system2-env" => {
                let value = args
                    .get(index + 1)
                    .filter(|value| !value.trim().is_empty() && !value.starts_with("--"))
                    .ok_or_else(|| format!("{flag} needs a value: {flag} <value>"))?;
                flags.value(flag, value)?;
                2
            }
            _ => {
                rest.push(args[index].clone());
                1
            }
        };
    }
    Ok((rest, flags.finish()?))
}

/// The `--system2-*` flags seen so far.
#[derive(Default)]
struct System2Flags {
    argv: Option<Vec<String>>,
    model: Option<(String, String)>,
    env: Vec<String>,
}

impl System2Flags {
    /// Takes `--system2-cmd`'s tokens from `tail` (the arguments after the flag) and returns
    /// how many arguments that consumed, the flag and a closing `--` included.
    fn command(&mut self, tail: &[String]) -> Result<usize, String> {
        if self.argv.is_some() {
            return Err("--system2-cmd was given more than once".to_owned());
        }
        let end = tail
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(tail.len());
        if end == 0 {
            return Err(
                "--system2-cmd needs a command: --system2-cmd <program> [<arg>...] [--]".to_owned(),
            );
        }
        self.argv = Some(tail[..end].to_vec());
        Ok(end + 2)
    }

    /// Takes `--system2-model <value>` or `--system2-env <value>`.
    fn value(&mut self, flag: &str, value: &str) -> Result<(), String> {
        if flag == "--system2-model" {
            if self.model.is_some() {
                return Err(format!("{flag} was given more than once"));
            }
            self.model = Some(split_model(value)?);
            return Ok(());
        }
        if value.contains(['=', '\0']) {
            return Err(format!("{flag} takes a variable name, not '{value}'"));
        }
        if self.env.iter().any(|name| name == value) {
            return Err(format!("{flag} {value} was given more than once"));
        }
        self.env.push(value.to_owned());
        Ok(())
    }

    /// The command, when `--system2-cmd` and `--system2-model` were both given.
    fn finish(self) -> Result<Option<System2Command>, String> {
        match (self.argv, self.model) {
            (Some(argv), Some((model, version))) => Ok(Some(System2Command {
                argv,
                model,
                version,
                env: self.env,
            })),
            (Some(_), None) => Err("--system2-cmd needs --system2-model <model>/<version>".into()),
            (None, Some(_)) => Err("--system2-model needs --system2-cmd".to_owned()),
            (None, None) if !self.env.is_empty() => {
                Err("--system2-env needs --system2-cmd".to_owned())
            }
            (None, None) => Ok(None),
        }
    }
}

/// `<model>/<version>`, split at the last `/`; both parts non-empty.
fn split_model(value: &str) -> Result<(String, String), String> {
    match value.rsplit_once('/') {
        Some((model, version)) if !model.is_empty() && !version.is_empty() => {
            Ok((model.to_owned(), version.to_owned()))
        }
        _ => Err(format!(
            "--system2-model takes <model>/<version>, not '{value}'"
        )),
    }
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
                system2: None,
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
    fn system2_cmd_takes_every_token_to_a_standalone_double_dash() {
        let parsed = parse_propose(&args(&[
            "--system2-env",
            "HOME",
            "--log-dir",
            "d",
            "--system2-model",
            "vendor/model-x/2026-09",
            "--system2-cmd",
            "/usr/bin/cli",
            "-p",
            "--json",
            "--world",
            "x",
            "--",
            "--json",
            "--system2-env",
            "PATH",
        ]))
        .expect("parses");
        assert!(parsed.json, "--json after `--` is ours");
        assert_eq!(
            parsed.world, "default",
            "--world before `--` is the command's"
        );
        assert_eq!(
            parsed.system2,
            Some(System2Command {
                argv: args(&["/usr/bin/cli", "-p", "--json", "--world", "x"]),
                model: "vendor/model-x".to_owned(),
                version: "2026-09".to_owned(),
                env: args(&["HOME", "PATH"]),
            })
        );

        let last = parse_propose(&args(&[
            "--log-dir",
            "d",
            "--system2-model",
            "m/v",
            "--system2-cmd",
            "sh",
            "-c",
            "cat",
        ]))
        .expect("parses");
        let command = last.system2.expect("system2");
        assert_eq!(command.argv, args(&["sh", "-c", "cat"]));
        assert!(command.env.is_empty());
    }

    #[test]
    fn system2_flags_refuse_bad_and_partial_arguments() {
        for (bad, expect) in [
            (&["--system2-cmd"][..], "needs a command"),
            (&["--system2-cmd", "--"][..], "needs a command"),
            (&["--system2-cmd", "a"][..], "needs --system2-model"),
            (&["--system2-model", "m/v"][..], "needs --system2-cmd"),
            (&["--system2-env", "HOME"][..], "needs --system2-cmd"),
            (&["--system2-model", "m"][..], "<model>/<version>"),
            (&["--system2-model", "m/"][..], "<model>/<version>"),
            (&["--system2-model", "/v"][..], "<model>/<version>"),
            (&["--system2-model"][..], "needs a value"),
            (&["--system2-env", "--json"][..], "needs a value"),
            (&["--system2-env", "A=B"][..], "variable name"),
            (
                &["--system2-env", "A", "--system2-env", "A"][..],
                "more than once",
            ),
            (
                &["--system2-model", "m/v", "--system2-model", "m/v"][..],
                "more than once",
            ),
            (
                &[
                    "--system2-model",
                    "m/v",
                    "--system2-cmd",
                    "a",
                    "--",
                    "--system2-cmd",
                    "b",
                ][..],
                "more than once",
            ),
            (
                &["--system2-model", "m/v", "--system2-cmd", "a", "--", "--"][..],
                "unexpected argument '--'",
            ),
        ] {
            let mut full = args(&["--log-dir", "d"]);
            full.extend(args(bad));
            let error = parse_propose(&full).expect_err(expect);
            assert!(error.contains(expect), "{full:?}: {error}");
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
