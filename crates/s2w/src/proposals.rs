//! Argument parsing and rendering for `s2w proposals list|grade|decide` (stream2worlds#185).
//!
//! Every read and write goes through `s2w_app::proposals`, the one service MCP
//! `decision_record` also calls; this module only parses flags and renders results.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use s2w_app::proposals::{
    RouteAfter, Seat, check_reviewer, parse_outcome, read_view, record_decision, route_after,
};
use s2w_app::query::{ActorDto, DecisionDto, GradeDto, ProposalDto, QueryError, TallyDto};
use s2w_app::routes;
use s2w_log::Outcome;

use crate::output::{self, Format};
use crate::{DEFAULT_LOG_DIR, usage_error};

/// Arguments accepted after `s2w proposals list` and `s2w proposals grade`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadArgs {
    log_dir: PathBuf,
    json: bool,
}

/// Arguments accepted after `s2w proposals decide`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DecideArgs {
    log_dir: PathBuf,
    proposal: String,
    outcome: Outcome,
    basis: String,
    reviewer: String,
    json: bool,
}

/// Dispatches `s2w proposals list|grade|decide`.
pub(crate) fn dispatch(args: &[String]) -> ExitCode {
    const TRY: &str = "Try: proposals list | proposals grade | proposals decide";
    match args.first().map(String::as_str) {
        Some("list") => match parse_read(&args[1..]) {
            Ok(args) => run_list(&args),
            Err(message) => usage_error(Format::Human, message),
        },
        Some("grade") => match parse_read(&args[1..]) {
            Ok(args) => run_grade(&args),
            Err(message) => usage_error(Format::Human, message),
        },
        Some("decide") => match parse_decide(&args[1..]) {
            Ok(args) => run_decide(&args),
            Err(message) => usage_error(Format::Human, message),
        },
        Some(other) => usage_error(
            Format::Human,
            format!("unknown 'proposals' subcommand '{other}'. {TRY}"),
        ),
        None => usage_error(
            Format::Human,
            format!("missing 'proposals' subcommand. {TRY}"),
        ),
    }
}

