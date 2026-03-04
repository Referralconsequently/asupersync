#![allow(missing_docs)]
#![allow(clippy::items_after_statements)]

//! [T9.2] Domain-specific migration cookbook enforcement.
//!
//! Validates the migration cookbooks document covers all 6 capability tracks,
//! includes before/after examples, anti-patterns, log expectations, and
//! evidence links to existing test/doc artifacts.
//!
//! Organisation:
//!   1. Document existence and structure
//!   2. Track coverage (T2..T7)
//!   3. Recipe presence per track
//!   4. Anti-pattern and failure mode documentation
//!   5. Evidence link validation
//!   6. Cross-cutting concerns
//!   7. User-friction assumptions
//!   8. Prerequisite and downstream references

#[macro_use]
mod common;

use common::init_test_logging;

use std::path::Path;

fn init_test(name: &str) {
    init_test_logging();
    test_phase!(name);
}

fn cookbook_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/tokio_migration_cookbooks.md")
}

fn load_cookbook() -> String {
    std::fs::read_to_string(cookbook_path()).expect("cookbook doc must exist")
}

// ============================================================================
// Tests: Section 1 - Document existence and structure
// ============================================================================

#[test]
fn t92_01_cookbook_exists_and_is_substantial() {
    init_test("t92_01_cookbook_exists_and_is_substantial");

    assert!(cookbook_path().exists(), "cookbook doc must exist");
    let doc = load_cookbook();
    assert!(doc.len() > 5000, "cookbook must be substantial (>5000 chars)");

    test_complete!("t92_01_cookbook_exists_and_is_substantial");
}

#[test]
fn t92_02_cookbook_references_bead_and_program() {
    init_test("t92_02_cookbook_references_bead_and_program");

    let doc = load_cookbook();
    assert!(doc.contains("asupersync-2oh2u.11.2"), "must reference bead");
    assert!(doc.contains("[T9.2]"), "must reference T9.2");

    test_complete!("t92_02_cookbook_references_bead_and_program");
}

#[test]
fn t92_03_cookbook_has_uniform_structure() {
    init_test("t92_03_cookbook_has_uniform_structure");

    let doc = load_cookbook();
    assert!(doc.contains("Cookbook Structure"), "must describe structure");
    assert!(doc.contains("Migration Recipes"), "must mention recipes");
    assert!(doc.contains("Before/After") || doc.contains("Before"), "must have examples");
    assert!(doc.contains("Anti-Pattern"), "must cover anti-patterns");
    assert!(doc.contains("Evidence"), "must have evidence links");

    test_complete!("t92_03_cookbook_has_uniform_structure");
}

// ============================================================================
// Tests: Section 2 - Track coverage
// ============================================================================

#[test]
fn t92_04_all_six_tracks_covered() {
    init_test("t92_04_all_six_tracks_covered");

    let doc = load_cookbook();

    for (track, domain) in [
        ("T2", "I/O"),
        ("T3", "fs"),
        ("T4", "QUIC"),
        ("T5", "Web"),
        ("T6", "Database"),
        ("T7", "Interop"),
    ] {
        test_section!(track);
        assert!(
            doc.contains(&format!("Track {track}")) || doc.contains(track),
            "missing track: {track} ({domain})"
        );
    }

    test_complete!("t92_04_all_six_tracks_covered");
}

#[test]
fn t92_05_each_track_has_domain_overview() {
    init_test("t92_05_each_track_has_domain_overview");

    let doc = load_cookbook();
    let overview_count = doc.matches("Domain Overview").count();
    assert!(
        overview_count >= 6,
        "each track must have a Domain Overview section, found {overview_count}"
    );

    test_complete!("t92_05_each_track_has_domain_overview");
}

// ============================================================================
// Tests: Section 3 - Recipe presence per track
// ============================================================================

#[test]
fn t92_06_track_recipes_use_consistent_naming() {
    init_test("t92_06_track_recipes_use_consistent_naming");

    let doc = load_cookbook();

    for prefix in ["R2-", "R3-", "R4-", "R5-", "R6-", "R7-"] {
        test_section!(prefix);
        let count = doc.matches(prefix).count();
        assert!(
            count >= 5,
            "track {prefix} must have at least 5 recipes, found {count}"
        );
    }

    test_complete!("t92_06_track_recipes_use_consistent_naming");
}

