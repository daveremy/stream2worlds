- **P1 inner allow — Resolved:** `forbid` prevents inner allowances from overriding the protected lints.
- **P2 patch — Not resolved:** `overrides()` omits repository-local `.cargo/config`, which Cargo supports and prefers over `config.toml`; `[patch]` and `paths` there bypass the check. Check both filenames. [Cargo documentation](https://doc.rust-lang.org/cargo/reference/config.html)
- **P2 replay cursor — Resolved:** the cursor advances only after a complete frame is processed, including after successful output for retained events.

**Verdict: REQUEST CHANGES.**