//! `s2w proposals propose` (#309): parses the flags, reads the `--mapping` file and renders
//! what `s2w_app::proposals::record_mapping_proposal` stored.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use s2w_app::proposals::{Proposed, check_author, record_mapping_proposal};
use s2w_app::query::QueryError;

use super::{actor_text, failure, parse_flags};

/// Arguments accepted after `s2w proposals propose`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProposeArgs {
    log_dir: PathBuf,
    source: String,
    mapping: PathBuf,
    author: String,
    json: bool,
}

/// Parses `--log-dir <path> --source <id> --mapping <file> --author <id> [--json]`. Every value
/// flag is required, as for `decide`.
pub(super) fn parse(args: &[String]) -> Result<ProposeArgs, String> {
    let mut flags = parse_flags(args, &["log-dir", "source", "mapping", "author"])?;
    let mut required = |name: &str, shape: &str| {
        flags
            .take(name)
            .ok_or_else(|| format!("--{name} is required: --{name} {shape}"))
    };
    let log_dir = required("log-dir", "<path>")?;
    let source = required("source", "<id>")?;
    let mapping = required("mapping", "<file>")?;
    let author = required("author", "<id>")?;
    check_author(&author).map_err(|error| error.to_string())?;
    Ok(ProposeArgs {
        log_dir: PathBuf::from(log_dir),
        source,
        mapping: PathBuf::from(mapping),
        author,
        json: flags.json,
    })
}

pub(super) fn run(args: &ProposeArgs) -> ExitCode {
    let text = match std::fs::read(&args.mapping) {
        Ok(text) => text,
        Err(error) => {
            let error = QueryError::BadParameter {
                name: "mapping",
                reason: format!("cannot read {}: {error}", args.mapping.display()),
            };
            return failure(args.json, &args.log_dir, &error);
        }
    };
    let proposed = match record_mapping_proposal(&args.log_dir, &args.author, &args.source, &text) {
        Ok(proposed) => proposed,
        Err(error) => return failure(args.json, &args.log_dir, &error),
    };
    if args.json {
        let body = serde_json::json!({
            "proposal": proposed.proposal,
            "identity": proposed.identity,
        });
        println!("{body}");
        return ExitCode::SUCCESS;
    }
    for line in propose_lines(&proposed, &args.source, &args.log_dir) {
        println!("{line}");
    }
    ExitCode::SUCCESS
}

fn propose_lines(proposed: &Proposed, source: &str, log_dir: &Path) -> Vec<String> {
    vec![
        format!(
            "proposed {} for source '{source}': mapping {} by {}",
            proposed.proposal.id,
            proposed.identity,
            actor_text(&proposed.proposal.actor)
        ),
        format!(
            "nothing routes until a human accepts it: s2w proposals decide --log-dir {} \
             --proposal {} --outcome accept --basis <text> --reviewer <id>",
            log_dir.display(),
            proposed.proposal.id
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::args;

    #[test]
    fn propose_parses_every_flag_and_requires_each() {
        let full = [
            "--log-dir",
            "d",
            "--source",
            "s.x",
            "--mapping",
            "m.json",
            "--author",
            "dave",
        ];
        let mut values = full.to_vec();
        values.push("--json");
        assert_eq!(
            parse(&args(&values)),
            Ok(ProposeArgs {
                log_dir: PathBuf::from("d"),
                source: "s.x".to_owned(),
                mapping: PathBuf::from("m.json"),
                author: "dave".to_owned(),
                json: true,
            })
        );
        for name in ["log-dir", "source", "mapping", "author"] {
            let flag = format!("--{name}");
            let mut values = args(&full);
            let at = values.iter().position(|value| *value == flag).unwrap_or(0);
            values.drain(at..at + 2);
            assert!(
                parse(&values)
                    .is_err_and(|message| message.contains(&format!("{flag} is required"))),
                "{name}"
            );
        }
    }

    #[test]
    fn propose_refuses_a_bad_author_at_parse_time() {
        for author in ["", "da ve", "da;ve", "da\u{7}ve"] {
            let values = args(&[
                "--log-dir",
                "d",
                "--source",
                "s.x",
                "--mapping",
                "m.json",
                "--author",
                author,
            ]);
            assert!(
                parse(&values).is_err_and(|message| message.contains("author")),
                "{author:?}"
            );
        }
    }
}
