//! Contract tests for the lab-vs-live scenario adapter contract (2a6k9.2.1).
//!
//! Verifies the shared ScenarioSpec shape, adapter boundary, valid/invalid
//! examples, downstream bindings, and `rch`-offloaded validation policy.

#![allow(missing_docs)]

use std::path::Path;

fn load_doc() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/lab_live_scenario_adapter_contract.md");
    std::fs::read_to_string(path).expect("lab-live scenario adapter contract must exist")
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
fn doc_references_bead_and_dependencies() {
    let doc = load_doc();
    for token in [
        "asupersync-2a6k9.2.1",
        "asupersync-2a6k9.2",
        "asupersync-2a6k9",
        "docs/lab_live_differential_scope_matrix.md",
        "docs/lab_live_normalized_observable_schema.md",
        "docs/tokio_differential_behavior_suites.md",
        "src/lab/scenario.rs",
        "src/lab/scenario_runner.rs",
        "src/lab/spork_harness.rs",
        "tests/common/mod.rs",
        "docs/integration.md",
    ] {
        assert!(
            doc.contains(token),
            "document missing dependency token: {token}"
        );
    }
}

#[test]
fn doc_defines_shared_scenario_contract_and_version() {
    let doc = load_doc();
    for token in [
        "DualRunScenarioSpec",
        "schema_version = \"lab-live-scenario-spec-v1\"",
        "lab-live-scenario-spec-v1",
        "ScenarioSpec intent -> lab adapter / live adapter -> normalized observable -> comparator",
    ] {
        assert!(
            doc.contains(token),
            "document missing scenario-contract token: {token}"
        );
    }
}

#[test]
fn doc_names_required_top_level_fields() {
    let doc = load_doc();
    for token in [
        "scenario_id",
        "surface_id",
        "surface_contract_version",
        "seed_plan",
        "participants",
        "setup",
        "operations",
        "perturbations",
        "expectations",
        "lab_binding",
        "live_binding",
        "artifacts",
    ] {
        assert!(
            doc.contains(token),
            "document missing top-level field token: {token}"
        );
    }
}

#[test]
fn doc_bridges_real_existing_code_surfaces() {
    let doc = load_doc();
    for token in [
        "src/lab/scenario.rs::Scenario",
        "src/lab/scenario_runner.rs::ScenarioRunner",
        "src/lab/spork_harness.rs::SporkScenarioSpec",
        "SporkScenarioRunner",
        "RuntimeBuilder::current_thread()",
        "run_test_with_cx(...)",
        "live.current_thread",
        "lab.scenario_runner",
        "lab.spork_harness",
    ] {
        assert!(
            doc.contains(token),
            "document missing adapter-boundary token: {token}"
        );
    }
}

#[test]
fn doc_requires_normalized_schema_bridge() {
    let doc = load_doc();
    for token in [
        "lab-live-normalized-observable-v1",
        "terminal_outcome",
        "cancellation",
        "loser_drain",
        "region_close",
        "obligation_balance",
        "resource_surface",
        "allowed_provenance_variance",
    ] {
        assert!(
            doc.contains(token),
            "document missing normalized-schema token: {token}"
        );
    }
}

#[test]
fn doc_includes_valid_example_and_expected_normalized_record() {
    let doc = load_doc();
    for token in [
        "phase1.cancel.race.one_loser",
        "cancel.race.v1",
        "seed.phase1.cancel.race.one_loser.v1",
        "winner=fast_branch",
        "\"schema_version\": \"lab-live-normalized-observable-v1\"",
        "\"surface_id\": \"cancel.race\"",
        "\"status\": \"complete\"",
        "\"balanced\": true",
    ] {
        assert!(
            doc.contains(token),
            "document missing valid-example token: {token}"
        );
    }
}

#[test]
fn doc_includes_invalid_examples_and_rejection_reasons() {
    let doc = load_doc();
    for token in [
        "invalid.missing_live_binding",
        "invalid.real_network_claim",
        "missing_adapter_binding",
        "unsupported_surface",
        "not_comparison_ready",
        "artifact_schema_violation",
    ] {
        assert!(
            doc.contains(token),
            "document missing invalid-example token: {token}"
        );
    }
}

#[test]
fn doc_names_validation_rules_and_non_goals() {
    let doc = load_doc();
    for token in [
        "seed_lineage_violation",
        "semantic_expectation_gap",
        "new universal scheduler DSL",
        "raw OS or real-network parity claims",
        "browser ambient behavior parity",
        "ad hoc per-surface one-off harness contracts",
    ] {
        assert!(
            doc.contains(token),
            "document missing validation or non-goal token: {token}"
        );
    }
}

#[test]
fn doc_binds_downstream_beads_and_validation_commands() {
    let doc = load_doc();
    for token in [
        "asupersync-2a6k9.2.2",
        "asupersync-2a6k9.2.3",
        "asupersync-2a6k9.2.4",
        "asupersync-2a6k9.4.*",
        "asupersync-2a6k9.6.*",
        "rch exec -- cargo fmt --check",
        "rch exec -- cargo check --all-targets",
        "rch exec -- cargo clippy --all-targets -- -D warnings",
        "rch exec -- cargo test --test lab_live_scenario_adapter_contract -- --nocapture",
    ] {
        assert!(
            doc.contains(token),
            "document missing downstream or validation token: {token}"
        );
    }
}
