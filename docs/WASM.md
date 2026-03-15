# Asupersync Browser Edition (WASM)

This document describes the WASM/browser support in Asupersync: what works
today, what the architecture looks like, what the known limitations are, and
what is planned for future phases.

## What Works Today

### JS/TS consumers via wasm-bindgen (Phase 1 -- shipped)

Asupersync ships a Browser Edition that compiles the core runtime to
`wasm32-unknown-unknown` and exposes it to JavaScript and TypeScript through
`wasm-bindgen`. This is the primary supported path.

The npm package stack (sources in `packages/`; not yet published to the npm
registry -- use workspace-local references for now):

| Package | Role |
|---|---|
| `@asupersync/browser-core` | Low-level wasm-bindgen bindings, compiled `.wasm` artifact, ABI types |
| `@asupersync/browser` | High-level SDK: typed handles, outcome helpers, lifecycle management |
| `@asupersync/react` | React hooks and provider for structured concurrency in React apps |
| `@asupersync/next` | Next.js App Router bootstrap adapter with server/edge boundary handling |

From JavaScript, you get:

- **Structured concurrency scopes**: `runtimeCreate()`, `scopeEnter()`, `scopeClose()`
- **Task lifecycle**: `taskSpawn()`, `taskJoin()`, `taskCancel()`
- **Cancel-correct fetch**: `fetchRequest()` with automatic `AbortController` integration
- **WebSocket management**: `websocketOpen()`, `websocketSend()`, `websocketRecv()`, `websocketClose()`
- **Four-valued outcomes**: every operation returns `ok | err | cancelled | panicked`
- **ABI versioning**: `abiVersion()`, `abiFingerprint()` for compatibility checking

Quick example (vanilla JS):

```js
import init, { runtimeCreate, scopeEnter, taskSpawn, scopeClose, runtimeClose } from "@asupersync/browser";

await init();

const rt = runtimeCreate();
if (rt.outcome !== "ok") throw new Error(rt.failure.message);

const scope = scopeEnter({ parent: rt.value });
// ... spawn tasks, fetch, etc. ...
scopeClose(scope.value);
runtimeClose(rt.value);
```

### Core semantic guarantees preserved in browser

The browser runtime preserves all core Asupersync invariants:

1. **No orphan tasks**: structured ownership (task belongs to exactly one region)
2. **Cancel-correctness**: cancellation protocol is `request -> drain -> finalize`
3. **No obligation leaks**: two-phase commit-or-abort for all effects
4. **Region close implies quiescence**: all child tasks must complete before region closes
5. **Explicit capability boundaries**: no ambient authority to browser globals

### Build profiles

Four canonical browser profiles control the wasm compilation surface:

| Profile | Feature flag | Use case |
|---|---|---|
| Minimal | `wasm-browser-minimal` | ABI boundary checks, smallest artifact |
| Dev | `wasm-browser-dev` | Local development with browser I/O |
| Prod | `wasm-browser-prod` | Production builds with browser I/O |
| Deterministic | `wasm-browser-deterministic` | Replay-safe builds with browser trace |

Build command (example for dev profile):

```bash
rustup target add wasm32-unknown-unknown
cargo check --target wasm32-unknown-unknown --no-default-features --features wasm-browser-dev
```

Native-only features (`cli`, `io-uring`, `tls`, `sqlite`, `postgres`, `mysql`,
`kafka`) are compile-time rejected on `wasm32`.

## Authoritative Support Matrix (live tree)

This section is the canonical browser-feasibility classification for the
current tree. If `README.md`, package diagnostics, or older design notes lag,
this matrix wins and follow-on beads should align the other surfaces to it.

The shipped JS/TS diagnostics expose this matrix directly:

- `packages/browser/src/index.ts` reports
  `supportClass: "direct_runtime_supported" | "unsupported"` and
  `runtimeContext: "browser_main_thread" | "dedicated_worker" | "unknown"`.
- `packages/next/src/index.ts` preserves the browser diagnostics for client
  boundaries and adds `supportClass: "bridge_only"` plus explicit bridge-only
  reasons for Next `server` and `edge` targets.

### Runtime contexts

