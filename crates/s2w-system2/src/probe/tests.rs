use super::{PROBE_CALL, clean_session_probe};
use crate::mapping::{CallGate, NoGate};
use crate::record::CallRecord;
use crate::{ReplayProvider, Reply, prompt};

fn answering(text: &str) -> ReplayProvider {
    let mut provider = ReplayProvider::new();
    provider.insert(
        prompt::PROBE,
        Reply {
            text: text.to_owned(),
            input_tokens: Some(40),
            ..Reply::default()
        },
    );
    provider
}

#[test]
fn a_none_reply_proves_the_session_clean_and_is_recorded() {
    for text in ["none", " None\n", "NONE"] {
        let record = clean_session_probe(&answering(text), &mut NoGate).expect("clean");
        assert_eq!((record.attempt, record.call), PROBE_CALL);
        assert_eq!(record.reply.as_deref(), Some(text));
        assert_eq!(record.input_tokens, Some(40));
    }
}

#[test]
fn any_other_reply_or_a_provider_failure_refuses_with_the_record() {
    let (reason, record) =
        clean_session_probe(&answering("CLAUDE.md: be terse"), &mut NoGate).unwrap_err();
    assert!(reason.contains("CLAUDE.md: be terse"), "{reason}");
    assert!(record.is_some());
    let (reason, record) = clean_session_probe(&ReplayProvider::new(), &mut NoGate).unwrap_err();
    assert!(
        reason.starts_with("clean-session probe: provider:"),
        "{reason}"
    );
    assert!(record.expect("a failed call is recorded").error.is_some());
}

struct Refuse;

impl CallGate for Refuse {
    fn before_call(&mut self, prompt: &str, calls: &[CallRecord]) -> Result<(), String> {
        assert!(prompt.contains("none") && calls.is_empty());
        Err("budget: no".to_owned())
    }
}

#[test]
fn a_refusing_gate_stops_the_probe_before_any_call() {
    let (reason, record) = clean_session_probe(&answering("none"), &mut Refuse).unwrap_err();
    assert_eq!((reason.as_str(), record), ("budget: no", None));
}
