//! Contract tests for the lab-vs-live verification taxonomy (2a6k9.1.4).
//!
//! Verifies the tier vocabulary, minimum coverage matrix, structured logging
//! field set, downstream bindings, and `rch`-offloaded validation policy.

#![allow(missing_docs)]

use std::path::Path;

fn load_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/lab_live_verification_taxonomy.md");
    std::fs::read_to_string(path).expect("lab-live verification taxonomy must exist")
}

#[test]
fn doc_exists_and_is_substantial() {
    let doc = load_doc();
    assert!(
        doc.len() > 8_000,
        "document should be substantial, got {} bytes",
        doc.len()
    );
}

#[test]
fn doc_references_bead_parent_and_upstream_contracts() {
    let doc = load_doc();
    for token in [
        "asupersync-2a6k9.1.4",
        "asupersync-2a6k9.1",
        "asupersync-2a6k9",
        "docs/lab_live_differential_scope_matrix.md",
        "docs/lab_live_normalized_observable_schema.md",
        "docs/lab_live_divergence_taxonomy.md",
        "docs/lab_live_scenario_adapter_contract.md",
        "docs/tokio_differential_behavior_suites.md",
        "TESTING.md",
        "tests/common/mod.rs",
    ] {
        assert!(
            doc.contains(token),
            "document missing upstream contract token: {token}"
        );
    }
}

#[test]
fn doc_defines_taxonomy_tiers() {
    let doc = load_doc();
    for token in [
        "T0",
        "unit_contract",
        "T1",
        "golden_fixture",
        "T2",
        "dual_run_smoke",
        "T3",
        "pilot_surface",
        "T4",
        "negative_control",
        "T5",
        "stress_nightly",
    ] {
        assert!(
            doc.contains(token),
            "document missing taxonomy tier token: {token}"
        );
    }
}

#[test]
fn doc_defines_minimum_bead_class_requirements() {
    let doc = load_doc();
    for token in [
        "Policy or Contract Beads",
        "Shared Harness or Helper Beads",
        "Live Evidence, Normalizer, and Comparator Beads",
        "Pilot Surface Beads",
        "Expansion, Eligibility, and Operations Beads",
    ] {
        assert!(
            doc.contains(token),
            "document missing bead-class section: {token}"
        );
    }
}

#[test]
fn doc_locks_phase1_surface_coverage_matrix() {
    let doc = load_doc();
    for token in [
        "cancellation -> combinators -> channels -> obligations -> region close/quiescence",
        "cancellation",
        "combinators",
        "channels",
        "obligations",
        "region_close",
        "quiescence",
    ] {
        assert!(
            doc.contains(token),
            "document missing phase-1 coverage token: {token}"
        );
    }
}

#[test]
fn doc_defines_external_surface_gate_requirements() {
    let doc = load_doc();
    for token in [
        "Eligibility-Gate Matrix for External Surfaces",
        "raw_socket",
        "http_surface",
        "browser_surface",
        "eligibility_verdict",
        "virtualization_boundary",
        "observability_status",
        "capture_manifest_path",
        "unsupported_reason",
        "host_role",
        "support_class",
        "reason_code",
        "lane_id",
    ] {
        assert!(
            doc.contains(token),
            "document missing external-surface gate token: {token}"
        );
    }
}

#[test]
fn doc_requires_structured_logging_vocabulary() {
    let doc = load_doc();
    for token in [
        "schema_version",
        "suite_id",
        "scenario_id",
        "surface_id",
        "surface_contract_version",
        "seed_lineage_id",
        "runtime_kind",
        "runner_profile",
        "adapter",
        "attempt_index",
        "rerun_count",
        "divergence_class",
        "policy_class",
        "normalized_record_path",
        "artifact_bundle",
        "repro_command",
        "terminal_outcome",
        "loser_drain",
        "obligation_balance",
    ] {
        assert!(
            doc.contains(token),
            "document missing structured logging token: {token}"
        );
    }
}

#[test]
fn doc_requires_bundle_file_names() {
    let doc = load_doc();
    for token in [
        "differential_summary.json",
        "differential_event_log.jsonl",
        "differential_failures.json",
        "differential_deviations.json",
        "differential_repro_manifest.json",
        "lab_normalized.json",
        "live_normalized.json",
    ] {
        assert!(
            doc.contains(token),
            "document missing artifact bundle token: {token}"
        );
    }
}

#[test]
fn doc_binds_downstream_beads() {
    let doc = load_doc();
    for token in [
        "asupersync-2a6k9.2.2",
        "asupersync-2a6k9.2.5",
        "asupersync-2a6k9.4.5",
        "asupersync-2a6k9.5.4",
        "asupersync-2a6k9.5.5",
        "asupersync-2a6k9.6.6",
        "asupersync-2a6k9.7.3",
        "asupersync-2a6k9.8.*",
    ] {
        assert!(
            doc.contains(token),
            "document missing downstream binding token: {token}"
        );
    }
}

#[test]
fn doc_requires_rch_validation_commands() {
    let doc = load_doc();
    for token in [
        "rch exec -- cargo fmt --check",
        "rch exec -- cargo check --all-targets",
        "rch exec -- cargo clippy --all-targets -- -D warnings",
        "rch exec -- cargo test --test lab_live_verification_taxonomy_contract -- --nocapture",
    ] {
        assert!(
            doc.contains(token),
            "document missing validation command: {token}"
        );
    }
}