| Context | Classification | Live-tree evidence | Notes |
|---|---|---|---|
| Browser main thread (`window` + `document` + `WebAssembly`) | Direct-runtime supported | `packages/browser/src/index.ts`, `tests/wasm_js_exports_coverage_contract.rs` | Primary shipped JS/TS Browser Edition lane |
| Dedicated Web Worker (`DedicatedWorkerGlobalScope`) | Direct-runtime supported | `packages/browser/src/index.ts`, `asupersync-browser-core/src/lib.rs`, `tests/wasm_js_exports_coverage_contract.rs` | Shipped: SDK detects `DedicatedWorkerGlobalScope`, fetch routes through `WorkerGlobalScope.fetch()`; examples and QA are catching up |
| Service worker / shared worker direct runtime | Direct-runtime feasible but not yet shipped | `packages/browser/src/index.ts` currently accepts only main-thread DOM or dedicated worker globals | Deferred until lifecycle/host constraints are productized explicitly |
| Node / SSR / edge direct runtime via `@asupersync/browser` | Impossible for direct browser runtime; bridge-only or unsupported | `packages/browser/src/index.ts`, `packages/next/src/index.ts` | Browser package fails closed; Next diagnostics classify server/edge as bridge-only targets |
| Rust-authored `wasm32-unknown-unknown` consumer path | Direct-runtime feasible but not yet shipped | semantic core is target-agnostic, but there is no public Rust-callable browser runtime builder path yet | Planned lane, not current public support |
| Multi-worker / `SharedArrayBuffer` parallel execution | Guarded optional, not shipped | browser model is single-threaded today; true parallelism requires cross-origin isolation | Explicitly non-default even if pursued later |

### Capability families

| Surface | Classification | Live-tree evidence | Notes |
|---|---|---|---|
| Structured scopes, task lifecycle, four-valued outcomes | Direct-runtime supported | `packages/browser/src/index.ts`, `asupersync-browser-core` ABI exports | Core shipped Browser Edition surface |
| Browser `fetch` | Direct-runtime supported | `packages/browser/src/index.ts`, `asupersync-browser-core/src/lib.rs` | Main-thread and dedicated-worker hosts are both wired |
| Browser `WebSocket` | Direct-runtime supported | `asupersync-browser-core/src/lib.rs` | Shipped public JS/TS surface |
| Browser-safe persistence via public Browser Edition APIs | Direct-runtime feasible but not yet shipped as a documented public lane | `src/io/browser_storage.rs`, `src/io/cap.rs` | Substrate exists, but the public Browser Edition support story is not yet aligned/documented |
| `IndexedDB` durable storage | Direct-runtime feasible but not yet shipped | `src/io/cap.rs`, `src/io/browser_storage.rs` | Rust host backend is complete (`IndexedDbHostBackend` with get/set/clear/list_keys via `web_sys::IdbFactory`); gap is only in public JS/TS package surface |
| `localStorage` host-backed storage substrate | Guarded optional / substrate-only today | `src/io/browser_storage.rs` | Real host backend exists, but not yet elevated to canonical package-level support guarantees |
| Browser-native transport expansion (`WebTransport`, message-channel-style lanes) | Direct-runtime feasible but not yet shipped | `src/io/cap.rs` | Capability model exists ahead of public product surface |
| Raw TCP/UDP, Unix sockets, filesystem, process/signal | Impossible for direct browser runtime | `cfg`-gated native surfaces in core/runtime/docs | Must remain bridge-only or unsupported |

### Substrate-only capabilities (Rust layer complete, no public JS/TS API)

These items have real Rust implementations but are not yet exposed in the
`@asupersync/browser` or `@asupersync/browser-core` public packages.
Follow-on beads should decide whether to ship, defer, or remove each one.