#[test]
fn t92_07_recipes_include_from_and_to_columns() {
    init_test("t92_07_recipes_include_from_and_to_columns");

    let doc = load_cookbook();
    // Each recipe table should have From and To headers
    let from_count = doc.matches("| From").count();
    let to_count = doc.matches("| To").count();
    assert!(from_count >= 6, "each track recipe table needs From column");
    assert!(to_count >= 6, "each track recipe table needs To column");

    test_complete!("t92_07_recipes_include_from_and_to_columns");
}

// ============================================================================
// Tests: Section 4 - Anti-patterns
// ============================================================================

#[test]
fn t92_08_each_track_has_anti_patterns() {
    init_test("t92_08_each_track_has_anti_patterns");

    let doc = load_cookbook();

    for prefix in ["AP-T2-", "AP-T3-", "AP-T4-", "AP-T5-", "AP-T6-", "AP-T7-"] {
        test_section!(prefix);
        let count = doc.matches(prefix).count();
        assert!(
            count >= 2,
            "track {prefix} must have at least 2 anti-patterns, found {count}"
        );
    }

    test_complete!("t92_08_each_track_has_anti_patterns");
}

// ============================================================================
// Tests: Section 5 - Evidence links
// ============================================================================

#[test]
fn t92_09_evidence_links_reference_test_files() {
    init_test("t92_09_evidence_links_reference_test_files");

    let doc = load_cookbook();
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));

    for test_file in [
        "tests/web_grpc_e2e_service_scripts.rs",
        "tests/web_grpc_exhaustive_unit.rs",
        "tests/tokio_interop_e2e_scenarios.rs",
        "tests/e2e_t6_data_path.rs",
        "tests/t6_database_messaging_unit_matrix.rs",
    ] {
        test_section!(test_file);
        let stem = test_file
            .strip_prefix("tests/")
            .unwrap()
            .strip_suffix(".rs")
            .unwrap();
        assert!(doc.contains(stem), "must reference {test_file}");
        assert!(base.join(test_file).exists(), "file must exist: {test_file}");
    }

    test_complete!("t92_09_evidence_links_reference_test_files");
}

#[test]
fn t92_10_evidence_links_reference_docs() {
    init_test("t92_10_evidence_links_reference_docs");

    let doc = load_cookbook();

    for doc_ref in [
        "tokio_web_grpc_migration_runbook",
        "tokio_web_grpc_parity_map",
        "tokio_interop_support_matrix",
        "tokio_adapter_boundary_architecture",
    ] {
        test_section!(doc_ref);
        assert!(doc.contains(doc_ref), "must reference {doc_ref}");
    }

    test_complete!("t92_10_evidence_links_reference_docs");
}

#[test]
fn t92_11_golden_corpus_referenced() {
    init_test("t92_11_golden_corpus_referenced");

    let doc = load_cookbook();
    assert!(
        doc.contains("golden") || doc.contains("logging_golden_corpus"),
        "must reference golden log corpus"
    );

    test_complete!("t92_11_golden_corpus_referenced");
}

// ============================================================================
// Tests: Section 6 - Cross-cutting concerns
// ============================================================================

#[test]
fn t92_12_structured_logging_requirements_documented() {
    init_test("t92_12_structured_logging_requirements_documented");

    let doc = load_cookbook();
    assert!(
        doc.contains("Structured Logging") || doc.contains("structured log"),
        "must document structured logging requirements"
    );
    assert!(
        doc.contains("schema") || doc.contains("schema_version"),
        "must reference log schema"
    );

    test_complete!("t92_12_structured_logging_requirements_documented");
}

#[test]
fn t92_13_correlation_id_propagation_documented() {
    init_test("t92_13_correlation_id_propagation_documented");

    let doc = load_cookbook();
    assert!(
        doc.contains("Correlation ID") || doc.contains("correlation"),
        "must document correlation ID propagation"
    );

    test_complete!("t92_13_correlation_id_propagation_documented");
}

#[test]
fn t92_14_rollback_decision_points_documented() {
    init_test("t92_14_rollback_decision_points_documented");

    let doc = load_cookbook();
    assert!(
        doc.contains("Rollback") || doc.contains("rollback"),
        "must document rollback decision points"
    );

    test_complete!("t92_14_rollback_decision_points_documented");
}

// ============================================================================
// Tests: Section 7 - User-friction assumptions
// ============================================================================

#[test]
fn t92_15_user_friction_assumptions_present() {
    init_test("t92_15_user_friction_assumptions_present");

    let doc = load_cookbook();
    assert!(
        doc.contains("User-Friction") || doc.contains("friction"),
        "must document user-friction assumptions"
    );
    assert!(
        doc.contains("Threshold") || doc.contains("threshold"),
        "must define measurable thresholds"
    );

    test_complete!("t92_15_user_friction_assumptions_present");
}

