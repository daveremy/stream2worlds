# 0021: Stream mapping v0 — a data-driven System 1 executor and a raw-input obfuscation replay

Date: 2026-09-28 · Status: accepted · Gate 3 · Issue #163 (PR 1 of 5) · Amends [0011](0011-system1-bridge.md) (routing) · Builds on [0012](0012-verdict-log.md), [0018](0018-no-compiled-domain-code.md)

## Decision

Since [0018](0018-no-compiled-domain-code.md) no engine may know what a stream is about, so no
engine turns a raw stream into entities. This record adds the piece every producer of that
knowledge needs, whether a heuristic profiler, System 2 or an operator: a mapping that is
**data**, and one generic executor for it.

**`StreamMapping` v0 (`s2w-model`).** `{version, decode, entities, relationships}`.

- A `FieldPath` is a list of segments: a JSON string selects an object key, a JSON number an
  array index.
- `decode` lists paths whose string value holds JSON text. The executor parses each one in
  order and puts the parsed value in its place, so later paths reach into it. The SSE adapter
  stores every event as `{"data":"<JSON text>","id":…}`, so a mapping for an SSE stream starts
  with `decode: [["data"]]` and its other paths begin with `"data"`. This is a format step.
- An entity rule has an `id` (unique; relationship rules name it), a `type_label` (rules may
  share one), one or more `key` paths and optional `attrs` (`name` plus `path`).
- A relationship rule names two entity rule ids and a `kind`.
- `validate()` rejects: a version other than 1, duplicate rule ids, unknown relationship
  endpoints, an empty key, an empty path or key segment, an empty id, label, attribute name or
  kind, a repeated attribute name in one rule, and U+001F in a label or key segment.
- Aliases and merge rules are out of scope. Adding them makes the format version 2.

**`MappingEngine` (`s2w-system1`).** Built from one validated mapping. Per payload:

1. Payload is not JSON, or a decode path holds a non-string or invalid JSON → abstain
   `Unparseable`. A decode path that is absent is skipped.
2. An entity rule matches when every key path holds a scalar: a string, an integer that fits
   `i64`, or a bool. Floats, larger numbers, nulls, arrays and objects never match, and never
   panic. Attributes take the same scalars and are left out otherwise.
3. The natural key is the type label, then each key part JSON-encoded (a string quoted and
   escaped, an integer or bool bare), joined by U+001F. So `"123"` and `123` are different
   keys, one type's `7` never merges with another's, and a replay can split a key into parts.
4. A relationship is claimed when both endpoint rules matched in the same payload.
5. No entity rule matched → abstain `Insufficient("no entity rule matched")`. The payload is
   well formed, so this is missing data, not a foreign schema; `NotMine` stays a routing miss.
   Otherwise propose every claim as certain: entities in rule order, then relationships in rule
   order.

`name()` is `mapping`; `version()` is 1 and versions the executor's code only. The mapping is
named in `provenance()` as `{"mapping_hash":"<16 hex>"}`, FNV-1a/64 over the mapping's
serde_json bytes (field order is declaration order). The FNV helper is a local copy: this crate
may not depend on `s2w-sources`. *(Amended 2026-09-28, s2w#170: FNV-1a 64 now lives once, in
`s2w_model::fnv1a64`; see Amendments.)* **(engine name, version) does not identify a mapping.** The
verdict log keys verdicts on (position, engine, version) (0012), so two mappings run under the
same executor version would share keys. PR 2, which builds routes from stored mappings, must
fold a mapping digest into that identity or reset stored verdicts when the mapping changes.

**Routing (amends 0011).** 0011 makes routing app composition in code. This record keeps that
for now: `MappingEngine` is registered nowhere and `EngineRegistry::with_defaults()` is
unchanged. In #163 PR 2, routes move to data: `serve` builds a route per source from the
accepted stored mapping.

**Check 11, raw obfuscation replay (`xtask/src/obfuscation_raw.rs`).** Check 10 replays
claims; this engine reads raw payloads, so check 11 starts one layer earlier. It runs the engine
over `crates/s2w-system1/testdata/raw-sample.jsonl` with
`crates/s2w-system1/testdata/sample.mapping.json`, then again with every object key renamed
(`fN`) and every string leaf hashed (`h` + FNV hex) in both files, including inside each decoded
`data` string. The expected world is built from pass A's claims by typed role and must fold to
the same world as pass B. The check fails closed on hash collisions and on a mapping segment
that is not a key in the fixture. It also fails when the run is vacuous: pass A must yield two
entity types, a relationship, a multi-part key, an integer key part and a string attribute, the
maps must change the claims, and pass B may not carry any original string. Self-tests prove it
fails for an engine keyed on a raw field name, an obfuscator that skips the decoded string, a
dropped claim and a changed label.

**Fixtures.** `raw-sample.jsonl` is the first 20 events of
`crates/s2w-sources/testdata/wikipedia-page-change.raw.sse` (captured 2026-09-27), each wrapped
in the envelope bytes `s2w-sources/src/sse/envelope.rs` stores. A test checks the bytes match.
Reproduce it with:

```sh
python3 - <<'EOF'
import json
evs, cur = [], {}
for line in open('crates/s2w-sources/testdata/wikipedia-page-change.raw.sse', encoding='utf-8'):
    line = line.rstrip('\n')
    if line == '':
        if 'data' in cur and 'id' in cur: evs.append(cur)
        cur = {}; continue
    if line.startswith(':'): continue
    k, _, v = line.partition(':'); v = v[1:] if v.startswith(' ') else v
    if k == 'data': cur['data'] = cur['data'] + '\n' + v if 'data' in cur else v
    elif k == 'id': cur['id'] = v
with open('crates/s2w-system1/testdata/raw-sample.jsonl', 'w', encoding='utf-8') as f:
    for e in evs[:20]:
        f.write(json.dumps({'data': e['data'], 'id': e['id']}, ensure_ascii=False, separators=(',', ':')) + '\n')
EOF
```

`sample.mapping.json` is written by hand. It keys an editor on one path, an edited object on
two (a site id and an object id) and a site on one, with relationships editor → object and
object → site. It is recorded domain data under `testdata/` (0018 §4); no Rust source names it.

## Consequences

- No demo change: nothing routes to `MappingEngine` until PR 2.
- Missing surfaces ([0017](0017-view-and-agents-first-class.md)): the view and MCP show nothing
  about mappings yet. #163 PR 5 adds per-source mapping state to `/worlds/{w}/sources`, the MCP
  `sources` tool and the web sources line.
- No new dependencies; the allowlist is unchanged.

## Amendments

**2026-09-28 (s2w#170).** One owner for each shared piece, with no value changed:
- The natural-key text (rule 3) is built and read only by `s2w_model::NaturalKey::from_parts` and
  `NaturalKey::parts`, next to `KEY_SEPARATOR`. The string-part encoding is pinned byte for byte
  against `serde_json::to_string`; `parts` accepts only what `from_parts` writes. The format is
  not versioned yet: s2w#163 PR 2, which first persists key text, owns that decision.
- Decode and path lookup live in `s2w_system1::decode`; `MappingEngine` and `cargo xtask check`
  11 both call it, so the check cannot drift from the engine.
- FNV-1a 64 lives in `s2w_model` (`Fnv64`, `fnv1a64`, `fnv1a64_hex`). Every former copy calls it;
  source ids, log content hashes, snapshot checksums and `FOLD_FIXTURE_HASH` are unchanged.

verify: `cargo test -p s2w-system1 mapping && cargo xtask check` passes.