| Surface | Rust evidence | Gap | Follow-on |
|---|---|---|---|
| `IndexedDB` durable storage | `src/io/browser_storage.rs` — complete `IndexedDbHostBackend` with `set`/`get`/`clear`/`list_keys` via `web_sys::IdbFactory`; worker-compatible | No JS/TS exports in `@asupersync/browser`; no public `BrowserStorage` handle type | `asupersync-3ak5y.1` |
| `localStorage` host backend | `src/io/browser_storage.rs` — `LocalStorageHostBackend` via `web_sys::Storage` | Same: substrate only, not elevated to package-level API | Part of `asupersync-3ak5y` |
| `MessagePort` reactor binding | `src/runtime/reactor/browser.rs` — `register_message_port()` with `onmessage`/`onmessageerror` handlers | No public API; reactor-internal only | `asupersync-1n453.1` |
| `BroadcastChannel` reactor binding | `src/runtime/reactor/browser.rs` — `register_broadcast_channel()` with `onmessage`/`onmessageerror` handlers | No public API; reactor-internal only | `asupersync-1n453.3` |
| `WebTransport` capability model | `src/io/cap.rs` — `BrowserTransportKind::WebTransport` defined | No host backend, no reactor integration, no JS/TS API | `asupersync-1n453.2` |
| `MessageChannel` capability model | `src/io/cap.rs` — modeled in config | No host backend, no JS/TS API | `asupersync-1n453.3` |
| WHATWG `ReadableStream`/`WritableStream` bridge | `src/io/browser_stream.rs` — maps WHATWG Streams to Asupersync `AsyncRead`/`AsyncWrite` with cancel semantics | No public JS/TS API; substrate-only | Future bead |
| Storage policy/capability layer | `src/io/cap.rs` — `StorageConsistencyPolicy`, `StorageIoCap`, `StorageBackend` enum, policy validation for namespace/size/consistency | Complete but only used internally by host backends | Part of `asupersync-3ak5y` |

### Live contradictions (2026-03-15, bead asupersync-1tte9)

These are concrete mismatches between what code, docs, and packages
currently claim. Each should be resolved by the referenced follow-on bead.

1. **IndexedDB: "not yet implemented" vs. real host backend.**
   `docs/WASM.md` and the support matrix above say "modeled in
   policy/storage layers, but not yet a completed public Browser Edition
   lane." In reality, `IndexedDbHostBackend` in `src/io/browser_storage.rs`
   is a complete async host backend with `set`/`get`/`clear`/`list_keys`,
   namespace isolation, and binary value encoding. The gap is only in the
   JS/TS package surface — no public `BrowserStorage` or `IndexedDbStore`
   type is exported. **Follow-on:** `asupersync-3ak5y.1`.

2. **Dedicated worker: shipped but under-documented.**
   The browser SDK (`packages/browser/src/index.ts`) correctly detects
   `DedicatedWorkerGlobalScope` and returns `direct_runtime_supported`.
   The browser-core fetch host routes through `WorkerGlobalScope.fetch()`.
   But `docs/WASM.md` says "QA/examples are still catching up" and the
   README browser section does not mention worker support. **Follow-on:**
   `asupersync-2w5tu`.

3. **MessagePort/BroadcastChannel: reactor wired, no public API.**
   `src/runtime/reactor/browser.rs` has real `register_message_port()`
   and `register_broadcast_channel()` implementations with `wasm_bindgen`
   closure attachment. The public package surface has no corresponding
   exports. **Follow-on:** `asupersync-1n453.1`, `asupersync-1n453.3`.

4. **Browser stream bridge: real implementation, no public surface.**
   `src/io/browser_stream.rs` bridges WHATWG `ReadableStream`/
   `WritableStream` to Asupersync `AsyncRead`/`AsyncWrite` with cancel
   semantics, byte accounting, and state-machine lifecycle. Not exported
   in any JS/TS package. **Follow-on:** future bead.

5. **Storage policy layer: mature but invisible.**
   `src/io/cap.rs` has a complete `StorageConsistencyPolicy` with
   `allowed_backends`, `max_key_len`, `max_value_len`, and
   `namespace_pattern` validation. This is used internally by the host
   backends but not documented or surfaced as a configurable option in
   the public API. **Follow-on:** part of `asupersync-3ak5y`.

### Contract test enforcement

The authoritative support matrix is encoded in executable contract tests:

```
tests/wasm_browser_feasibility_matrix.rs
```

These tests validate that the four-bucket classification matches the live
tree. If a contradiction is resolved (e.g. IndexedDB ships in the browser
package), the corresponding test assertion must be updated.

## Maintainer Admission Rule For New Browser Surfaces

Use this rule for every future Browser Edition feature request:

