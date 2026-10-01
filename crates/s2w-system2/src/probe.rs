//! The clean-session probe (decision 0032): the first call of a gate-3 run asks the model what
//! it can see besides its built-in system prompt and the probe itself, and the run goes on only
//! when the reply is `none`. The probe is gated and recorded like every other call.

use crate::mapping::{CallGate, system_clock};
use crate::provider::Provider;
use crate::record::CallRecord;
use crate::{prompt, replay};

/// The probe's `(attempt, call)` in its record: before any proposal attempt.
pub const PROBE_CALL: (u32, u32) = (0, 0);

/// Asks `provider` the probe, through `gate`. `Ok` holds the record of a call whose reply was
/// `none` (trimmed, any case). `Err` holds why the session is not proven clean, and the call's
/// record when one was made (a refused gate makes none).
///
/// # Errors
///
/// The gate refused the call, the provider failed, or the reply was not `none`.
pub fn clean_session_probe<P: Provider>(
    provider: &P,
    gate: &mut dyn CallGate,
) -> Result<CallRecord, (String, Option<Box<CallRecord>>)> {
    let text = prompt::PROBE;
    gate.before_call(text, &[])
        .map_err(|reason| (reason, None))?;
    let started_at_ms = system_clock();
    let result = provider.complete(text);
    let record = CallRecord::new(PROBE_CALL, replay::hash(text), &result, started_at_ms);
    match result {
        Ok(reply) if reply.text.trim().eq_ignore_ascii_case("none") => Ok(record),
        Ok(reply) => Err((
            format!(
                "clean-session probe: the model reported context besides the probe: {:?}",
                reply.text
            ),
            Some(Box::new(record)),
        )),
        Err(e) => Err((
            format!("clean-session probe: provider: {e}"),
            Some(Box::new(record)),
        )),
    }
}

#[cfg(test)]
mod tests;
