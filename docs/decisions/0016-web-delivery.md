# 0016: Local web delivery and the browser boundary

Date: 2026-09-27 · Status: accepted · Gate 2 · Issue #10 (PR2)

## Decision

Use `memory-serve` 2.4.0 (Apache-2.0 OR MIT) to deliver the committed `web/dist`
from the Rust binary. The integration is `memory_serve::load_directory_with_embed("web/dist", true)`
in `build.rs`, followed by `memory_serve::load!().index_file(Some("/index.html")).into_router()`
in `assets.rs`. Both runtime and build dependencies are required. The build script also emits
`cargo:rerun-if-changed=web/dist`, so a rebuilt frontend is never shadowed by a stale embed.
Node never runs inside a Rust build or the server. See the
[upstream API](https://docs.rs/memory-serve/2.4.0/memory_serve/).

This follows research 0003's preference over `rust-embed` and `include_dir`: automatic
compression and ETags behind one router. Embedding is forced in every profile: by default
memory-serve reads `web/dist` from disk in debug builds, which would let `cargo test` pass
without ever exercising the embed a release binary ships. Release builds also brotli-compress
the assets; debug builds embed them uncompressed. The release binary is byte-identical either
way (measured: the stripped binary below is the same file before and after the switch). This
gives up research 0003's "disk reads in debug" convenience; `rerun-if-changed` makes a frontend
rebuild recompile the crate instead. Missing asset paths return
404; they do not fall back to the application HTML. A mistyped API path (`/worlds/x/evnets`) also
falls through to the asset fallback and gets its plain 404, not the API's JSON error shape; the
fallback only sees unmatched paths, so it never masks a real route. This is accepted for now. Unhashed assets use `no-cache` so a new
binary's frontend is revalidated. The committed bundle lets independent Rust CI build without
Node; the bundle job rebuilds cleanly and detects both tracked changes and untracked output.

esbuild builds the TypeScript application. `force-graph` (MIT) renders its 2D graph behind the
`GraphRenderer` interface; replacing the renderer for #21 does not change the query contract.
TypeScript checks separately because esbuild does not check types. Node is pinned to 22.23.3
in `web/.nvmrc` (one exact version, not a range), which CI reads through `actions/setup-node`'s
`node-version-file`, so CI and local development resolve the same Node. `package-lock.json` is
committed and CI installs with `npm ci`. esbuild minifies the bundle and keeps licence comments
(`legalComments: 'eof'`).

## Browser boundary and event lifecycle

There is no authentication. Loopback binding plus the Host and Origin allowlists are the
entire current boundary. The shared `serve::app` assembles API routes, applies Origin checking
to those routes, attaches the asset fallback, and finally applies Host checking to everything.
Static files therefore cannot bypass the Host check.

An absent Origin passes: top-level navigation and some same-origin requests omit it. A
present Origin is compared as scheme, host and port together: it must be exactly
`http://localhost:<port>`, `http://127.0.0.1:<port>` or `http://[::1]:<port>`, where `<port>`
is the port in the request's (already validated) Host header, and both omit the port or both
carry the same one. Any of the three loopback names is accepted, because the page may be opened
under any of them. HTTPS, other hosts, other ports, a trailing path, `null` and a duplicated
header fail with 403 and a JSON `origin_rejected` error. Origin and Host share one loopback
authority parser, so the two allowlists cannot drift apart.

Each served timeline shares 32 concurrent event slots (one timeline/process in the supported
serve topology). `/events` acquires a permit before producing streaming headers and transfers
it into the stream object; dropping the body releases it, even if the body was never polled.
The 33rd request receives a `stream_limit` JSON error with HTTP 503 — a distinct code from
`unavailable` (the poisoned-lock case), so a client can tell a transient cap from a server
fault (round-1 review finding). A bounded
replay uses a slot too. The existing five-second shutdown drain remains in effect.

`from=` remains exclusive and Last-Event-ID retains precedence. The optional `at=` parameter
makes `/events` finite: replay through that offset and close. An invalid range is 400 and a
bound past the head is 404. This reuses `fold_with_delta` and the existing flattened SSE
message shape; it does not introduce another event representation.

Since decision 0023 (PR 2b-i) the viewer pins the `epoch` of its first snapshot on its stream,
evidence and probe URLs. A `stale_epoch` answer (the stream's final error frame, a 410 probe, or
a refreshed snapshot with another epoch) rebuilds the page from a fresh snapshot rather than
reconnecting at an offset of another history.

A pinned URL has `at=` and opens no EventSource. Both live and pinned views first fetch the
world, then seed the latest 500 deltas through that exact snapshot offset using finite replay.
The table uses offset keys and no invented timestamps. In live mode, deltas update only the
evidence rows and cursor. Every non-noop delta schedules a complete graph refetch, at most
once per second with trailing coalescing, including hub, merge and split transitions.

The client closes the old stream on every mode change and aborts stale fetches/timers.
It resumes from the last applied offset, ignores duplicates and owns exponential reconnect
backoff with a 30-second ceiling. EventSource hides HTTP status, so after an error the client
closes it and fetches a finite empty replay from the same route to distinguish a permanent
403 (persistent banner, stop retrying) from 503 (back off and retry). The probe has a ten-second
timeout. Evidence retention is 500 rows by count. Graph labels and evidence use text nodes
so content from the event stream is not interpreted as HTML.

## Licence and rebuild checks

Shipped libraries are production dependencies; build tools are devDependencies. The private
package root is excluded from `license-checker-rseidelsohn`'s production scan. The allowlist is
MIT, Apache-2.0, ISC, BSD-2-Clause, BSD-3-Clause and 0BSD. A committed installed fixture contains
GPL-3.0 and CC-BY-NC-4.0 packages; two checks independently require each rejection to name the
package and licence, so a missing executable or unrelated error cannot pass the fixture test.

`npm run build` removes dist, bundles the frontend, copies HTML/CSS, then runs `npm run notices`.
That step sorts production packages by name and writes their full licence text and repo-relative
source paths to `dist/THIRD-PARTY-LICENSES.txt`; it fails if licence text is missing. The clean
rebuild and `git status --porcelain -- dist` check cover this generated file as well.

## Measurements

Measured 2026-09-27 on Linux x86_64 with cargo 1.98.1. The baseline is `origin/main` at
`3d3eacc` (exported with `git archive`, no `memory-serve`, no bundle); the after build is this
change with the minified bundle embedded. Each binary is `cargo build --release -p s2w`, then
`strip`, then `ls -la`. The dependency count is `cargo tree -p s2w-app | wc -l`.

| Measurement | Before | After | Delta |
|---|---:|---:|---:|
| Stripped release `s2w` (bytes) | 48,152,560 | 49,866,416 | +1,713,856 (+3.6%) |
| `cargo tree -p s2w-app` (lines) | 633 | 670 | +37 |
| Distinct crates new to the tree | | | 14 |

The 14 new crates are `memory-serve` 2.4.0 and its dependencies: `brotli`,
`brotli-decompressor`, `alloc-no-stdlib`, `alloc-stdlib`, `async-trait`, `fs-err`, `hex`,
`mime_guess`, `same-file`, `sha256`, `unicase`, `urlencoding` and `walkdir`. All pass
`cargo deny check licenses advisories bans`. The committed bundle itself is 226,282 bytes before
compression (`main.js` 187,334; `THIRD-PARTY-LICENSES.txt` 36,028; `index.html` 1,553;
`style.css` 1,367) and 90,355 bytes as embedded in the release binary (brotli; the licence text
is stored uncompressed), so most of the binary delta is the added crates, not the assets.
research 0003's open question is answered: about 1.7 MB and 14 crates buys compressed, ETagged
assets with no Node in the Rust build.

verify: cargo test -p s2w-app serve && cd crates/s2w-app/web && npm ci && npm run build && npm run typecheck && npm test && npm run licenses && npm run test:licence-gate