1. If the browser security model makes the surface impossible as a direct
   runtime capability, classify it as **impossible** and keep it
   bridge-only or unsupported. Do not add fake parity layers for raw
   sockets, ambient filesystem/process access, or native reactor semantics.
2. If the surface is browser-feasible but depends on explicit deployment or
   runtime prerequisites, classify it as **guarded optional** and name those
   prerequisites up front. `SharedArrayBuffer` worker pools, cross-origin
   isolation, and other special-host assumptions must never be treated as the
   default Browser Edition story.
3. If the surface is browser-feasible under ordinary browser constraints and
   preserves Asupersync's invariants, it should become real product work, not
   policy-only scaffolding. Classify it as **direct-runtime supported** if it
   is already shipped, or **direct-runtime feasible but not yet shipped** if
   code substrate exists ahead of public packaging, diagnostics, docs, or
   tests.

Invariant gate for steps 2 and 3:

- Preserve structured concurrency and explicit region ownership.
- Preserve cancellation as `request -> drain -> finalize`, including loser
  drain semantics.
- Preserve explicit capability boundaries; browser support must not smuggle
  in ambient authority.
- Preserve fail-closed diagnostics when a surface is outside the supported
  direct-runtime boundary.

## What Does Not Work Yet

### Rust-to-WASM compilation path (feasible, but not yet a public lane)

**Using Asupersync from async Rust code that itself compiles to WASM is not
documented or tested.** This is the scenario where you write Rust code using
Asupersync's `Cx`, scopes, and combinators, then compile that Rust code to
`wasm32-unknown-unknown` for execution in the browser.

The core semantic layer (structured scopes, cancellation state machine,
obligation accounting, combinators) is architecturally target-agnostic and
should be portable. However:

- The runtime scheduler and I/O reactor have native-specific code paths
  (`epoll`, `io_uring`, `polling`, `socket2`, `signal-hook`) that are
  `cfg`-gated for `not(target_arch = "wasm32")`.
- A browser-specific scheduler pump (driven by `queueMicrotask` /
  `MessageChannel` / `setTimeout`) exists in the design but is not yet
  exposed as a Rust-callable API.
- There is no public `RuntimeBuilder` path that produces a wasm32-compatible
  runtime from Rust consumer code.

