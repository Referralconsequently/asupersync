# WASM TypeScript Package Topology Contract

Contract ID: `wasm-typescript-package-topology-v1`  
Bead: `asupersync-umelq.9.1`

## Purpose

Define deterministic TypeScript package boundaries for Browser Edition so users
can adopt a layered API without hidden semantic drift.

## Canonical Inputs

- Policy: `.github/wasm_typescript_package_policy.json`
- Gate script: `scripts/check_wasm_typescript_package_policy.py`
- Onboarding runner: `scripts/run_browser_onboarding_checks.py`

## Package Topology

Required package set:

1. `@asupersync/browser-core`
2. `@asupersync/browser`
3. `@asupersync/react`
4. `@asupersync/next`

Layer contract:

1. `@asupersync/browser-core` owns low-level runtime and type surface contracts.
2. `@asupersync/browser` owns high-level SDK semantics and diagnostics surface.
3. `@asupersync/react` and `@asupersync/next` are adapter layers over
   `@asupersync/browser`.
4. Public exports must be tree-shake safe and must not expose
   `./internal/*` or `./native/*` subpaths.

## Type Surface Ownership

Required symbols and package owners:

1. `Outcome` -> `@asupersync/browser-core`
2. `Budget` -> `@asupersync/browser-core`
3. `CancellationToken` -> `@asupersync/browser`
4. `RegionHandle` -> `@asupersync/browser`

Any symbol owner outside the declared package topology fails policy.

## Resolution Matrix and E2E Command Contract

The policy encodes deterministic install-and-run command pairs for:

1. Vanilla TypeScript (ESM + CJS)
2. React (ESM + CJS)
3. Next.js (ESM + CJS)

Each scenario defines:

1. `entrypoint`
2. `module_mode`
3. `bundler`
4. `adapter_path`
5. `runtime_profile`
6. `install_command`
7. `run_command`

Coverage gates:

1. Required frameworks: `vanilla-ts`, `react`, `next`
2. Required module modes: `esm`, `cjs`
3. Required bundlers: `vite`, `webpack`, `next-turbopack`

## Structured Logging Contract

Onboarding and policy logs must include:

1. `scenario_id`
2. `step_id`
3. `package_entrypoint`
4. `adapter_path`
5. `runtime_profile`
6. `diagnostic_category`
7. `outcome`
8. `artifact_log_path`
9. `repro_command`

`run_browser_onboarding_checks.py` emits these fields per step so onboarding
failures are diagnosable by package boundary and adapter lane.

## Gate Outputs

- Summary JSON: `artifacts/wasm_typescript_package_summary.json`
- NDJSON log: `artifacts/wasm_typescript_package_log.ndjson`

## Repro Commands

Self-test:

```bash
python3 scripts/check_wasm_typescript_package_policy.py --self-test
```

Full policy gate:

```bash
python3 scripts/check_wasm_typescript_package_policy.py \
  --policy .github/wasm_typescript_package_policy.json
```

Framework-scoped checks (used by onboarding runner):

```bash
python3 scripts/check_wasm_typescript_package_policy.py \
  --policy .github/wasm_typescript_package_policy.json \
  --only-scenario TS-PKG-VANILLA-ESM \
  --only-scenario TS-PKG-VANILLA-CJS

python3 scripts/check_wasm_typescript_package_policy.py \
  --policy .github/wasm_typescript_package_policy.json \
  --only-scenario TS-PKG-REACT-ESM \
  --only-scenario TS-PKG-REACT-CJS

python3 scripts/check_wasm_typescript_package_policy.py \
  --policy .github/wasm_typescript_package_policy.json \
  --only-scenario TS-PKG-NEXT-ESM \
  --only-scenario TS-PKG-NEXT-CJS
```