/// Parsed flags: each `--<name> <value>` seen, and whether `--json` was given.
struct Flags {
    values: Vec<(&'static str, String)>,
    json: bool,
}

impl Flags {
    /// Removes and returns the value given for `--<name>`, if any.
    fn take(&mut self, name: &str) -> Option<String> {
        let index = self.values.iter().position(|(seen, _)| *seen == name)?;
        Some(self.values.swap_remove(index).1)
    }
}

/// Collects `--<name> <value>` flags (each at most once, from `allowed`) and a value-less
/// `--json` (at most once). A value may not be blank or start with `--`.
fn parse_flags(args: &[String], allowed: &[&'static str]) -> Result<Flags, String> {
    let expected = allowed
        .iter()
        .map(|name| format!("--{name}"))
        .chain(std::iter::once("--json".to_owned()))
        .collect::<Vec<_>>()
        .join(", ");
    let mut values: Vec<(&'static str, String)> = Vec::new();
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
        let Some(name) = flag
            .strip_prefix("--")
            .and_then(|name| allowed.iter().copied().find(|allowed| *allowed == name))
        else {
            return Err(format!("unexpected argument '{flag}': expected {expected}"));
        };
        if values.iter().any(|(seen, _)| *seen == name) {
            return Err(format!("{flag} was given more than once"));
        }
        let value = args
            .get(index + 1)
            .filter(|value| !value.trim().is_empty() && !value.starts_with("--"))
            .ok_or_else(|| format!("{flag} needs a value: {flag} <value>"))?;
        values.push((name, value.clone()));
        index += 2;
    }
    Ok(Flags { values, json })
}

/// Parses `[--log-dir <path>] [--json]`; `--log-dir` defaults to [`DEFAULT_LOG_DIR`].
fn parse_read(args: &[String]) -> Result<ReadArgs, String> {
    let mut flags = parse_flags(args, &["log-dir"])?;
    Ok(ReadArgs {
        log_dir: flags
            .take("log-dir")
            .map_or_else(|| PathBuf::from(DEFAULT_LOG_DIR), PathBuf::from),
        json: flags.json,
    })
}

/// Parses `--log-dir <path> --proposal <id> --outcome accept|reject --basis <text>
/// --reviewer <id> [--json]`. Every value flag is required: a write never lands in a default
/// directory by accident.
fn parse_decide(args: &[String]) -> Result<DecideArgs, String> {
    let mut flags = parse_flags(
        args,
        &["log-dir", "proposal", "outcome", "basis", "reviewer"],
    )?;
    let mut required = |name: &str, shape: &str| {
        flags
            .take(name)
            .ok_or_else(|| format!("--{name} is required: --{name} {shape}"))
    };
    let log_dir = required("log-dir", "<path>")?;
    let proposal = required("proposal", "<id>")?;
    let outcome = required("outcome", "accept|reject")?;
    let basis = required("basis", "<text>")?;
    let reviewer = required("reviewer", "<id>")?;
    let outcome = parse_outcome(&outcome).map_err(|error| error.to_string())?;
    check_reviewer(&reviewer).map_err(|error| error.to_string())?;
    Ok(DecideArgs {
        log_dir: PathBuf::from(log_dir),
        proposal,
        outcome,
        basis,
        reviewer,
        json: flags.json,
    })
}

/// Prints a data error (exit 1): under `--json`, the `{"error", "message"}` body HTTP and MCP
/// serve for the same error; otherwise one line with a next step where there is one.
fn failure(json: bool, log_dir: &Path, error: &QueryError) -> ExitCode {
    if json {
        eprintln!("{}", error.json_body());
        return ExitCode::FAILURE;
    }
    let hint = match error {
        QueryError::UnknownProposal { .. } => Some(format!(
            "s2w proposals list --log-dir {}",
            log_dir.display()
        )),
        _ => None,
    };
    output::print_failure(error.code(), &error.to_string(), hint.as_deref())
}

fn run_list(args: &ReadArgs) -> ExitCode {
    let view = match read_view(&args.log_dir) {
        Ok(view) => view,
        Err(error) => return failure(args.json, &args.log_dir, &error),
    };
    if args.json {
        return print_json(&args.log_dir, serde_json::to_string(&view));
    }
    if view.proposals.is_empty() {
        println!("no proposals in {}", args.log_dir.display());
        return ExitCode::SUCCESS;
    }
    // Resolve routes before printing anything, so a failed read never leaves partial output.
    let resolution = match routes::load(&args.log_dir) {
        Ok(resolution) => resolution,
        Err(error) => {
            return failure(
                args.json,
                &args.log_dir,
                &QueryError::Storage(error.to_string()),
            );
        }
    };
    for proposal in &view.proposals {
        println!("{}", proposal_line(proposal));
        for decision in view
            .decisions
            .iter()
            .filter(|decision| decision.proposal_id == proposal.id)
        {
            println!("  {}", decision_line(decision));
        }
    }
    for line in routes::report_lines(&resolution) {
        println!("{line}");
    }
    ExitCode::SUCCESS
}

fn run_grade(args: &ReadArgs) -> ExitCode {
    let view = match read_view(&args.log_dir) {
        Ok(view) => view,
        Err(error) => return failure(args.json, &args.log_dir, &error),
    };
    if args.json {
        return print_json(&args.log_dir, serde_json::to_string(&view.grades));
    }
    if view.grades.is_empty() {
        println!("no proposals in {}", args.log_dir.display());
    }
    for grade in &view.grades {
        println!("{}", grade_line(grade));
    }
    ExitCode::SUCCESS
}

fn run_decide(args: &DecideArgs) -> ExitCode {
    let seat = Seat::Human {
        reviewer: args.reviewer.clone(),
    };
    let recorded = match record_decision(
        &args.log_dir,
        &seat,
        &args.proposal,
        args.outcome,
        &args.basis,
    ) {
        Ok(recorded) => recorded,
        Err(error) => return failure(args.json, &args.log_dir, &error),
    };
    // The decision is stored: a failed route read-back is a warning, never exit 1, so nobody
    // retries a write that already happened.
    let route = recorded.mapping_source.as_ref().and_then(|source| {
        match route_after(&args.log_dir, source) {
            Ok(route) => Some(route),
            Err(error) => {
                eprintln!(
                    "s2w: warning: decision recorded, but reading the route back failed: {error}"
                );
                None
            }
        }
    });
    if args.json {
        let body = serde_json::json!({ "decision": recorded.decision, "route": route });
        println!("{body}");
        return ExitCode::SUCCESS;
    }
    for line in decide_lines(&recorded.decision, route.as_ref()) {
        println!("{line}");
    }
    ExitCode::SUCCESS
}

/// Prints an already-serialized value; `serde_json` is called at each site so this crate needs
/// no direct `serde` dependency.
fn print_json(log_dir: &Path, rendered: serde_json::Result<String>) -> ExitCode {
    match rendered {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(error) => failure(
            true,
            log_dir,
            &QueryError::Storage(format!("could not render JSON: {error}")),
        ),
    }
}

fn actor_text(actor: &ActorDto) -> String {
    match actor {
        ActorDto::Human { id } => format!("human {id}"),
        ActorDto::Agent { model, version } => format!("agent {model}@{version}"),
    }
}

fn tally_text(tally: &TallyDto) -> String {
    format!("{}/{}", tally.accepted, tally.rejected)
}

fn proposal_line(proposal: &ProposalDto) -> String {
    format!(
        "proposal {} {} class {} by {} at {} ms (log offset {})",
        proposal.seq,
        proposal.id,
        proposal.class,
        actor_text(&proposal.actor),
        proposal.proposed_at_ms,
        proposal.snapshot_offset
    )
}

fn decision_line(decision: &DecisionDto) -> String {
    format!(
        "decision {}: {} {} at {} ms, basis: {}",
        decision.seq, decision.decider, decision.outcome, decision.decided_at_ms, decision.basis
    )
}

/// One line per (class, actor); every tally is `accepted/rejected`.
fn grade_line(grade: &GradeDto) -> String {
    format!(
        "{} {}: proposed {}, ungraded {}, human {}, evidence {}, agent {}, policy accepted {} \
         rejected {}, policy-applied {} (ungraded {})",
        grade.class,
        actor_text(&grade.actor),
        grade.proposed,
        grade.ungraded,
        tally_text(&grade.human),
        tally_text(&grade.evidence),
        tally_text(&grade.agent),
        grade.policy_accepted,
        grade.policy_rejected,
        tally_text(&grade.policy_applied),
        grade.policy_applied_ungraded
    )
}

fn decide_lines(decision: &DecisionDto, route: Option<&RouteAfter>) -> Vec<String> {
    let mut lines = vec![format!(
        "recorded decision {} on proposal {}: {} {}, basis: {}",
        decision.seq, decision.proposal_id, decision.decider, decision.outcome, decision.basis
    )];
    if let Some(route) = route {
        lines.push(match (&route.mapping, &route.proposal_id) {
            (Some(mapping), Some(proposal)) => format!(
                "source '{}' now runs mapping {mapping} from proposal {proposal}",
                route.source
            ),
            _ => format!("source '{}' is now unrouted", route.source),
        });
        lines.push("a running `s2w serve` picks this up at its next start".to_owned());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::args;

    fn decide(extra: &[&str]) -> Result<DecideArgs, String> {
        let mut values = vec![
            "--log-dir",
            "d",
            "--proposal",
            "p-1",
            "--outcome",
            "accept",
            "--basis",
            "looks right",
            "--reviewer",
            "dave",
        ];
        values.extend_from_slice(extra);
        parse_decide(&args(&values))
    }

    #[test]
    fn reads_default_to_the_local_data_directory() {
        assert_eq!(
            parse_read(&[]),
            Ok(ReadArgs {
                log_dir: PathBuf::from("./s2w-data"),
                json: false
            })
        );
        assert_eq!(
            parse_read(&args(&["--json", "--log-dir", "x"])),
            Ok(ReadArgs {
                log_dir: PathBuf::from("x"),
                json: true
            })
        );
    }

    #[test]
    fn reads_refuse_unknown_repeated_and_valueless_flags() {
        for (values, needle) in [
            (vec!["--world", "w"], "unexpected argument '--world'"),
            (vec!["extra"], "unexpected argument 'extra'"),
            (vec!["--json", "--json"], "--json was given more than once"),
            (
                vec!["--log-dir", "a", "--log-dir", "b"],
                "--log-dir was given more than once",
            ),
            (vec!["--log-dir"], "--log-dir needs a value"),
            (vec!["--log-dir", "--json"], "--log-dir needs a value"),
        ] {
            assert!(
                parse_read(&args(&values)).is_err_and(|message| message.contains(needle)),
                "{values:?}"
            );
        }
    }

    #[test]
    fn decide_parses_every_flag() {
        assert_eq!(
            decide(&["--json"]),
            Ok(DecideArgs {
                log_dir: PathBuf::from("d"),
                proposal: "p-1".to_owned(),
                outcome: Outcome::Accept,
                basis: "looks right".to_owned(),
                reviewer: "dave".to_owned(),
                json: true,
            })
        );
    }

    #[test]
    fn decide_requires_every_value_flag() {
        for name in ["log-dir", "proposal", "outcome", "basis", "reviewer"] {
            let flag = format!("--{name}");
            let mut values = args(&[
                "--log-dir",
                "d",
                "--proposal",
                "p-1",
                "--outcome",
                "reject",
                "--basis",
                "b",
                "--reviewer",
                "dave",
            ]);
            let at = values.iter().position(|value| *value == flag).unwrap_or(0);
            values.drain(at..at + 2);
            assert!(
                parse_decide(&values)
                    .is_err_and(|message| message.contains(&format!("{flag} is required"))),
                "{name}"
            );
        }
    }

    #[test]
    fn decide_refuses_a_bad_outcome_or_reviewer_at_parse_time() {
        let mut values = args(&[
            "--log-dir",
            "d",
            "--proposal",
            "p",
            "--outcome",
            "maybe",
            "--basis",
            "b",
            "--reviewer",
            "dave",
        ]);
        assert!(parse_decide(&values).is_err_and(|message| message.contains("'maybe'")));
        values[5] = "accept".to_owned();
        for reviewer in ["da ve", "da;ve", "da\u{7}ve"] {
            values[9] = reviewer.to_owned();
            assert!(
                parse_decide(&values).is_err_and(|message| message.contains("reviewer")),
                "{reviewer:?}"
            );
        }
    }

    #[test]
    fn grade_lines_name_every_tally() {
        let tally = |accepted, rejected| TallyDto {
            accepted,
            rejected,
            fraction: [accepted, accepted + rejected],
        };
        let line = grade_line(&GradeDto {
            class: "stream-mapping".to_owned(),
            actor: ActorDto::Agent {
                model: "m".to_owned(),
                version: "1".to_owned(),
            },
            proposed: 3,
            ungraded: 2,
            policy_accepted: 1,
            policy_rejected: 0,
            human: tally(0, 1),
            evidence: tally(0, 0),
            agent: tally(2, 0),
            policy_applied: tally(0, 0),
            policy_applied_ungraded: 1,
        });
        assert_eq!(
            line,
            "stream-mapping agent m@1: proposed 3, ungraded 2, human 0/1, evidence 0/0, agent \
             2/0, policy accepted 1 rejected 0, policy-applied 0/0 (ungraded 1)"
        );
    }
}