This path is on the roadmap but not prioritized. If you need it, please
comment on [issue #11](https://github.com/Dicklesworthstone/asupersync/issues/11).

## Architectural Boundary

The cleanest way to think about the WASM story:

```
+-----------------------------------------------+
|          Shared Semantic Core                  |
|  (scopes, cancellation, combinators,           |
|   obligation accounting, trace, types)         |
+-----------------------------------------------+
         |                          |
         v                          v
+------------------+    +--------------------+
| Native Executor  |    | Browser Executor   |
| (epoll/io_uring, |    | (event-loop pump,  |
|  threads, OS I/O)|    |  Web APIs, fetch,  |
|                  |    |  WebSocket)        |
+------------------+    +--------------------+
```

The semantic core is the same code compiled to both targets. The executor
layer is environment-specific:

- **Native**: multi-threaded work-stealing scheduler, OS-level I/O reactor,
  real TCP/UDP sockets, filesystem, process/signal handling.
- **Browser**: single-threaded cooperative scheduler driven by the JS event
  loop, browser `fetch()` and `WebSocket` APIs, and browser-safe host
  integration points for storage and transport expansion.

The `asupersync-browser-core` crate is the concrete bridge: it instantiates
`WasmExportDispatcher` (the core ABI surface) and wires it to browser APIs
via `web-sys` and `wasm-bindgen-futures`.

## Browser Runtime Model

The current browser runtime model (Phase 1) is:

- **Single-threaded**: all Asupersync tasks run on the browser main thread
  or inside a single dedicated Web Worker.
- **Cooperative**: the scheduler yields back to the JS event loop between
  scheduling steps to avoid blocking the UI thread.
- **Event-loop driven**: browser timer APIs, `fetch` completions, and
  WebSocket events feed into the runtime's wakeup machinery.

### What this means for guarantees

| Guarantee | Native | Browser | Notes |
|---|---|---|---|
| No orphan tasks | Full | Full | Structured scopes enforce ownership |
| Cancel-correctness | Full | Full | Three-phase protocol is target-agnostic |
| Bounded cleanup | Full | Cooperative | Depends on cooperative yielding; no preemption |
| Deterministic scheduling | Full (lab mode) | Partial | Browser event loop introduces nondeterminism unless strictly serialized |
| CPU parallelism | Full (work-stealing) | None (single-threaded) | See "Future: threaded WASM" below |

## Known Limitations and Constraints

### Browser environment constraints

- **No raw TCP/UDP**: networking is limited to browser APIs (`fetch`,
  `WebSocket`). Native TCP/UDP, Unix sockets, and raw I/O are
  unavailable.
- **No filesystem access**: `fs` module surfaces are `cfg`-gated out on
  wasm32. Browser-safe storage capability substrate exists, including a
  wasm `localStorage` host backend in `src/io/browser_storage.rs`, but the
  public Browser Edition persistence story is not yet fully aligned and
  `IndexedDB` has a complete Rust host backend (`IndexedDbHostBackend`) but
  is not yet shipped as a public JS/TS API.
- **No process/signal handling**: the `process` and `signal` modules are
  native-only.
- **No multi-threading by default**: the Phase 1 browser runtime is
  single-threaded. Supported direct-runtime lanes are the browser main thread
  and a single dedicated Web Worker; service-worker/shared-worker lanes remain
  deferred. True parallelism requires additional workers plus the Phase 2
  model below.

### Cross-origin isolation for SharedArrayBuffer

Multi-threaded WASM (using `SharedArrayBuffer` + Atomics) requires
cross-origin isolation headers:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

This is a significant deployment constraint: many web applications cannot
enable these headers due to third-party embed requirements. Phase 1
intentionally avoids this dependency.

### Artifact size budgets

Browser Edition artifacts are size-budgeted:

| Profile | Raw `.wasm` budget | Gzip budget |
|---|---|---|
| `core-min` | 650 KiB | 220 KiB |
| `core-trace` | 900 KiB | 320 KiB |
| `full-dev` | 1300 KiB | 480 KiB |

## Future: Threaded WASM Executor (Phase 2)

A future phase may add a multi-threaded WASM executor using:

- `SharedArrayBuffer` + Atomics for shared memory between workers
- A native-style scheduler inside WASM (potentially in a `SharedWorker`)
- Work-stealing across Web Worker threads

This would enable closer parity with native scheduling semantics but requires:

1. Cross-origin isolation (see above)
2. Careful message-passing design (Workers don't share JS state)
3. A different cancellation propagation model across worker boundaries

This is explicitly Phase 2 and will only be pursued if demand materializes.
The single-threaded, event-loop-driven model provides the core structured
concurrency guarantees that matter most.

## Crate Map

| Crate | Purpose | Browser role |
|---|---|---|
| `asupersync` | Core runtime library | Compiles to wasm32 with browser feature profiles |
| `asupersync-browser-core` | wasm-bindgen export boundary | Bridges core runtime to JS via ABI symbol table |
| `asupersync-wasm` | Alternative WASM binding surface (scaffold) | Placeholder for future binding strategies |
| `asupersync-tokio-compat` | Tokio bridge adapters | Native-only; not applicable to browser |

## Further Reading

- [`PLAN_TO_BUILD_ASUPERSYNC_IN_WASM_FOR_USE_IN_BROWSERS.md`](../PLAN_TO_BUILD_ASUPERSYNC_IN_WASM_FOR_USE_IN_BROWSERS.md) -- full execution blueprint
- [`docs/wasm_quickstart_migration.md`](./wasm_quickstart_migration.md) -- onboarding commands and profile selection
- [`docs/wasm_canonical_examples.md`](./wasm_canonical_examples.md) -- vanilla/React/Next.js example catalog
- [`docs/wasm_browser_scheduler_semantics.md`](./wasm_browser_scheduler_semantics.md) -- scheduler/event-loop contract
- [`docs/wasm_platform_trait_seams.md`](./wasm_platform_trait_seams.md) -- seam contracts between semantic core and backends
- [`docs/wasm_troubleshooting_compendium.md`](./wasm_troubleshooting_compendium.md) -- failure recipes and diagnostics
- [Issue #11](https://github.com/Dicklesworthstone/asupersync/issues/11) -- WASM support discussion and architectural questions
