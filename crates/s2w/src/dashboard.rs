//! Argument parsing and output for `s2w dashboard show` (decision 0029): the world's effective
//! dashboard manifest, read from the proposal store without creating it.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use s2w_app::query::{DashboardView, read_dashboard};

use crate::output::{self, Format};
use crate::{DEFAULT_LOG_DIR, usage_error, valid_world_name};

/// Arguments accepted after `s2w dashboard show`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShowArgs {
    log_dir: PathBuf,
    world: String,
    json: bool,
}

pub(crate) fn dispatch(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("show") => dispatch_show(&args[1..]),
        Some(other) => usage_error(
            Format::Human,
            format!("unknown 'dashboard' subcommand '{other}'. Try: dashboard show"),
        ),
        None => usage_error(
            Format::Human,
            "missing 'dashboard' subcommand. Try: dashboard show".to_owned(),
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

/// Parses `[--log-dir <path>] [--world <name>] [--json]`, each at most once.
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
        ] {
            let error = parse_show(&args(bad)).expect_err(expect);
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
