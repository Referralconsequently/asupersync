//! Contract checks for the service-worker bounded broker policy
//! (`asupersync-n6kwt.7.1`).

use std::path::PathBuf;

const DOC_PATH: &str = "docs/wasm_service_worker_broker_contract.md";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_file(path: &str) -> String {
    let path = repo_root().join(path);
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    assert!(
        !content.is_empty(),
        "expected non-empty contract file at {}",
        path.display()
    );
    content
}

#[test]
fn doc_exists_and_pins_bead_and_contract_id() {
    assert!(
        repo_root().join(DOC_PATH).exists(),
        "contract doc must exist at repo-relative path"
    );
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
fn doc_and_browser_package_pin_bounded_broker_exports() {
    let doc = read_file(DOC_PATH);
    for marker in [
        "detectBrowserServiceWorkerBrokerSupport()",
        "BrowserServiceWorkerBrokerStore",
        "createBrowserServiceWorkerBrokerStore()",
        "registerBroker()",
        "persistBrokerWork()",
        "persistDurableHandoff()",
        "lane.browser.service_worker.broker",
    ] {
        assert!(
            doc.contains(marker),
            "doc missing broker API marker: {marker}"
        );
    }

    let browser = read_file("packages/browser/src/index.ts");
    for marker in [
        "BROWSER_SERVICE_WORKER_BROKER_CONTRACT_ID",
        "BROWSER_SERVICE_WORKER_BROKER_LANE",
        "export interface BrowserServiceWorkerBrokerAdmissionTuple",
        "export interface BrowserServiceWorkerBrokerSupportDiagnostics",
        "export function detectBrowserServiceWorkerBrokerSupport(",
        "export class BrowserServiceWorkerBrokerStore",
        "registerBroker(",
        "persistBrokerWork(",
        "persistDurableHandoff(",
        "createBrowserServiceWorkerBrokerStore(",
    ] {
        assert!(
            browser.contains(marker),
            "browser package missing bounded broker marker: {marker}"
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
fn doc_and_browser_package_pin_durable_handoff_listing_and_key_schema() {
    let doc = read_file(DOC_PATH);
    for marker in [
        "listDurableHandoffs()",
        "__service_worker_broker_registration__",
        "broker_work:<broker_work_id>",
        "broker_handoff:<broker_work_id>",
        "service_worker_broker_v1",
        "newest-first order",
    ] {
        assert!(
            doc.contains(marker),
            "doc missing durable handoff/key-schema marker: {marker}"
        );
    }

    let browser = read_file("packages/browser/src/index.ts");
    for marker in [
        "BROWSER_SERVICE_WORKER_BROKER_REGISTRATION_KEY",
        "BROWSER_SERVICE_WORKER_BROKER_WORK_PREFIX",
        "BROWSER_SERVICE_WORKER_BROKER_HANDOFF_PREFIX",
        "DEFAULT_BROWSER_SERVICE_WORKER_BROKER_NAMESPACE",
        "async listDurableHandoffs()",
        "right.recordedAtMs - left.recordedAtMs",
    ] {
        assert!(
            browser.contains(marker),
            "browser package missing durable handoff/key-schema marker: {marker}"
        );
    }
}

#[test]
fn browser_package_fail_closes_foreign_requested_lane_in_durable_broker_records() {
    let browser = read_file("packages/browser/src/index.ts");
    for marker in [
        "function parseBrowserServiceWorkerBrokerDescriptor(",
        "function parseBrowserServiceWorkerBrokerHandoffRecord(",
        "candidate.requestedLane !== BROWSER_SERVICE_WORKER_BROKER_LANE",
        "service-worker broker descriptor is missing required fields",
        "service-worker broker handoff record is missing required fields",
    ] {
        assert!(
            browser.contains(marker),
            "browser package missing requested-lane fail-closed marker: {marker}"
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
        wasm.contains("detectBrowserServiceWorkerBrokerSupport()"),
        "WASM guide must reference the bounded broker support helper"
    );
    assert!(
        wasm.contains("BrowserServiceWorkerBrokerStore"),
        "WASM guide must reference the bounded broker store"
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
        integration.contains("registerBroker()"),
        "integration guide must reference broker registration"
    );
    assert!(
        integration.contains("persistDurableHandoff()"),
        "integration guide must reference durable handoff"
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

#[test]
fn maintained_service_worker_broker_fixture_exists_with_required_files() {
    let fixture = repo_root().join("tests/fixtures/service-worker-broker-consumer");
    assert!(
        fixture.exists(),
        "service-worker broker fixture directory must exist"
    );

    for rel in [
        "README.md",
        "package.json",
        "index.html",
        "vite.config.ts",
        "src/main.ts",
        "src/service-worker.ts",
        "scripts/check-bundle.mjs",
        "scripts/check-browser-run.mjs",
    ] {
        let path = fixture.join(rel);
        assert!(path.exists(), "missing fixture file: {}", path.display());
    }
}

#[test]
fn maintained_service_worker_broker_validation_path_is_pinned() {
    let script = read_file("scripts/validate_service_worker_broker_consumer.sh");
    for marker in [
        "tests/fixtures/service-worker-broker-consumer",
        "target/e2e-results/service_worker_broker_consumer",
        "BROWSER_RUN_FILE",
        "npm run build",
        "npm run check:bundle",
        "npm run check:browser -- \"${BROWSER_RUN_FILE}\"",
        "\"real_browser_run_ok\": browser_run[\"status\"] == \"ok\"",
        "\"browser_broker_supported\": browser_run[\"broker_supported\"] is True",
        "\"browser_direct_execution_reason\": browser_run[\"direct_execution_reason_code\"]",
        "\"browser_handoff_target_lane_id\": browser_run[\"handoff_target_lane_id\"]",
        "\"browser_mismatch_reason\": browser_run[\"mismatch_reason\"]",
        "L6-SERVICE-WORKER-BROKER",
        "asupersync-n6kwt.7.2",
    ] {
        assert!(
            script.contains(marker),
            "validation script missing expected marker: {marker}"
        );
    }

    let fixture_readme = read_file("tests/fixtures/service-worker-broker-consumer/README.md");
    for marker in [
        "scripts/validate_service_worker_broker_consumer.sh",
        "detectBrowserServiceWorkerBrokerSupport()",
        "BrowserServiceWorkerBrokerStore",
        "registerBroker()",
        "persistBrokerWork()",
        "persistDurableHandoff()",
        "check-browser-run.mjs",
    ] {
        assert!(
            fixture_readme.contains(marker),
            "fixture readme missing expected marker: {marker}"
        );
    }

    let main = read_file("tests/fixtures/service-worker-broker-consumer/src/main.ts");
    for marker in [
        "navigator.serviceWorker.register",
        "service-worker-broker-ready",
        "\"cleanup_complete\"",
        "\"run-broker-demo\"",
    ] {
        assert!(
            main.contains(marker),
            "fixture main source missing expected marker: {marker}"
        );
    }

    let service_worker =
        read_file("tests/fixtures/service-worker-broker-consumer/src/service-worker.ts");
    for marker in [
        "detectBrowserServiceWorkerBrokerSupport",
        "createBrowserServiceWorkerBrokerStore",
        "registerBroker",
        "persistBrokerWork",
        "persistDurableHandoff",
        "listDurableHandoffs",
        "service-worker-broker-mismatch",
        "service_worker_direct_runtime_not_shipped",
    ] {
        assert!(
            service_worker.contains(marker),
            "fixture service-worker source missing expected marker: {marker}"
        );
    }

    let vite = read_file("tests/fixtures/service-worker-broker-consumer/vite.config.ts");
    for marker in ["base: \"./\"", "service-worker.js", "src/service-worker.ts"] {
        assert!(
            vite.contains(marker),
            "fixture vite config missing expected marker: {marker}"
        );
    }

    let browser_check =
        read_file("tests/fixtures/service-worker-broker-consumer/scripts/check-browser-run.mjs");
    for marker in [
        "import { chromium } from \"playwright-core\";",
        "SERVICE-WORKER-BROKER-CONSUMER",
        "service_worker_direct_runtime_not_shipped",
        "broker_protocol_version_mismatch",
        "cleanup_complete",
        "service-worker-broker-ready",
    ] {
        assert!(
            browser_check.contains(marker),
            "fixture browser-run checker missing expected marker: {marker}"
        );
    }
}
