# 0014: Serve topology — ingestion, bridge and HTTP in one process

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #10 (PR1)

## Decision

`s2w serve <source> [--log-dir <path>] [--port <port>]` ingests through the same source
registry and group-commit pump as `watch`, runs the System 1 bridge, and exposes the existing
query API. It creates a missing log directory, defaults to `./s2w-data`, and resumes stored
source cursors. There is no `--since` flag. MCP remains a separate, empty, read-only server.

The [review ruling](https://github.com/daveremy/stream2worlds/issues/10#issuecomment-5862060812)
rejects the proposed split `watch` + lockless-reader topology: one command must own the live
world. Exactly one `SqliteEventLog::open` holds the event writer lock, and one
`SqliteVerdictStore::open` holds the verdict writer lock for the same directory. No extra
sentinel lock is needed. Either lock conflict is a usage/state error (exit 2), naming the
store and directory, with advice to run only one watch/serve per directory. The lock API has
no PID; the message cannot identify one. Dropping the stores releases both locks.

0013 remains reserved for #64; the directory ended at 0012 when this record was added.

## Concurrency and startup

The current-thread runtime shares the event log through `Rc<RefCell<_>>`. Ingestion delegates
writes to it; the bridge's reader collects at most `BridgeConfig::batch` owned rows before
releasing its borrow. No borrow spans an await. Verdicts remain durable before claims become
visible, per [0012](0012-verdict-log.md).

The actual `Bridge::run` requires `Send` and uses `spawn_blocking`, so this command drives
`Bridge::poll_once` in a local future with the default batch and backoff policy (1,000 events,
250 ms to 2 s). Full batches explicitly yield so replay cannot monopolize the executor.
Neither ingestion nor the bridge is spawned. HTTP only holds the shared `QueryState`.
This adapts the proposed concurrency sketch without changing bridge internals.

Before accepting HTTP, wait for the first bridge poll, capped at 500 ms. That poll may find
no events; `/world` can initially be empty and grows as ingestion commits. This timer is a
cooperative bound: a synchronous SQLite operation or engine evaluation cannot be preempted.
Revisit moving I/O off-thread if measured latency requires it; do not open a second log.

## HTTP boundary and shutdown

Bind only `127.0.0.1`, default port **4310** (no conflicting configured port found).
`--port 0` asks the OS for a free port; stderr prints the actual address. No CORS layer and
no static assets: the evidence view and bundle pipeline belong to PR2.

Every route is behind a Host allowlist: exactly `localhost`, `127.0.0.1` or `[::1]`, optionally
followed by a numeric port in 0–65535. Missing, duplicate and all other Host headers return
400. Loopback binding alone does not prevent DNS rebinding: an attacker-controlled DNS name
can resolve to loopback while the request still carries that attacker's Host name.

The supervisor watches ingestion, bridge, HTTP and Ctrl-C together. Any early bridge return,
including success, is fatal: print the reason, signal HTTP shutdown and exit nonzero. A
panic during a bridge poll also becomes a fatal exit and follows the same drain path. Ingestion failure or an
unexpected end of a live source is fatal; finite stdin EOF stops the command successfully.
Ctrl-C stops ingestion; as with a crash, up to the pump's uncommitted batch can be lost.

HTTP drains gracefully for at most **five seconds**, including open `/events` SSE streams;
a timeout ends the runtime and closes remaining connections. Runtime shutdown does not wait
for Tokio stdin's uncancellable blocking read. A fatal producer error retains
its failure status even if draining succeeds. Poll-level retryable errors retain the existing
bridge policy; they are reported, not treated as a bridge exit.

## Consequences

A second watch/serve cannot run against the same directory until the owner releases it.
A lockless reader remains deferred until a demonstrated need for split processes. The query
API now serves live data without a frontend; the dashboard remains unfinished.

verify: `cargo test -p s2w-app serve && cargo test -p s2w` passes.
