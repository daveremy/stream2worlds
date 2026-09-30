//! The profiler's per-key report (`s2w_discover::key_report`) for a recorded stream, printed for a
//! person tuning the entity tests (decision 0022). s2w#291 and s2w#327 each rebuilt this by hand;
//! it asserts nothing.
//!
//! ```text
//! cargo test -p s2w-app --release --test discover_diag -- --ignored --nocapture
//! S2W_DIAG_SSE=<raw SSE file> S2W_DIAG_WINDOW=200000 cargo test ... (the same)
//! ```
//!
//! With no `S2W_DIAG_SSE` it reads the pinned recorded fixture. Either way each frame is wrapped
//! as the fixture loader wraps it, `{"data": <frame text>, "id": <frame id>}`, and the first
//! `S2W_DIAG_WINDOW` frames (default `DISCOVER_WINDOW`) are profiled under `Config::default()`.

#[path = "support/recorded.rs"]
#[expect(
    dead_code,
    reason = "this test reads the fixture's bytes, not its events or mapping"
)]
mod recorded;

// `allow-unwrap-in-tests` applies inside `#[cfg(test)]` items only.
#[cfg(test)]
mod discover_diag {
    use s2w_app::discover::DISCOVER_WINDOW;
    use s2w_discover::{Config, key_report};
    use s2w_sources::replay_frames;

    use super::recorded::{Fallible, bytes};

    #[test]
    #[ignore = "a hand-run diagnostic: prints the profiler's per-key report, asserts nothing"]
    fn print_the_key_report() -> Fallible<()> {
        let raw = match std::env::var_os("S2W_DIAG_SSE") {
            Some(path) => std::fs::read(path)?,
            None => bytes()?,
        };
        let window = match std::env::var("S2W_DIAG_WINDOW") {
            Ok(n) => n.parse()?,
            Err(_) => DISCOVER_WINDOW,
        };
        let payloads: Vec<Vec<u8>> = replay_frames(&raw)?
            .into_iter()
            .take(window)
            .map(|(id, data)| serde_json::to_vec(&serde_json::json!({ "data": data, "id": id })))
            .collect::<Result<_, _>>()?;
        let refs: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
        println!("{}", key_report(&refs, &Config::default()));
        Ok(())
    }
}
