//! Contract checks for the service-worker bounded broker policy
//! (`asupersync-n6kwt.7.1`).

use std::path::{Path, PathBuf};

const DOC_PATH: &str = "docs/wasm_service_worker_broker_contract.md";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_file(path: &str) -> String {
    let path = repo_root().join(path);
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        !content.is_empty(),
        "expected non-empty contract file at {}",
        path.display()
    );
    content
}

#[test]
fn doc_exists_and_pins_bead_and_contract_id() {
    assert!(Path::new(DOC_PATH).exists(), "contract doc must exist");
    let doc = read_file(DOC_PATH);
    for marker in [
        "asupersync-n6kwt.7.1",
        "wasm-service-worker-broker-contract-v1",
        "Current Truthful Runtime Status",
        "packages/browser/src/index.ts",
        "src/runtime/builder.rs",
    ] {
        assert!(doc.contains(marker), "doc missing marker: {marker}");
    }
}

#[test]
fn doc_scopes_bounded_broker_responsibilities() {
    let doc = read_file(DOC_PATH);
    for marker in [
        "Allowed broker responsibilities",
        "serialize fetch/push/sync/notification ingress",
        "explicit broker work",
        "persist durable broker manifests before claiming restartable progress",
        "hand work off to a dedicated worker, browser main thread, or explicit",
        "general-purpose always-alive runtime",
        "unbounded queue or broker-of-last-resort",
    ] {
        assert!(doc.contains(marker), "doc missing broker marker: {marker}");
    }
}

#[test]
fn doc_distinguishes_ephemeral_and_durable_state() {
    let doc = read_file(DOC_PATH);
    for marker in [
        "Ephemeral broker state",
        "Durable state",
        "IndexedDB-backed BrowserStorage / BrowserArtifactStore",
        "pending broker work descriptors and idempotency keys",
        "artifact manifests and retained evidence indexes",
        "restart reconciliation journal",
        "The service-worker broker owns no irreplaceable authoritative state.",
    ] {
        assert!(doc.contains(marker), "doc missing state marker: {marker}");
    }
}

#[test]
fn doc_pins_restart_reconciliation_and_capability_reestablishment() {
    let doc = read_file(DOC_PATH);
    for marker in [
        "1. `cold_start`",
        "2. `validating_scope`",
        "3. `reconciling_durable_state`",
        "4. `brokering`",
        "5. `draining`",
        "6. `quiescent`",
        "7. `terminated`",
        "capabilities are re-established explicitly",
        "resume is allowed only when the durable descriptor",
        "new capability",
        "snapshot still match",
        "downgrade_to_dedicated_worker",
        "downgrade_to_browser_main_thread",
        "downgrade_to_bridge_only",
        "capability_manifest_mismatch_on_restart",
    ] {
        assert!(
            doc.contains(marker),
            "doc missing reconciliation marker: {marker}"
        );
    }
}

#[test]
fn browser_package_and_runtime_builder_preserve_service_worker_fail_closed_markers() {
    let browser = read_file("packages/browser/src/index.ts");
    for marker in [
        "reason: \"service_worker_not_yet_shipped\"",
        "@asupersync/browser does not yet ship direct runtime APIs for service-worker hosts.",
        "Keep service-worker orchestration at the application boundary until this host is promoted.",
        "return \"service_worker\";",
    ] {
        assert!(
            browser.contains(marker),
            "browser package missing service-worker marker: {marker}"
        );
    }

    let builder = read_file("src/runtime/builder.rs");
    for marker in [
        "Some(\"ServiceWorkerGlobalScope\") => BrowserExecutionHostRole::ServiceWorker",
        "BrowserRuntimeSupportReason::ServiceWorkerNotYetShipped",
        "BrowserExecutionReasonCode::ServiceWorkerDirectRuntimeNotShipped",
        "\"service_worker_direct_runtime_not_shipped\"",
        "Rust Browser Edition does not yet ship a service-worker direct-runtime lane.",
    ] {
        assert!(
            builder.contains(marker),
            "runtime builder missing service-worker marker: {marker}"
        );
    }
}

#[test]
fn canonical_browser_docs_reference_the_contract_and_current_reason_codes() {
    let wasm = read_file("docs/WASM.md");
    let integration = read_file("docs/integration.md");
    let troubleshooting = read_file("docs/wasm_troubleshooting_compendium.md");

    assert!(
        wasm.contains("docs/wasm_service_worker_broker_contract.md"),
        "WASM guide must reference the service-worker contract"
    );
    assert!(
        wasm.contains("service_worker_not_yet_shipped"),
        "WASM guide must preserve the package-level service-worker denial reason"
    );
    assert!(
        wasm.contains("service_worker_direct_runtime_not_shipped"),
        "WASM guide must preserve the ladder-level service-worker denial reason"
    );
    assert!(
        integration.contains("docs/wasm_service_worker_broker_contract.md"),
        "integration guide must reference the service-worker contract"
    );
    assert!(
        integration.contains("Browser service worker"),
        "integration guide must carry a dedicated service-worker environment row"
    );
    assert!(
        integration.contains("service_worker_not_yet_shipped"),
        "integration guide must use the current browser package reason code"
    );
    assert!(
        troubleshooting.contains("service_worker_not_yet_shipped"),
        "troubleshooting guide must list the current service-worker reason"
    );
    assert!(
        troubleshooting.contains("shared_worker_not_yet_shipped"),
        "troubleshooting guide must list the current shared-worker reason"
    );
    assert!(
        !troubleshooting.contains("missing_browser_dom"),
        "troubleshooting guide must not mention the removed missing_browser_dom reason"
    );
}
