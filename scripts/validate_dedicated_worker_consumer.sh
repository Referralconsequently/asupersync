#!/usr/bin/env bash
set -euo pipefail

# bead: asupersync-18tbo.4

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
FIXTURE_DIR="${REPO_ROOT}/tests/fixtures/dedicated-worker-consumer"
RESULT_ROOT="${REPO_ROOT}/target/e2e-results/dedicated_worker_consumer"
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="${RESULT_ROOT}/${TIMESTAMP}"
LOG_FILE="${RUN_DIR}/consumer_build.log"
SUMMARY_FILE="${RUN_DIR}/summary.json"

mkdir -p "${RUN_DIR}"

require_cmd() {
  local cmd="$1"
  if ! command -v "${cmd}" >/dev/null 2>&1; then
    echo "FATAL: required command not found: ${cmd}" >&2
    exit 1
  fi
}

require_cmd nodejs
require_cmd npm
require_cmd npx
require_cmd python3

if [[ ! -d "${FIXTURE_DIR}" ]]; then
  echo "FATAL: fixture missing: ${FIXTURE_DIR}" >&2
  exit 1
fi

MISSING_ARTIFACTS=0
for required in \
  "packages/browser-core/asupersync.js" \
  "packages/browser-core/asupersync_bg.wasm" \
  "packages/browser-core/abi-metadata.json" \
  "packages/browser/dist/index.js" \
  "packages/browser/dist/index.d.ts"
do
  if [[ ! -f "${REPO_ROOT}/${required}" ]]; then
    echo "MISSING: ${required}" >&2
    MISSING_ARTIFACTS=$((MISSING_ARTIFACTS + 1))
  fi
done

if [[ "${MISSING_ARTIFACTS}" -gt 0 ]]; then
  cat >&2 <<'EOF'
FATAL: required packaged Browser Edition artifacts are missing.

Build and stage package artifacts first, then rerun:
  PATH=/usr/bin:$PATH corepack pnpm run build

This worker validation intentionally runs only against built package outputs.
EOF
  exit 1
fi

WORK_DIR="$(mktemp -d "/tmp/asupersync-dedicated-worker.XXXXXX")"
CONSUMER_DIR="${WORK_DIR}/consumer"
PKG_DIR="${WORK_DIR}/packages"

mkdir -p "${CONSUMER_DIR}" "${PKG_DIR}"
cp -R "${FIXTURE_DIR}/." "${CONSUMER_DIR}/"
cp -R "${REPO_ROOT}/packages/browser-core" "${PKG_DIR}/browser-core"
cp -R "${REPO_ROOT}/packages/browser" "${PKG_DIR}/browser"

python3 - "${CONSUMER_DIR}/package.json" "${PKG_DIR}/browser/package.json" <<'PY'
import json
import pathlib
import sys

consumer_pkg = pathlib.Path(sys.argv[1])
browser_pkg = pathlib.Path(sys.argv[2])

consumer_data = json.loads(consumer_pkg.read_text())
consumer_deps = consumer_data.setdefault("dependencies", {})
consumer_deps["@asupersync/browser"] = "file:../packages/browser"
consumer_deps["@asupersync/browser-core"] = "file:../packages/browser-core"
consumer_pkg.write_text(json.dumps(consumer_data, indent=2) + "\n")

browser_data = json.loads(browser_pkg.read_text())
browser_deps = browser_data.setdefault("dependencies", {})
browser_deps["@asupersync/browser-core"] = "file:../browser-core"
browser_pkg.write_text(json.dumps(browser_data, indent=2) + "\n")
PY

(
  cd "${CONSUMER_DIR}"
  PATH="/usr/bin:${PATH}" npm install --no-audit --no-fund
  PATH="/usr/bin:${PATH}" npm run build
  PATH="/usr/bin:${PATH}" npm run check:bundle
) | tee "${LOG_FILE}"

python3 - "${CONSUMER_DIR}" "${SUMMARY_FILE}" "${TIMESTAMP}" <<'PY'
import json
import pathlib
import sys

consumer = pathlib.Path(sys.argv[1])
summary_path = pathlib.Path(sys.argv[2])
timestamp = sys.argv[3]
dist = consumer / "dist"
assets = dist / "assets"
asset_files = list(assets.glob("*.js")) if assets.exists() else []

markers = {
    "worker_bootstrap_marker": False,
    "worker_shutdown_marker": False,
    "worker_storage_support_marker": False,
    "worker_storage_roundtrip_marker": False,
    "storage_artifact_marker": False,
    "download_unsupported_marker": False,
    "worker_artifact_export_marker": False,
    "worker_artifact_download_guard_marker": False,
    "worker_artifact_quota_guard_marker": False,
    "worker_artifact_cleanup_marker": False,
}
for asset in asset_files:
    content = asset.read_text(encoding="utf-8", errors="replace")
    markers["worker_bootstrap_marker"] |= "worker-bootstrap" in content
    markers["worker_shutdown_marker"] |= "worker-shutdown-complete" in content
    markers["worker_storage_support_marker"] |= "worker-storage-support" in content
    markers["worker_storage_roundtrip_marker"] |= "worker-storage-roundtrip" in content
    markers["storage_artifact_marker"] |= "worker-storage-artifact-export-handoff" in content
    markers["download_unsupported_marker"] |= (
        "ASUPERSYNC_BROWSER_ARTIFACT_DOWNLOAD_UNSUPPORTED" in content
    )
    markers["worker_artifact_export_marker"] |= "worker-artifact-archive" in content
    markers["worker_artifact_download_guard_marker"] |= "worker-artifact-download-unavailable" in content
    markers["worker_artifact_quota_guard_marker"] |= "worker-artifact-quota-guard" in content
    markers["worker_artifact_cleanup_marker"] |= "worker-artifact-cleanup" in content

summary = {
    "scenario_id": "L6-BUNDLER-DEDICATED-WORKER",
    "timestamp": timestamp,
    "fixture": "tests/fixtures/dedicated-worker-consumer",
    "status": "pass",
    "checks": {
        "dist_exists": dist.exists(),
        "index_html_exists": (dist / "index.html").exists(),
        "asset_js_count": len(asset_files),
        **markers,
    },
}
summary_path.write_text(json.dumps(summary, indent=2) + "\n")
PY

cat <<EOF
Dedicated worker consumer validation passed.
Artifacts:
  log: ${LOG_FILE}
  summary: ${SUMMARY_FILE}
EOF
