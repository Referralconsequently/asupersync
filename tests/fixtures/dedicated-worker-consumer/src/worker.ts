/// <reference lib="webworker" />

import {
  abiFingerprint,
  abiVersion,
  BROWSER_ARTIFACT_DOWNLOAD_UNSUPPORTED_CODE,
  createBrowserArtifactStore,
  createBrowserRuntime,
  createBrowserStorage,
  detectBrowserRuntimeSupport,
  detectBrowserStorageSupport,
  formatOutcomeFailure,
  type BrowserRuntime,
} from "@asupersync/browser";

declare const self: DedicatedWorkerGlobalScope;

type ShutdownRequest = {
  type: "shutdown";
  reason?: string;
};

type RuntimeOutcome = Awaited<ReturnType<typeof createBrowserRuntime>>;
type ScopeOutcome = ReturnType<BrowserRuntime["enterScope"]>;

const WORKER_STORAGE_NAMESPACE = "worker_fixture_storage";
const WORKER_ARTIFACT_NAMESPACE = "worker_fixture_artifacts";
const WORKER_ARTIFACT_QUOTA_NAMESPACE = "worker_fixture_artifacts_quota";
const WORKER_STORAGE_SUPPORT_MARKER = "worker-storage-support";
const WORKER_STORAGE_ROUNDTRIP_MARKER = "worker-storage-roundtrip";
const WORKER_STORAGE_ARTIFACT_MARKER = "worker-storage-artifact-export-handoff";
const WORKER_ARTIFACT_EXPORT_MARKER = "worker-artifact-archive";
const WORKER_ARTIFACT_DOWNLOAD_GUARD_MARKER = "worker-artifact-download-unavailable";
const WORKER_ARTIFACT_QUOTA_GUARD_MARKER = "worker-artifact-quota-guard";
const WORKER_ARTIFACT_CLEANUP_MARKER = "worker-artifact-cleanup";

let runtimeHandle: RuntimeOutcome | null = null;
let scopeHandle: ScopeOutcome | null = null;

function errorCode(error: unknown): string | null {
  if (!error || typeof error !== "object" || !("code" in error)) {
    return null;
  }
  const value = (error as { code?: unknown }).code;
  return typeof value === "string" ? value : null;
}

function errorReason(error: unknown): string | null {
  if (!error || typeof error !== "object" || !("diagnostics" in error)) {
    return null;
  }
  const diagnostics = (
    error as {
      diagnostics?: {
        reason?: unknown;
      };
    }
  ).diagnostics;
  return typeof diagnostics?.reason === "string" ? diagnostics.reason : null;
}

