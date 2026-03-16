import fs from "node:fs";
import path from "node:path";

const distDir = path.resolve("dist");
const indexPath = path.join(distDir, "index.html");
const assetDir = path.join(distDir, "assets");

if (!fs.existsSync(distDir)) {
  throw new Error(`Missing dist directory: ${distDir}`);
}

if (!fs.existsSync(indexPath)) {
  throw new Error(`Missing built index.html: ${indexPath}`);
}

if (!fs.existsSync(assetDir)) {
  throw new Error(`Missing assets directory: ${assetDir}`);
}

const jsAssets = fs
  .readdirSync(assetDir)
  .filter((name) => name.endsWith(".js") || name.endsWith(".mjs"));
if (jsAssets.length < 2) {
  throw new Error(
    `Expected at least two JS assets in dist/assets for main-thread + worker bundles, got ${jsAssets.length}`,
  );
}

let sawBootstrapMarker = false;
let sawShutdownMarker = false;
let sawStorageSupportMarker = false;
let sawStorageRoundtripMarker = false;
let sawStorageArtifactMarker = false;
let sawDownloadUnsupportedMarker = false;
let sawArtifactExportMarker = false;
let sawArtifactDownloadGuardMarker = false;
let sawArtifactQuotaGuardMarker = false;
let sawArtifactCleanupMarker = false;
for (const assetName of jsAssets) {
  const content = fs.readFileSync(path.join(assetDir, assetName), "utf8");
  sawBootstrapMarker ||= content.includes("worker-bootstrap");
  sawShutdownMarker ||= content.includes("worker-shutdown-complete");
  sawStorageSupportMarker ||= content.includes("worker-storage-support");
  sawStorageRoundtripMarker ||= content.includes("worker-storage-roundtrip");
  sawStorageArtifactMarker ||= content.includes("worker-storage-artifact-export-handoff");
  sawDownloadUnsupportedMarker ||= content.includes(
    "ASUPERSYNC_BROWSER_ARTIFACT_DOWNLOAD_UNSUPPORTED",
  );
  sawArtifactExportMarker ||= content.includes("worker-artifact-archive");
  sawArtifactDownloadGuardMarker ||= content.includes("worker-artifact-download-unavailable");
  sawArtifactQuotaGuardMarker ||= content.includes("worker-artifact-quota-guard");
  sawArtifactCleanupMarker ||= content.includes("worker-artifact-cleanup");
}

if (!sawBootstrapMarker) {
  throw new Error("Built worker bundle must retain the worker-bootstrap message marker");
}

if (!sawShutdownMarker) {
  throw new Error(
    "Built worker bundle must retain the worker-shutdown-complete message marker",
  );
}

if (!sawStorageArtifactMarker) {
  throw new Error("Built worker bundle must retain the storage/artifact exercise marker");
}

if (!sawDownloadUnsupportedMarker) {
  throw new Error("Built worker bundle must retain the worker download failure code marker");
}

if (!sawStorageSupportMarker) {
  throw new Error("Built worker bundle must retain the worker-storage-support marker");
}

if (!sawStorageRoundtripMarker) {
  throw new Error("Built worker bundle must retain the worker-storage-roundtrip marker");
}

if (!sawArtifactExportMarker) {
  throw new Error("Built worker bundle must retain the worker-artifact-archive marker");
}

if (!sawArtifactDownloadGuardMarker) {
  throw new Error(
    "Built worker bundle must retain the worker-artifact-download-unavailable marker",
  );
}

if (!sawArtifactQuotaGuardMarker) {
  throw new Error("Built worker bundle must retain the worker-artifact-quota-guard marker");
}

if (!sawArtifactCleanupMarker) {
  throw new Error("Built worker bundle must retain the worker-artifact-cleanup marker");
}

console.log(
  JSON.stringify(
    {
      status: "ok",
      jsAssetCount: jsAssets.length,
      sawBootstrapMarker,
      sawShutdownMarker,
      sawStorageSupportMarker,
      sawStorageRoundtripMarker,
      sawStorageArtifactMarker,
      sawDownloadUnsupportedMarker,
      sawArtifactExportMarker,
      sawArtifactDownloadGuardMarker,
      sawArtifactQuotaGuardMarker,
      sawArtifactCleanupMarker,
    },
    null,
    2,
  ),
);
