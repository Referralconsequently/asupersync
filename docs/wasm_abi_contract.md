# WASM ABI Contract (WASM-ADR-007)

Contract ID: `asupersync-wasm-abi-v1`  
Status: active for browser profile lanes (`wasm-browser-dev`, `wasm-browser-prod`, `wasm-browser-deterministic`, `wasm-browser-minimal`)  
Primary owner: bindings/api

## Scope

This contract defines the stable JS/TS <-> WASM boundary schema for Asupersync:

1. ABI versioning and compatibility decisions.
2. Stable boundary symbol set and request/response payload shapes.
3. Outcome, error, and cancellation encoding rules.
4. Ownership/lifecycle boundary state transitions.
5. Deterministic drift-detection fingerprint for CI policy enforcement.

Canonical implementation: `src/types/wasm_abi.rs`.

## Versioning Rules

- Version type: `major.minor` (`WasmAbiVersion`).
- Compatibility function: `classify_wasm_abi_compatibility(producer, consumer)`.
- Rules:
  - major mismatch => incompatible
  - same major + consumer minor < producer minor => incompatible
  - same major + equal minor => exact
  - same major + consumer minor > producer minor => backward compatible

## Break Taxonomy

`WasmAbiChangeClass` and `required_wasm_abi_bump()` are normative:

- Minor bump:
  - additive fields
  - additive symbols
  - behavioral relaxations
- Major bump:
  - behavioral tightening
  - symbol removal/rename
  - wire-encoding changes
  - outcome semantic reinterpretation
  - cancellation semantic reinterpretation

## Boundary Symbols (v1)

`WASM_ABI_SIGNATURES_V1` defines the canonical symbol + payload-shape table:

- `runtime_create`
- `runtime_close`
- `scope_enter`
- `scope_close`
- `task_spawn`
- `task_join`
- `task_cancel`
- `fetch_request`

Each symbol is bound to request/response shape classes (`WasmAbiPayloadShape`).

## Outcome and Cancellation Encoding

- Outcome envelope: `WasmAbiOutcomeEnvelope`
  - `ok { value }`
  - `err { failure }`
  - `cancelled { cancellation }`
  - `panicked { message }`
- Error payload: `WasmAbiFailure` (`code`, `recoverability`, `message`)
- Cancellation payload: `WasmAbiCancellation` maps core `CancelReason` + `CancelPhase`
  with timestamp, origin, and truncation metadata for diagnostics.

## Ownership/Lifecycle State Machine

Boundary states: `WasmBoundaryState`

- `unbound -> bound -> active`
- `active -> cancelling -> draining -> closed`
- legal direct shutdown shortcuts:
  - `bound -> closed`
  - `active -> closed`
  - `cancelling -> closed`

Validation entrypoint: `validate_wasm_boundary_transition()`.

## Structured Observability Contract

`WasmAbiBoundaryEvent` must include:

- `abi_version`
- `symbol`
- `payload_shape`
- `state_from`
- `state_to`
- `compatibility` / `compatibility_decision`
- `compatibility_compatible`
- `compatibility_producer_major` / `compatibility_consumer_major`
- `compatibility_producer_minor` / `compatibility_consumer_minor` (when available)

`as_log_fields()` emits a deterministic key/value map for replay diagnostics.

## Drift Detection and CI Gate

- Deterministic signature fingerprint:
  - `wasm_abi_signature_fingerprint(WASM_ABI_SIGNATURES_V1)`
- Guard constant:
  - `WASM_ABI_SIGNATURE_FINGERPRINT_V1`
- Policy:
  - signature drift without version-policy update is a gate failure.
  - when fingerprint changes, update:
    1. version policy decision,
    2. migration notes,
    3. fingerprint constant.
- CI enforcement:
  - Policy file: `.github/wasm_abi_policy.json`
  - Gate script: `python3 scripts/check_wasm_abi_policy.py --policy .github/wasm_abi_policy.json`
  - Artifacts:
    - `artifacts/wasm_abi_contract_summary.json`
    - `artifacts/wasm_abi_contract_events.ndjson`

## Migration Notes Ledger

Current ABI entry: `v1.0 fingerprint=4558451663113424898`.

- `2026-02-28`: Initial `v1.0` contract baseline for WASM ABI v1 symbols,
  payload classes, and boundary state-machine semantics.

Update protocol for future ABI changes:

1. Update `WasmAbiVersion` and/or `WASM_ABI_SIGNATURE_FINGERPRINT_V1`.
2. Record a new migration ledger entry with version + fingerprint + rationale.
3. Update `.github/wasm_abi_policy.json` expected values.
4. Ensure `scripts/check_wasm_abi_policy.py` passes with updated artifacts.

## Test Evidence

See `src/types/wasm_abi.rs` test module:

- compatibility classification
- break taxonomy -> version bump mapping
- envelope serialization round-trips
- cancellation mapping
- lifecycle transition validation
- boundary event log-field contract
- signature fingerprint drift guard
