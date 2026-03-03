//! Contract tests for the Async I/O parity audit (2oh2u.2.1).
//!
//! Validates document structure, gap coverage, and semantic analysis completeness.

#![allow(missing_docs)]

use std::collections::BTreeSet;
use std::path::Path;

fn load_audit_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/tokio_io_parity_audit.md");
    std::fs::read_to_string(path).expect("audit document must exist")
}

fn extract_gap_ids(doc: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in doc.lines() {
        let trimmed = line.trim().trim_start_matches('|').trim();
        if let Some(id) = trimmed.split('|').next() {
            let id = id.trim();
            if id.starts_with("IO-G") && id.len() >= 4 {
                ids.insert(id.to_string());
            }
        }
    }
    ids
}

#[test]
fn audit_document_exists_and_is_nonempty() {
    let doc = load_audit_doc();
    assert!(
        doc.len() > 2000,
        "audit document should be substantial, got {} bytes",
        doc.len()
    );
}

#[test]
fn audit_references_correct_bead() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("asupersync-2oh2u.2.1"),
        "document must reference bead 2oh2u.2.1"
    );
    assert!(doc.contains("[T2.1]"), "document must reference T2.1");
}

#[test]
fn audit_covers_tokio_io_surface() {
    let doc = load_audit_doc();
    assert!(doc.contains("tokio::io"), "must reference tokio::io");
    assert!(
        doc.contains("AsyncRead") && doc.contains("AsyncWrite"),
        "must cover core AsyncRead/AsyncWrite traits"
    );
    assert!(
        doc.contains("AsyncBufRead"),
        "must cover AsyncBufRead trait"
    );
    assert!(doc.contains("AsyncSeek"), "must cover AsyncSeek trait");
}

#[test]
fn audit_covers_tokio_util_codec_surface() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("tokio-util") || doc.contains("tokio_util"),
        "must reference tokio-util"
    );
    assert!(
        doc.contains("Decoder") && doc.contains("Encoder"),
        "must cover Decoder/Encoder traits"
    );
    assert!(doc.contains("Framed"), "must cover Framed transport");
    assert!(
        doc.contains("LengthDelimited"),
        "must cover LengthDelimitedCodec"
    );
}

#[test]
fn audit_covers_read_ext_methods() {
    let doc = load_audit_doc();
    let methods = [
        "read_exact",
        "read_to_end",
        "read_to_string",
        "chain",
        "take",
    ];
    for method in &methods {
        assert!(
            doc.contains(method),
            "audit must cover AsyncReadExt::{method}"
        );
    }
}

#[test]
fn audit_covers_write_ext_methods() {
    let doc = load_audit_doc();
    let methods = ["write_all", "flush", "shutdown"];
    for method in &methods {
        assert!(
            doc.contains(method),
            "audit must cover AsyncWriteExt::{method}"
        );
    }
}

#[test]
fn audit_covers_buffered_io() {
    let doc = load_audit_doc();
    assert!(doc.contains("BufReader"), "must cover BufReader");
    assert!(doc.contains("BufWriter"), "must cover BufWriter");
}

#[test]
fn audit_covers_split_ownership() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("split") && doc.contains("into_split"),
        "must cover both split modes (borrowed and owned)"
    );
    assert!(
        doc.contains("ReadHalf") || doc.contains("WriteHalf"),
        "must reference split half types"
    );
}

#[test]
fn audit_covers_vectored_io() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("vectored") || doc.contains("Vectored"),
        "must cover vectored I/O"
    );
    assert!(
        doc.contains("is_write_vectored"),
        "must cover vectored capability check"
    );
}

#[test]
fn audit_covers_eof_behavior() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("EOF") && doc.contains("UnexpectedEof"),
        "must cover EOF behavior semantics"
    );
}

#[test]
fn audit_covers_shutdown_semantics() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("Shutdown Semantics") || doc.contains("poll_shutdown"),
        "must cover shutdown semantics"
    );
}

#[test]
fn audit_covers_cancel_safety() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("Cancel-Safe") || doc.contains("cancel-safe"),
        "must cover cancel-safety analysis"
    );
}

#[test]
fn audit_has_gap_entries() {
    let doc = load_audit_doc();
    let ids = extract_gap_ids(&doc);
    assert!(
        ids.len() >= 10,
        "audit must identify >= 10 I/O gaps, found {}",
        ids.len()
    );
}

#[test]
fn audit_classifies_gap_severity() {
    let doc = load_audit_doc();
    for level in &["High", "Medium", "Low"] {
        assert!(
            doc.contains(level),
            "audit must use severity level: {level}"
        );
    }
}

#[test]
fn audit_has_gap_summary_with_phases() {
    let doc = load_audit_doc();
    assert!(doc.contains("Gap Summary"), "must have gap summary section");
    let phase_count = ["Phase A", "Phase B", "Phase C", "Phase D"]
        .iter()
        .filter(|p| doc.contains(**p))
        .count();
    assert!(
        phase_count >= 3,
        "gap summary must have >= 3 execution phases, found {phase_count}"
    );
}

#[test]
fn audit_covers_codec_types() {
    let doc = load_audit_doc();
    let codecs = ["BytesCodec", "LinesCodec", "LengthDelimitedCodec"];
    for codec in &codecs {
        assert!(doc.contains(codec), "audit must cover codec: {codec}");
    }
}

#[test]
fn audit_covers_stream_adapter_gaps() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("ReaderStream") || doc.contains("StreamReader"),
        "must identify Stream/AsyncRead bridge adapter gaps"
    );
}

#[test]
fn audit_covers_duplex_stream_gap() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("Duplex") || doc.contains("SimplexStream"),
        "must identify in-memory duplex/simplex stream gap"
    );
}

#[test]
fn audit_notes_asupersync_extensions() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("WritePermit"),
        "must note Asupersync-specific WritePermit"
    );
    assert!(
        doc.contains("IoCap") || doc.contains("Capability"),
        "must note capability-based I/O extensions"
    );
}

#[test]
fn audit_covers_integer_read_write_gap() {
    let doc = load_audit_doc();
    assert!(
        doc.contains("read_u16") || doc.contains("read_u32"),
        "must identify missing integer read/write methods"
    );
}
