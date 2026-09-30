use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, Instant};

use super::{ExecLimits, ExecProvider, ExecSetupError};
use crate::provider::{Provider, ProviderError};

fn sh(script: &str) -> Vec<String> {
    vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()]
}

fn run(script: &str) -> Result<crate::Reply, ProviderError> {
    ExecProvider::new(sh(script), vec![])
        .unwrap()
        .complete("the prompt")
}

fn small(limits: impl FnOnce(&mut ExecLimits)) -> ExecLimits {
    let mut out = ExecLimits::default();
    limits(&mut out);
    out
}

#[test]
fn the_prompt_goes_on_stdin_and_the_reply_is_stdout() {
    let reply = run("cat").unwrap();
    assert_eq!(reply.text, "the prompt");
    assert!(reply.latency_ms.is_some());
    assert_eq!(reply.input_tokens, None);
}

#[test]
fn the_environment_is_cleared_except_the_passed_names() {
    let env = vec![("S2W_PASSED".to_owned(), OsString::from("yes"))];
    let reply = ExecProvider::new(vec!["/usr/bin/env".to_owned()], env)
        .unwrap()
        .complete("")
        .unwrap();
    assert_eq!(reply.text, "S2W_PASSED=yes\n");
}

#[test]
fn the_working_directory_is_fresh_empty_and_removed_after() {
    let reply = run("ls -A; pwd").unwrap();
    let mut lines = reply.text.lines();
    let cwd = lines.next().unwrap();
    assert_eq!(
        lines.next(),
        None,
        "the directory was not empty: {}",
        reply.text
    );
    assert!(cwd.contains("s2w-system2-"), "{cwd}");
    assert!(!Path::new(cwd).exists());
}

#[test]
fn a_command_past_its_time_limit_is_killed_with_its_partial_stdout() {
    let limits = small(|l| l.timeout = Duration::from_millis(300));
    let start = Instant::now();
    let error = ExecProvider::new(sh("printf partial; exec sleep 30"), vec![])
        .unwrap()
        .with_limits(limits)
        .complete("")
        .unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(10));
    let ProviderError::Timeout {
        stdout, latency_ms, ..
    } = &error
    else {
        panic!("{error:?}");
    };
    assert_eq!(stdout.as_deref(), Some("partial"));
    assert!(*latency_ms >= 300);
}

#[test]
fn a_child_of_the_command_does_not_hold_the_call_open() {
    let limits = small(|l| l.timeout = Duration::from_millis(300));
    let start = Instant::now();
    // The shell waits on a background sleep that inherits its stdout.
    let error = ExecProvider::new(sh("sleep 30 & wait"), vec![])
        .unwrap()
        .with_limits(limits)
        .complete("")
        .unwrap_err();
    assert!(matches!(error, ProviderError::Timeout { .. }), "{error:?}");
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn stdout_over_the_cap_is_refused_with_the_first_bytes_kept() {
    let limits = small(|l| l.stdout_bytes = 10);
    let error = ExecProvider::new(sh("printf 0123456789abcdef"), vec![])
        .unwrap()
        .with_limits(limits)
        .complete("")
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::StdoutTooLarge { limit: 10, .. }),
        "{error:?}"
    );
    assert_eq!(error.raw(), Some("0123456789"));
}

#[test]
fn a_failing_command_reports_its_status_and_capped_stderr() {
    let limits = small(|l| l.stderr_bytes = 4);
    let error = ExecProvider::new(sh("printf out; printf 'err-long' >&2; exit 3"), vec![])
        .unwrap()
        .with_limits(limits)
        .complete("")
        .unwrap_err();
    let ProviderError::Exit { stderr, status, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(stderr, "err-");
    assert!(status.contains('3'), "{status}");
    assert_eq!(error.raw(), Some("out"));
}

#[test]
fn a_missing_program_cannot_start() {
    let error = ExecProvider::new(vec!["/nonexistent/s2w-model-cli".to_owned()], vec![])
        .unwrap()
        .complete("")
        .unwrap_err();
    assert!(matches!(error, ProviderError::Spawn(_)), "{error:?}");
}

#[test]
fn non_utf8_stdout_is_refused() {
    let error = run(r"printf '\377'").unwrap_err();
    assert!(matches!(error, ProviderError::NotUtf8 { .. }), "{error:?}");
}

#[test]
fn a_large_prompt_to_a_command_that_ignores_stdin_does_not_block() {
    let prompt = "x".repeat(4 << 20);
    let reply = ExecProvider::new(sh("printf done"), vec![])
        .unwrap()
        .complete(&prompt)
        .unwrap();
    assert_eq!(reply.text, "done");
}

#[test]
fn setup_refuses_an_empty_command_a_bad_name_and_an_unset_variable() {
    assert_eq!(
        ExecProvider::new(vec![], vec![]).unwrap_err(),
        ExecSetupError::EmptyCommand
    );
    assert_eq!(
        ExecProvider::inherit(sh("true"), &["A=B".to_owned()]).unwrap_err(),
        ExecSetupError::BadName("A=B".to_owned())
    );
    let unset = "S2W_SYSTEM2_TEST_SURELY_UNSET".to_owned();
    assert_eq!(
        ExecProvider::inherit(sh("true"), std::slice::from_ref(&unset)).unwrap_err(),
        ExecSetupError::Unset(unset)
    );
}

#[test]
fn inherit_passes_a_set_variable_by_name() {
    // PATH is set in every test environment.
    let reply = ExecProvider::inherit(vec!["/usr/bin/env".to_owned()], &["PATH".to_owned()])
        .unwrap()
        .complete("")
        .unwrap();
    assert!(reply.text.starts_with("PATH="), "{}", reply.text);
    assert_eq!(reply.text.lines().count(), 1);
}
