//! Contract tests for the fs/process/signal parity matrix (2oh2u.3.1).
//!
//! Validates matrix completeness, gap/ownership/evidence mapping, and
//! platform-specific divergence coverage.

#![allow(missing_docs)]

use std::collections::BTreeSet;
use std::path::Path;

fn load_matrix_doc() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/tokio_fs_process_signal_parity_matrix.md");
    std::fs::read_to_string(path).expect("matrix document must exist")
}

fn extract_gap_ids(doc: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in doc.lines() {
        let trimmed = line.trim().trim_start_matches('|').trim();
        if let Some(id) = trimmed.split('|').next() {
            let id = id
                .trim()
                .trim_matches('`')
                .trim_matches('*')
                .trim_end_matches(':');
            let prefixes = ["FS-G", "PR-G", "SG-G"];
            if prefixes.iter().any(|p| id.starts_with(p)) && id.len() >= 5 {
                ids.insert(id.to_string());
            }
        }
    }
    ids
}

#[test]
fn matrix_document_exists_and_is_substantial() {
    let doc = load_matrix_doc();
    assert!(
        doc.len() > 2000,
        "matrix document should be substantial, got {} bytes",
        doc.len()
    );
}

#[test]
fn matrix_references_correct_bead() {
    let doc = load_matrix_doc();
    assert!(
        doc.contains("asupersync-2oh2u.3.1"),
        "document must reference bead 2oh2u.3.1"
    );
    assert!(doc.contains("[T3.1]"), "document must reference T3.1");
}

#[test]
fn matrix_covers_tokio_fs_process_signal_surfaces() {
    let doc = load_matrix_doc();
    for token in ["tokio::fs", "tokio::process", "tokio::signal"] {
        assert!(doc.contains(token), "matrix must reference {token}");
    }
}

#[test]
fn matrix_covers_expected_asupersync_owner_modules() {
    let doc = load_matrix_doc();
    for token in [
        "src/fs/file.rs",
        "src/fs/path_ops.rs",
        "src/process.rs",
        "src/signal/signal.rs",
        "src/signal/ctrl_c.rs",
        "src/signal/shutdown.rs",
    ] {
        assert!(
            doc.contains(token),
            "matrix missing owner module token: {token}"
        );
    }
}

#[test]
fn matrix_includes_platform_specific_semantics_section() {
    let doc = load_matrix_doc();
    assert!(
        doc.contains("Platform-Specific Semantics Matrix"),
        "must include platform-specific semantics section"
    );
    for token in ["Unix", "Windows", "WASM", "Known Divergence Risk"] {
        assert!(
            doc.contains(token),
            "platform semantics matrix missing token: {token}"
        );
    }
}

#[test]
fn matrix_has_gap_entries_for_all_three_domains() {
    let doc = load_matrix_doc();
    let ids = extract_gap_ids(&doc);

    let domain_prefixes = [("FS-G", 5usize), ("PR-G", 4usize), ("SG-G", 4usize)];
    for (prefix, min_count) in &domain_prefixes {
        let count = ids.iter().filter(|id| id.starts_with(prefix)).count();
        assert!(
            count >= *min_count,
            "domain {prefix} must have >= {min_count} gaps, found {count}"
        );
    }

    assert!(
        ids.len() >= 13,
        "matrix should identify >=13 total gaps, found {}",
        ids.len()
    );
}

#[test]
fn matrix_maps_track_level_gaps_g8_g12_g13() {
    let doc = load_matrix_doc();
    for token in ["G8", "G12", "G13"] {
        assert!(
            doc.contains(token),
            "matrix must map track-level gap token: {token}"
        );
    }
}

#[test]
fn matrix_includes_owner_and_evidence_columns_in_gap_registers() {
    let doc = load_matrix_doc();
    for token in ["Owner Modules", "Evidence Requirements", "Downstream Bead"] {
        assert!(
            doc.contains(token),
            "gap register missing required column token: {token}"
        );
    }
}

#[test]
fn matrix_references_current_evidence_artifacts() {
    let doc = load_matrix_doc();
    for token in [
        "tests/fs_verification.rs",
        "tests/e2e_fs.rs",
        "tests/compile_test_process.rs",
        "tests/e2e_signal.rs",
    ] {
        assert!(
            doc.contains(token),
            "matrix missing evidence token: {token}"
        );
    }
}

#[test]
fn matrix_execution_mapping_points_to_t3_followups() {
    let doc = load_matrix_doc();
    for token in [
        "2oh2u.3.2",
        "2oh2u.3.4",
        "2oh2u.3.5",
        "2oh2u.3.6",
        "2oh2u.3.7",
    ] {
        assert!(
            doc.contains(token),
            "execution mapping missing followup task token: {token}"
        );
    }
}
