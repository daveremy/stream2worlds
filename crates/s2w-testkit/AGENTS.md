# s2w-testkit

Test support: fixtures, golden replays, stream builders.

## Allowed dependencies

- `s2w-model`, `s2w-log`

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- Used only as a dev-dependency.
- Golden files are human-owned; the testkit reads them, never rewrites them.
- `in_memory_log()` is the event-log fixture for tests in other crates.
