//! Contract tests for Tokio adapter boundary architecture (2oh2u.7.2).
//!
//! Validates enforceable adapter invariants, outcome contracts, structured
//! replay evidence requirements, and rch-offloaded validation commands.

#![allow(missing_docs)]

use std::path::Path;

fn load_doc() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/tokio_adapter_boundary_architecture.md");
    std::fs::read_to_string(path).expect("adapter boundary architecture document must exist")
}

#[test]
fn architecture_doc_exists_and_is_substantial() {
    let doc = load_doc();
    assert!(
        doc.len() > 9_000,
        "adapter boundary architecture doc should be substantial, got {} bytes",
        doc.len()
    );
}

#[test]
fn architecture_doc_references_correct_bead_and_metadata() {
    let doc = load_doc();
    for token in [
        "asupersync-2oh2u.7.2",
        "[T7.2]",
        "Maintained by",
        "WhiteDesert",
        "Version",
        "1.2.0",
    ] {
        assert!(doc.contains(token), "missing metadata token: {token}");
    }
}

#[test]
fn architecture_doc_declares_non_negotiable_runtime_invariants() {
    let doc = load_doc();
    for token in [
        "No ambient authority",
        "Structured concurrency",
        "Cancellation is a protocol",
        "No obligation leaks",
        "Outcome severity lattice",
    ] {
        assert!(doc.contains(token), "missing invariant token: {token}");
    }
}

#[test]
fn architecture_doc_enforces_hard_tokio_boundary_rules() {
    let doc = load_doc();
    for token in [
        "RULE 1: No Tokio in core runtime paths.",
        "RULE 2: Adapters are in a separate crate.",
        "RULE 3: Cx must cross the boundary.",
        "RULE 4: Region ownership is non-negotiable.",
        "asupersync-tokio-compat",
    ] {
        assert!(doc.contains(token), "missing boundary-rule token: {token}");
    }
}

#[test]
fn architecture_doc_has_success_failure_cancellation_outcome_matrix() {
    let doc = load_doc();
    assert!(
        doc.contains("Boundary Outcome Contract (Success/Failure/Cancellation)"),
        "must include boundary outcome contract section"
    );
    for token in [
        "Success Contract",
        "Failure Contract",
        "Cancellation Contract",
        "Deterministic Assertion",
        "Runtime bridge (`with_tokio_context`)",
        "Hyper bridge (`hyper_bridge`)",
        "SQLx runtime adapter (`sqlx_runtime`)",
        "Tonic transport bridge (`tonic_transport`)",
        "Outcome::Cancelled",
    ] {
        assert!(
            doc.contains(token),
            "missing outcome-contract token: {token}"
        );
    }
}

#[test]
fn architecture_doc_declares_forbidden_patterns_explicitly() {
    let doc = load_doc();
    for token in [
        "NEVER: Embed a Hidden Tokio Runtime",
        "NEVER: Bypass Cx for Convenience",
        "NEVER: Spawn Untracked Background Tasks",
        "NEVER: Swallow Cancellation",
    ] {
        assert!(
            doc.contains(token),
            "missing forbidden-pattern token: {token}"
        );
    }
}

#[test]
fn architecture_doc_requires_structured_logs_and_replay_artifacts() {
    let doc = load_doc();
    assert!(
        doc.contains("Structured Logs and Replay Artifacts"),
        "must include structured logs and replay artifacts section"
    );
    for token in [
        "`correlation_id`",
        "`adapter_path`",
        "`trace_id`",
        "`decision_id`",
        "`replay_seed`",
        "`artifact_uri`",
        "artifacts/tokio_adapter_boundary/<run-id>/adapter_events.jsonl",
        "artifacts/tokio_adapter_boundary/<run-id>/replay_summary.json",
        "artifacts/tokio_adapter_boundary/<run-id>/failure_triage.md",
        "Hard-fail quality gate policy",
    ] {
        assert!(
            doc.contains(token),
            "missing replay-evidence token: {token}"
        );
    }
}

#[test]
fn architecture_doc_includes_rch_validation_bundle() {
    let doc = load_doc();
    for token in [
        "rch exec -- cargo test --test tokio_adapter_boundary_architecture -- --nocapture",
        "rch exec -- cargo check --all-targets -q",
        "rch exec -- cargo fmt --check",
        "rch exec -- cargo clippy --all-targets -- -D warnings",
    ] {
        assert!(
            doc.contains(token),
            "missing validation command token: {token}"
        );
    }
}

#[test]
fn architecture_doc_links_contract_and_source_evidence() {
    let doc = load_doc();
    for token in [
        "docs/tokio_interop_target_ranking.md",
        "docs/tokio_functional_parity_contract.md",
        "docs/tokio_nonfunctional_closure_criteria.md",
        "docs/tokio_evidence_checklist.md",
        "asupersync-tokio-compat/src/runtime.rs",
        "asupersync-tokio-compat/src/executor.rs",
        "asupersync-tokio-compat/src/timer.rs",
        "asupersync-tokio-compat/src/io.rs",
        "asupersync-tokio-compat/src/cancel.rs",
        "tests/tokio_adapter_boundary_architecture.rs",
    ] {
        assert!(doc.contains(token), "missing evidence-link token: {token}");
    }
}

#[test]
fn architecture_doc_revision_history_tracks_latest_update() {
    let doc = load_doc();
    assert!(
        doc.contains("| 2026-03-03 | WhiteDesert |"),
        "revision history should include WhiteDesert row"
    );
    assert!(
        doc.contains("| 2026-03-03 | SapphireHill | Initial architecture (v1.0) |"),
        "revision history should retain initial baseline row"
    );
}

// ---------------------------------------------------------------------------
// T7.4 Contract Tests — Adapter Primitive Implementation Evidence
// ---------------------------------------------------------------------------