async function bootstrap(): Promise<void> {
  const support = detectBrowserRuntimeSupport(self as unknown as Record<string, unknown>);
  const storageSupport = detectBrowserStorageSupport(
    "indexeddb",
    self as unknown as Record<string, unknown>,
  );
  const runtime = await createBrowserRuntime();
  runtimeHandle = runtime;

  let scopeOutcome: ScopeOutcome | null = null;
  if (runtime.outcome === "ok") {
    scopeOutcome = runtime.value.enterScope("dedicated-worker-fixture");
    scopeHandle = scopeOutcome;
  }

  let storageExercise: Record<string, unknown> | null = null;
  let artifactExercise: Record<string, unknown> | null = null;
  if (storageSupport.supported) {
    const storage = createBrowserStorage({
      backend: "indexeddb",
      dbName: "asupersync-fixture",
      storeName: "browser-fixture",
      globalObject: self as unknown as Record<string, unknown>,
    });
    const artifactStore = createBrowserArtifactStore({
      backend: "indexeddb",
      namespace: WORKER_ARTIFACT_NAMESPACE,
      globalObject: self as unknown as Record<string, unknown>,
      retention: {
        maxArtifacts: 4,
        maxArtifactBytes: 16 * 1024,
        maxTotalBytes: 64 * 1024,
        quotaStrategy: "evict_oldest",
      },
    });
    const quotaStore = createBrowserArtifactStore({
      backend: "indexeddb",
      namespace: WORKER_ARTIFACT_QUOTA_NAMESPACE,
      globalObject: self as unknown as Record<string, unknown>,
      retention: {
        maxArtifacts: 1,
        maxArtifactBytes: 1024,
        maxTotalBytes: 1024,
        quotaStrategy: "fail",
      },
    });

    await storage.clearNamespace(WORKER_STORAGE_NAMESPACE);
    await artifactStore.clearArtifacts();
    await quotaStore.clearArtifacts();

    await storage.set(WORKER_STORAGE_NAMESPACE, "ready", "worker-storage-ready");
    const storedValue = await storage.get(WORKER_STORAGE_NAMESPACE, "ready");
    const listedKeys = await storage.listKeys(WORKER_STORAGE_NAMESPACE);
    if (storedValue === null) {
      throw new Error("expected dedicated-worker storage round-trip payload to be readable");
    }
    if (!listedKeys.includes("ready")) {
      throw new Error("expected dedicated-worker storage namespace to retain the ready key");
    }

    const persisted = await artifactStore.persistEvidenceArtifact(
      {
        marker: WORKER_STORAGE_ARTIFACT_MARKER,
        lane: "worker",
        runtimeOutcome: runtime.outcome,
      },
      {
        id: "worker-evidence",
        tags: ["fixture", "worker", "storage", "artifacts"],
      },
    );
    const archive = await artifactStore.exportArchive();

    let downloadFailureCode: string | null = null;
    try {
      await artifactStore.downloadArchive();
    } catch (error) {
      downloadFailureCode = errorCode(error);
    }
    if (downloadFailureCode !== BROWSER_ARTIFACT_DOWNLOAD_UNSUPPORTED_CODE) {
      throw new Error(
        `expected ${BROWSER_ARTIFACT_DOWNLOAD_UNSUPPORTED_CODE} from dedicated-worker downloadArchive()`,
      );
    }

    await quotaStore.persistEvidenceArtifact("q".repeat(600), {
      id: "worker-quota-a",
      format: "text",
    });
    let quotaFailureReason: string | null = null;
    try {
      await quotaStore.persistEvidenceArtifact("q".repeat(600), {
        id: "worker-quota-b",
        format: "text",
      });
    } catch (error) {
      quotaFailureReason = errorReason(error);
    }
    if (quotaFailureReason !== "quota_exceeded") {
      throw new Error("expected quota_exceeded from the dedicated-worker quota guard");
    }

    const clearedArtifacts = await artifactStore.clearArtifacts();
    const clearedQuotaArtifacts = await quotaStore.clearArtifacts();
    const clearedKeys = await storage.clearNamespace(WORKER_STORAGE_NAMESPACE);
    if (clearedArtifacts < 1) {
      throw new Error("expected at least one dedicated-worker artifact to be cleared");
    }
    if (clearedQuotaArtifacts < 1) {
      throw new Error("expected at least one dedicated-worker quota artifact to be cleared");
    }

    storageExercise = {
      supportMarker: WORKER_STORAGE_SUPPORT_MARKER,
      roundtripMarker: WORKER_STORAGE_ROUNDTRIP_MARKER,
      artifactMarker: WORKER_STORAGE_ARTIFACT_MARKER,
      backend: storage.backend,
      dbName: storage.dbName,
      storeName: storage.storeName,
      support: storageSupport,
      listedKeys,
      storedValueLength: storedValue?.byteLength ?? null,
      clearedKeys,
    };
    artifactExercise = {
      marker: WORKER_STORAGE_ARTIFACT_MARKER,
      exportMarker: WORKER_ARTIFACT_EXPORT_MARKER,
      downloadGuardMarker: WORKER_ARTIFACT_DOWNLOAD_GUARD_MARKER,
      quotaGuardMarker: WORKER_ARTIFACT_QUOTA_GUARD_MARKER,
      cleanupMarker: WORKER_ARTIFACT_CLEANUP_MARKER,
      namespace: artifactStore.namespace,
      retention: artifactStore.retentionPolicy(),
      persistedArtifactId: persisted.artifact.id,
      exportedArtifactCount: archive.archive.artifacts.length,
      archiveFilename: archive.filename,
      downloadFailureCode,
      quotaFailureReason,
      clearedArtifacts,
      clearedQuotaArtifacts,
    };
  }

  self.postMessage({
    type: "worker-bootstrap",
    payload: {
      support,
      storageSupport,
      abiVersion: abiVersion(),
      abiFingerprint: abiFingerprint(),
      runtimeOutcome: runtime.outcome,
      scopeOutcome: scopeOutcome?.outcome ?? null,
      storageExercise,
      artifactExercise,
    },
  });
}

async function shutdown(reason: string | null): Promise<void> {
  if (scopeHandle?.outcome === "ok") {
    scopeHandle.value.close();
  }
  if (runtimeHandle?.outcome === "ok") {
    runtimeHandle.value.close();
  }

  self.postMessage({
    type: "worker-shutdown-complete",
    reason,
  });
  self.close();
}

self.addEventListener("message", (event: MessageEvent<ShutdownRequest>) => {
  if (event.data?.type === "shutdown") {
    void shutdown(event.data.reason ?? null);
  }
});

void bootstrap().catch((error) => {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : formatOutcomeFailure({
            outcome: "err",
            failure: {
              code: "worker_bootstrap_failed",
              recoverability: "transient",
              message: "dedicated worker bootstrap failed",
            },
          });

  self.postMessage({
    type: "worker-bootstrap-failed",
    message,
  });
});
