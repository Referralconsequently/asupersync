# Dedicated Worker Consumer Fixture

Fixture beads:
- `asupersync-18tbo.4` for the maintained dedicated-worker example and onboarding lane

Purpose:
- validate a real dedicated-worker consumer build against packaged Browser Edition outputs
- demonstrate the supported direct-runtime worker bootstrap path for `@asupersync/browser`
- make worker startup, message coordination, and shutdown explicit in maintained example code
- exercise a worker-safe IndexedDB round-trip plus explicit `BrowserArtifactStore`
  export, cleanup, quota-guard, and download-fallback behavior

This fixture is executed through:
- `scripts/validate_dedicated_worker_consumer.sh`

The validation script copies this fixture into a temporary workspace and installs
local package copies to keep runs deterministic and side-effect free.

## What This Example Shows

- `src/main.ts`
  main-thread bootstrap that spawns a dedicated worker, records the worker
  support snapshot, and requests graceful shutdown after the worker reports
  readiness
- `src/worker.ts`
  dedicated-worker bootstrap that detects direct-runtime support, initializes a
  Browser Edition runtime, enters a scope, performs a `BrowserStorage`
  round-trip, persists/export-clears evidence through `BrowserArtifactStore`,
  proves `downloadArchive()` fails closed in workers, and reports shutdown
  completion back to the main thread
- `scripts/check-bundle.mjs`
  verifies the bundled app still carries the durable-storage and artifact-export
  markers (`worker-storage-roundtrip`, `worker-artifact-archive`,
  `worker-artifact-download-unavailable`, `worker-artifact-quota-guard`,
  `worker-artifact-cleanup`)

## Deterministic Validation

Run the maintained example through the canonical validation path:

```bash
PATH=/usr/bin:$PATH bash scripts/validate_dedicated_worker_consumer.sh
```

The validation artifacts are emitted under:

```text
target/e2e-results/dedicated_worker_consumer/
```