#[test]
fn t74_architecture_doc_records_implementation_evidence() {
    let doc = load_doc();
    assert!(
        doc.contains("T7.4"),
        "architecture doc must reference T7.4 implementation"
    );
    for token in [
        "I/O trait bridging",
        "TokioIo",
        "AsupersyncIo",
        "executor",
        "spawn_fn",
        "timer",
        "waker",
    ] {
        assert!(
            doc.to_ascii_lowercase()
                .contains(&token.to_ascii_lowercase()),
            "T7.4 evidence missing implementation token: {token}"
        );
    }
}

#[test]
fn t74_compat_crate_cargo_toml_has_required_deps() {
    let toml_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("asupersync-tokio-compat/Cargo.toml");
    let toml = std::fs::read_to_string(toml_path).expect("compat crate Cargo.toml must exist");

    for dep in ["asupersync", "tokio", "pin-project-lite"] {
        assert!(toml.contains(dep), "must depend on {dep}");
    }
    for feature in ["hyper-bridge", "tokio-io"] {
        assert!(toml.contains(feature), "must define {feature} feature");
    }
    assert!(
        toml.contains("default-features = false"),
        "tokio dependency must disable default features (no runtime)"
    );
}

#[test]
fn t74_compat_crate_lib_rs_exports_expected_modules() {
    let lib_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("asupersync-tokio-compat/src/lib.rs");
    let lib = std::fs::read_to_string(lib_path).expect("compat crate lib.rs must exist");

    for module in ["pub mod cancel", "pub mod io"] {
        assert!(lib.contains(module), "lib.rs must export module: {module}");
    }
    assert!(
        lib.contains("pub mod hyper_bridge"),
        "lib.rs must export hyper_bridge (gated on hyper-bridge feature)"
    );
    assert!(
        lib.contains("#![deny(unsafe_code)]"),
        "compat crate must deny unsafe code by default"
    );
}

#[test]
fn t74_io_module_has_bidirectional_trait_impls() {
    let io_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("asupersync-tokio-compat/src/io.rs");
    let io = std::fs::read_to_string(io_path).expect("compat crate io.rs must exist");

    // Direction 1: Asupersync → Tokio
    assert!(
        io.contains("tokio::io::AsyncRead for TokioIo"),
        "must impl tokio::io::AsyncRead for TokioIo"
    );
    assert!(
        io.contains("tokio::io::AsyncWrite for TokioIo"),
        "must impl tokio::io::AsyncWrite for TokioIo"
    );

    // Direction 2: Tokio → Asupersync
    assert!(
        io.contains("asupersync::io::AsyncRead for AsupersyncIo"),
        "must impl asupersync::io::AsyncRead for AsupersyncIo"
    );
    assert!(
        io.contains("asupersync::io::AsyncWrite for AsupersyncIo"),
        "must impl asupersync::io::AsyncWrite for AsupersyncIo"
    );

    // Direction 3: Asupersync → hyper v1
    assert!(
        io.contains("hyper::rt::Read for TokioIo"),
        "must impl hyper::rt::Read for TokioIo"
    );
    assert!(
        io.contains("hyper::rt::Write for TokioIo"),
        "must impl hyper::rt::Write for TokioIo"
    );

    // ReadBuf bridging evidence
    assert!(
        io.contains("ReadBuf::new"),
        "must bridge ReadBuf types between Asupersync and Tokio"
    );
}

#[test]
fn t74_executor_uses_callback_not_ambient_authority() {
    let bridge_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("asupersync-tokio-compat/src/hyper_bridge.rs");
    let bridge =
        std::fs::read_to_string(bridge_path).expect("compat crate hyper_bridge.rs must exist");

    // INV-1: No ambient authority — executor uses explicit spawn callback.
    assert!(
        bridge.contains("spawn_fn"),
        "executor must use explicit spawn_fn (no ambient authority)"
    );
    assert!(
        bridge.contains("with_spawn_fn"),
        "executor must expose with_spawn_fn constructor"
    );

    // INV-2: Structured concurrency — documents region ownership.
    assert!(
        bridge.contains("region-owned"),
        "executor must document region ownership of spawned tasks"
    );

    // No unimplemented! in production code paths.
    assert!(
        !bridge.contains("unimplemented!"),
        "executor must not contain unimplemented!() stubs"
    );
}

#[test]
fn t74_timer_uses_waker_based_notification() {
    let bridge_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("asupersync-tokio-compat/src/hyper_bridge.rs");
    let bridge =
        std::fs::read_to_string(bridge_path).expect("compat crate hyper_bridge.rs must exist");

    assert!(
        bridge.contains("waker"),
        "timer must use waker-based notification"
    );
    assert!(
        bridge.contains("hyper::rt::Timer for AsupersyncTimer"),
        "must impl hyper::rt::Timer"
    );
    assert!(
        bridge.contains("hyper::rt::Sleep for AsupersyncSleep"),
        "must impl hyper::rt::Sleep"
    );
}

#[test]
fn t74_cancellation_bridge_supports_three_modes() {
    let cancel_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("asupersync-tokio-compat/src/cancel.rs");
    let cancel = std::fs::read_to_string(cancel_path).expect("compat crate cancel.rs must exist");

    assert!(
        cancel.contains("CancelAware"),
        "must define CancelAware wrapper"
    );
    assert!(
        cancel.contains("CancelResult"),
        "must define CancelResult enum"
    );

    for mode in ["BestEffort", "Strict", "TimeoutFallback"] {
        assert!(
            cancel.contains(mode),
            "cancellation bridge must support {mode} mode"
        );
    }

    // INV-3: Cancellation is a protocol.
    assert!(
        cancel.contains("request_cancel"),
        "must expose request_cancel for protocol propagation"
    );
}
