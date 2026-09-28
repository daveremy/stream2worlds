# 0018: No compiled domain code; domain knowledge is discovered data

Date: 2026-09-28 · Status: accepted (Dave-directed) · Issue #119 · Supersedes the Wikimedia defaults in [0011](0011-system1-bridge.md) · Builds on [0010](0010-gate3-h-arm.md)

## Decision

Dave, 2026-09-28: *"i don't think there should be compiled code for a domain at all. this needs
to all be discovered from the sources. it could be persisted but that is just data about the
stream."* (Earlier, from PredictStream: *"there should be nothing about wikipedia or air traffic
control in core modules."*)

1. **No crate contains domain code.** No engine, type, flag or branch names a domain (Wikipedia,
   ADS-B, retail, …) or keys on a domain's field names. Domain packs as crates are rejected: a
   pack is still compiled domain code.
2. **Domain knowledge is data about the stream.** Entity types, identifier fields, links, labels,
   free-text fields, filters and (per [0017](0017-view-and-agents-first-class.md)) the view spec
   are discovered by System 1 heuristics and System 2 proposals. They are persisted as events and
   a world profile in the log: versioned, revocable and replayable, like any repair.
3. **Allowed in code: technology, not domain.** Transports and formats (SSE, Kafka, HTTP, stdin
   NDJSON, JSON, Avro, Protobuf) are generic protocols. A named source such as `wikipedia` is an
   alias to a URL and stored settings: data, not code.
4. **Test fixtures may contain real domain data.** Recorded streams are data; the code under test
   may not know what they mean.

## Why this is already the design

The evaluation contract defines arm H as "heuristics alone: System 1 rules and local embeddings".
[Decision 0010](0010-gate3-h-arm.md) fixes H as research 0002 §6's seven domain-free stages
(flatten, profile, event-type field, role signatures, relate identifiers, assemble types, emit),
chosen to work on the obfuscated stream. `WikimediaPageChangeEngine` and the Wikimedia-bound
embeddings engine were bootstrapping scaffolding that let Gate 2 show a typed world before H
existed. This record retires them.

## Consequences

- **The demo gets less typed before it gets better.** Until H lands, a new stream shows raw
  identifiers and inferred shapes, not `user`/`page`. That is the honest state of the product;
  the view (#114) shows whatever the world profile knows.
- **Retire:** `WikimediaPageChangeEngine`; the Wikimedia schema binding in the embeddings engine
  (it classifies whichever free-text fields H selects by shape); the `wikipedia.*` →
  `wikimedia.page_change` bridge defaults in 0011; the `--wiki` flag (replaced by a generic
  field filter). The `wikipedia` preset becomes a URL alias plus stored settings.
- **Enforced by the build** (`cargo xtask check`, each check with a self-test proving it can fail):
  1. A domain-vocabulary scan over every crate's non-test source and the view source. The term
     list is committed (wiki, enwiki, page_id, revert, adsb, callsign, …) and grows with each
     stream we try.
  2. The obfuscation replay, which is the real guarantee because it catches terms nobody listed.
     The golden stream is replayed with every field renamed and every id hashed, and the world
     must come out structurally identical (the same graph up to labels). Any code that keys on a
     field name fails it.
  3. Each crate's `AGENTS.md` states the invariant; the architecture-lens review checks it.
