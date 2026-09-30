# s2w-model

Shared vocabulary: timestamps, source ids, and later events, offsets, entity ids, proposals and issuances.

## Allowed dependencies

- `serde`, `serde_json`, `thiserror` (`serde_json` produces a mapping's canonical bytes for its
  identity, decision 0023)

The enforced list is `xtask/allowlist.toml`; `cargo xtask check` fails on anything else. Layer rules: `docs/decisions/0001-workspace-layers.md`.

## Invariants

- No I/O, no async, no clock, no randomness, no `HashMap`/`HashSet` (clippy enforces).
- A type arrives here only when a second crate needs it; `Cursor` and `RawEvent` live here so
  source and log adapters can exchange them without depending on one another.
- Persisted types get a version before any format is frozen (decision record, then migration).
- An entity id is assigned once and never reused. A merge aliases ids under the survivor;
  revoking a repair splits them back apart; neither operation changes an id. (`EntityId` lives
  in `s2w-core` until a second crate needs it; decision 0005.)
- `StreamMapping` (decision 0021) is persisted data with a `version` field. Versions 1
  (`MAPPING_VERSION`, no links) and 2 (`MAPPING_VERSION_LINKS`, decision 0027) are read; `links`
  is omitted from the JSON when empty, so a version-1 mapping's bytes never change. A format
  change bumps the version with a decision record. The mapping's paths and labels are data;
  this crate never names what they mean.
- The mapped natural-key text has one owner: `NaturalKey::from_parts` builds it and
  `NaturalKey::parts` reads it (decision 0021, amendment 2026-09-28). No other crate's non-test
  code joins or splits on `KEY_SEPARATOR`. String parts encode byte-identically to `serde_json::to_string`,
  pinned by a test. The format is versioned by `KEY_FORMAT` (decision 0023): change the bytes a
  key holds and you bump it, which a known-answer key test enforces.
- `StreamMapping::identity()` (decision 0023) names a mapping: `KEY_FORMAT`, the mapping's own
  `version` (decision 0027) and the canonical JSON, in that order. It names the engine that
  runs the mapping, so stored verdicts and snapshots are keyed by it. Never change what it
  hashes without a decision record; the fixture mapping's identity is pinned, and so are a
  version-1 and a version-2 identity.
- `DashboardManifest` format 1 (decision 0029) is a world's dashboard: domain, quintessential
  projection, roles, per-type and per-event presentation. `validate(&ManifestContext)` is the
  write path (accepted mappings plus the input profile's paths); `stale_entries` is the read
  path, against the current mappings only. `identity()` hashes the format and the manifest's
  canonical JSON (declaration order, `None` skipped) and is pinned for a full and a
  domain-level-only manifest. Every string is untrusted model output: capped, and free of `<`
  and `>`. The envelope around it lives in `s2w-app`.
- FNV-1a 64 (`Fnv64`, `fnv1a64`, `fnv1a64_hex`) lives here once. Its values are persisted
  (source ids, log content hashes, snapshot checksums, fixture hashes): never change it.
- No domain knowledge in this crate; see decision 0018.