#[test]
fn t92_16_friction_assumptions_are_measurable() {
    init_test("t92_16_friction_assumptions_are_measurable");

    let doc = load_cookbook();
    // Must include quantitative thresholds
    assert!(
        doc.contains("min") || doc.contains('<') || doc.contains('>') || doc.contains('%'),
        "friction assumptions must include quantitative thresholds"
    );

    test_complete!("t92_16_friction_assumptions_are_measurable");
}

// ============================================================================
// Tests: Section 8 - Prerequisites and downstream
// ============================================================================

#[test]
fn t92_17_prerequisites_referenced() {
    init_test("t92_17_prerequisites_referenced");

    let doc = load_cookbook();

    for bead in [
        "asupersync-2oh2u.10.13",
        "asupersync-2oh2u.2.10",
        "asupersync-2oh2u.11.1",
    ] {
        test_section!(bead);
        assert!(doc.contains(bead), "must reference prerequisite: {bead}");
    }

    test_complete!("t92_17_prerequisites_referenced");
}

#[test]
fn t92_18_downstream_references_present() {
    init_test("t92_18_downstream_references_present");

    let doc = load_cookbook();
    assert!(
        doc.contains("asupersync-2oh2u.11.10") || doc.contains("T9.10"),
        "must reference downstream T9.10"
    );

    test_complete!("t92_18_downstream_references_present");
}

// ============================================================================
// Tests: Section 9 - Code examples
// ============================================================================

#[test]
fn t92_19_has_code_examples() {
    init_test("t92_19_has_code_examples");

    let doc = load_cookbook();
    let code_fences = doc.matches("```").count();
    assert!(
        code_fences >= 4,
        "must have at least 2 code blocks (4 fences), found {code_fences}"
    );

    test_complete!("t92_19_has_code_examples");
}

#[test]
fn t92_20_code_examples_show_before_after() {
    init_test("t92_20_code_examples_show_before_after");

    let doc = load_cookbook();
    assert!(
        doc.contains("// Before") && doc.contains("// After"),
        "must have before/after code comments"
    );

    test_complete!("t92_20_code_examples_show_before_after");
}

// ============================================================================
// Tests: Section 10 - CI and quality
// ============================================================================

#[test]
fn t92_21_ci_commands_present() {
    init_test("t92_21_ci_commands_present");

    let doc = load_cookbook();
    assert!(doc.contains("cargo test"), "must include cargo test commands");
    assert!(doc.contains("rch exec"), "must include rch exec");

    test_complete!("t92_21_ci_commands_present");
}

#[test]
fn t92_22_recipe_tables_have_minimum_rows() {
    init_test("t92_22_recipe_tables_have_minimum_rows");

    let doc = load_cookbook();
    let table_rows = doc.lines().filter(|l| l.contains("|--")).count();
    assert!(
        table_rows >= 8,
        "must have at least 8 markdown tables, found {table_rows}"
    );

    test_complete!("t92_22_recipe_tables_have_minimum_rows");
}

// ============================================================================
// Tests: Section 11 - Compat adapter documentation
// ============================================================================

#[test]
fn t92_23_compat_adapter_migration_path() {
    init_test("t92_23_compat_adapter_migration_path");

    let doc = load_cookbook();
    assert!(
        doc.contains("compat") || doc.contains("adapter"),
        "must document tokio-compat adapter for incremental migration"
    );
    assert!(
        doc.contains("asupersync-tokio-compat"),
        "must reference the compat crate"
    );

    test_complete!("t92_23_compat_adapter_migration_path");
}

#[test]
fn t92_24_structured_concurrency_migration() {
    init_test("t92_24_structured_concurrency_migration");

    let doc = load_cookbook();
    assert!(
        doc.contains("structured concurrency") || doc.contains("regions"),
        "must mention structured concurrency as tokio::spawn replacement"
    );

    test_complete!("t92_24_structured_concurrency_migration");
}

#[test]
fn t92_25_cookbook_scope_table_has_all_tracks() {
    init_test("t92_25_cookbook_scope_table_has_all_tracks");

    let doc = load_cookbook();
    // Verify the scope table lists all 6 tracks with their domains
    for domain in [
        "Async I/O",
        "fs/process/signal",
        "QUIC",
        "Web/gRPC",
        "Database",
        "Interop",
    ] {
        test_section!(domain);
        assert!(doc.contains(domain), "scope table missing domain: {domain}");
    }

    test_complete!("t92_25_cookbook_scope_table_has_all_tracks");
}
