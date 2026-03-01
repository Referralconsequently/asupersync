#![allow(missing_docs)]

use asupersync::{
    WASM_ABI_MAJOR_VERSION, WASM_ABI_MINOR_VERSION, WASM_ABI_SIGNATURE_FINGERPRINT_V1,
    WASM_ABI_SIGNATURES_V1, WasmAbiBoundaryEvent, WasmAbiCompatibilityDecision,
    WasmAbiPayloadShape, WasmAbiSymbol, WasmAbiVersion, WasmBoundaryState,
    classify_wasm_abi_compatibility, wasm_abi_signature_fingerprint,
};
use std::collections::BTreeSet;

#[test]
fn wasm_abi_signature_matrix_matches_v1_contract() {
    let signatures: Vec<(&str, WasmAbiPayloadShape, WasmAbiPayloadShape)> = WASM_ABI_SIGNATURES_V1
        .iter()
        .map(|signature| {
            (
                signature.symbol.as_str(),
                signature.request,
                signature.response,
            )
        })
        .collect();

    assert_eq!(
        signatures,
        vec![
            (
                "runtime_create",
                WasmAbiPayloadShape::Empty,
                WasmAbiPayloadShape::HandleRefV1,
            ),
            (
                "runtime_close",
                WasmAbiPayloadShape::HandleRefV1,
                WasmAbiPayloadShape::OutcomeEnvelopeV1,
            ),
            (
                "scope_enter",
                WasmAbiPayloadShape::ScopeEnterRequestV1,
                WasmAbiPayloadShape::HandleRefV1,
            ),
            (
                "scope_close",
                WasmAbiPayloadShape::HandleRefV1,
                WasmAbiPayloadShape::OutcomeEnvelopeV1,
            ),
            (
                "task_spawn",
                WasmAbiPayloadShape::SpawnRequestV1,
                WasmAbiPayloadShape::HandleRefV1,
            ),
            (
                "task_join",
                WasmAbiPayloadShape::HandleRefV1,
                WasmAbiPayloadShape::OutcomeEnvelopeV1,
            ),
            (
                "task_cancel",
                WasmAbiPayloadShape::CancelRequestV1,
                WasmAbiPayloadShape::OutcomeEnvelopeV1,
            ),
            (
                "fetch_request",
                WasmAbiPayloadShape::FetchRequestV1,
                WasmAbiPayloadShape::OutcomeEnvelopeV1,
            ),
        ],
        "ABI signature matrix drifted from v1 contract"
    );

    let unique_symbols: BTreeSet<&str> = signatures.iter().map(|(symbol, _, _)| *symbol).collect();
    assert_eq!(
        unique_symbols.len(),
        signatures.len(),
        "ABI signature matrix must not contain duplicate symbols"
    );
}

#[test]
fn wasm_abi_version_and_fingerprint_constants_match_signature_table() {
    assert_eq!(WASM_ABI_MAJOR_VERSION, 1);
    assert_eq!(WASM_ABI_MINOR_VERSION, 0);
    assert_eq!(
        wasm_abi_signature_fingerprint(&WASM_ABI_SIGNATURES_V1),
        WASM_ABI_SIGNATURE_FINGERPRINT_V1
    );
}

#[test]
fn wasm_abi_compatibility_rules_cover_exact_backward_and_rejecting_paths() {
    let exact = classify_wasm_abi_compatibility(
        WasmAbiVersion { major: 1, minor: 0 },
        WasmAbiVersion { major: 1, minor: 0 },
    );
    assert_eq!(exact, WasmAbiCompatibilityDecision::Exact);

    let backward = classify_wasm_abi_compatibility(
        WasmAbiVersion { major: 1, minor: 0 },
        WasmAbiVersion { major: 1, minor: 3 },
    );
    assert!(matches!(
        backward,
        WasmAbiCompatibilityDecision::BackwardCompatible {
            producer_minor: 0,
            consumer_minor: 3
        }
    ));

    let consumer_too_old = classify_wasm_abi_compatibility(
        WasmAbiVersion { major: 1, minor: 3 },
        WasmAbiVersion { major: 1, minor: 2 },
    );
    assert!(matches!(
        consumer_too_old,
        WasmAbiCompatibilityDecision::ConsumerTooOld {
            producer_minor: 3,
            consumer_minor: 2
        }
    ));

    let major_mismatch = classify_wasm_abi_compatibility(
        WasmAbiVersion { major: 1, minor: 3 },
        WasmAbiVersion { major: 2, minor: 0 },
    );
    assert!(matches!(
        major_mismatch,
        WasmAbiCompatibilityDecision::MajorMismatch {
            producer_major: 1,
            consumer_major: 2
        }
    ));
}

#[test]
fn wasm_boundary_event_log_fields_are_deterministic() {
    let event = WasmAbiBoundaryEvent {
        abi_version: WasmAbiVersion::CURRENT,
        symbol: WasmAbiSymbol::TaskCancel,
        payload_shape: WasmAbiPayloadShape::CancelRequestV1,
        state_from: WasmBoundaryState::Active,
        state_to: WasmBoundaryState::Cancelling,
        compatibility: WasmAbiCompatibilityDecision::Exact,
    };

    let fields = event.as_log_fields();
    let key_order: Vec<&str> = fields.keys().copied().collect();
    assert_eq!(
        key_order,
        vec![
            "abi_version",
            "compatibility",
            "payload_shape",
            "state_from",
            "state_to",
            "symbol"
        ]
    );
    assert_eq!(fields["abi_version"], "1.0");
    assert_eq!(fields["symbol"], "task_cancel");
}
