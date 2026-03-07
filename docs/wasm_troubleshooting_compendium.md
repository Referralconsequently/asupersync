# Browser Troubleshooting Compendium and Diagnostics Cookbook (WASM-15)

Contract ID: `wasm-browser-troubleshooting-cookbook-v1`  
Bead: `asupersync-umelq.16.4`  
Depends on: `asupersync-umelq.16.2`, `asupersync-umelq.18.8`

## Purpose

Provide deterministic symptom-to-action playbooks for common Browser Edition
failures so operators can move from incident to replayable evidence without
ad-hoc debugging.

Each recipe includes:

1. symptom pattern,
2. likely root cause,
3. deterministic command bundle,
4. expected evidence artifacts,
5. escalation pointer if the gate remains red.

## Fast Triage Ladder

Run these in order before deep investigation:

```bash
mkdir -p artifacts/troubleshooting

python3 scripts/run_browser_onboarding_checks.py --scenario all \
  | tee artifacts/troubleshooting/onboarding_all.log

bash ./scripts/run_all_e2e.sh --verify-matrix \
  | tee artifacts/troubleshooting/e2e_verify_matrix.log

python3 scripts/check_wasm_dependency_policy.py \
  --policy .github/wasm_dependency_policy.json \
  | tee artifacts/troubleshooting/dependency_policy.log

rch exec -- cargo test --test e2e_log_quality_schema -- --nocapture \
  | tee artifacts/troubleshooting/log_quality_schema.log
```

If all four pass, move to targeted recipes below.

## Recipe Matrix

| Symptom | Likely Cause | Run | Expected Evidence |
|---|---|---|---|
| wasm32 compile fails with forbidden-surface errors | Invalid profile/feature mix (`tls`, `sqlite`, `postgres`, `mysql`, `kafka`, etc.) | `rch exec -- cargo check --target wasm32-unknown-unknown --no-default-features --features wasm-browser-dev` | compile output references wasm guardrails and forbidden feature set; no silent fallback |
| `ASUPERSYNC_*_UNSUPPORTED_RUNTIME` thrown during init/bootstrap | direct runtime attempted in Node, SSR, Next server/edge, or another environment outside the shipped browser support boundary | `rch exec -- cargo test --test wasm_js_exports_coverage_contract -- --nocapture` | contract test output proves package-specific unsupported-runtime codes, support reasons, and guidance strings |
| `run_all_e2e --verify-matrix` fails on redaction policy | Invalid `ARTIFACT_REDACTION_MODE` (`none` under CI) or bad retention value | `CI=1 ARTIFACT_REDACTION_MODE=none bash ./scripts/run_all_e2e.sh --verify-matrix` | failure includes policy text and schema reason; manifest/log paths are still emitted |
| log-quality gate failure | Missing required summary fields or low score under threshold | `rch exec -- cargo test --test e2e_log_quality_schema -- --nocapture` | `e2e_log_quality_schema` tests identify missing/invalid contract tokens with deterministic assertions |
| bundler compatibility lane red | Bundler contract drift, docs/workflow mismatch, or profile closure gap | `rch exec -- cargo test --test wasm_bundler_compatibility -- --nocapture` | pass/fail tied to matrix contract; CI summary artifact path: `artifacts/wasm_bundler_compatibility_summary.json` |
| replay/forensics lane red | Flake governance drift or missing quarantine/forensics metadata | `python3 scripts/check_wasm_flake_governance.py --policy .github/wasm_flake_governance_policy.json` | report + events files: `artifacts/wasm_flake_governance_report.json`, `artifacts/wasm_flake_governance_events.ndjson` |
| obligation/quiescence failures in browser lifecycle tests | Cancel/drain sequencing regression or missing lifecycle cleanup path | `rch exec -- cargo test --test obligation_wasm_parity wasm_full_browser_lifecycle_simulation -- --nocapture` | deterministic failure points to lifecycle phase and obligation invariant breach |

## Deep-Dive Playbooks

### A. Profile and Dependency Closure

Use when wasm32 checks fail or native-only features appear in browser closure.

```bash
python3 scripts/check_wasm_dependency_policy.py \
  --policy .github/wasm_dependency_policy.json

rch exec -- cargo check --target wasm32-unknown-unknown \
  --no-default-features --features wasm-browser-dev

rch exec -- cargo check --target wasm32-unknown-unknown \
  --no-default-features --features wasm-browser-deterministic
```

Evidence to capture:

- policy output with offending dependency path,
- wasm32 check logs for each profile,
- exact feature flags used in failing command.

### B. Replay and Incident Forensics

Use when behavior is flaky across runs or incident triage lacks reproducible logs.

```bash
bash ./scripts/run_all_e2e.sh --suite wasm-incident-forensics \
  | tee artifacts/troubleshooting/incident_forensics.log

python3 scripts/check_wasm_flake_governance.py \
  --policy .github/wasm_flake_governance_policy.json
```

Evidence to capture:

- replay command from suite output,
- quarantine/governance report artifacts,
- trace pointer and scenario id from emitted summary.

### C. Log Contract Violations

Use when diagnostics are present but not machine-parseable or policy-compliant.

```bash
rch exec -- cargo test --test e2e_log_quality_schema -- --nocapture
```

Evidence to capture:

- exact failing test names,
- missing contract token/field from assertion output,
- updated doc/workflow references if contract drift is intentional.

### D. Unsupported Runtime and Compatibility Boundary Failures

Use when `@asupersync/browser`, `@asupersync/react`, or `@asupersync/next`
throws an unsupported-runtime error during bootstrap.

```bash
rch exec -- cargo test --test wasm_js_exports_coverage_contract -- --nocapture
```

Evidence to capture:

- package-specific error code:
  - `ASUPERSYNC_BROWSER_UNSUPPORTED_RUNTIME`
  - `ASUPERSYNC_REACT_UNSUPPORTED_RUNTIME`
  - `ASUPERSYNC_NEXT_UNSUPPORTED_RUNTIME`
- `diagnostics.reason` (`missing_global_this`, `missing_browser_dom`,
  `missing_webassembly`, or `supported`)
- capability snapshot (`hasWindow`, `hasDocument`, `hasWebAssembly`,
  `hasAbortController`, `hasFetch`, `hasWebSocket`)
- Next target (`client`, `server`, `edge`) if the failure came through
  `@asupersync/next`

Expected operator action:

- keep `@asupersync/browser` direct runtime creation in a real browser
  main-thread entrypoint
- keep `@asupersync/react` direct runtime usage inside client-rendered React
  trees only
- keep `@asupersync/next` server and edge code on bridge-only adapters and move
  runtime creation into a client component or browser-only module
- do not treat browser-worker, Node.js, or SSR contexts as implicitly supported
  direct-runtime lanes unless the support matrix and package guards are promoted
  together

## Escalation Rules

Escalate immediately if any condition holds:

1. a failure is non-reproducible under fixed command/seed,
2. evidence artifacts are missing or non-parseable,
3. a workaround requires disabling redaction or quality gates.

Escalation route:

1. Post findings in Agent Mail with thread id matching active bead.
2. Include command, exact failure, and artifact pointers.
3. Keep mitigation proposals explicit (no hidden policy bypasses).

## Cross-References

- `docs/integration.md` (Browser Documentation IA + guardrails)
- `docs/wasm_dx_error_taxonomy.md` (package error codes, recoverability, and guidance contract)
- `docs/wasm_quickstart_migration.md` (onboarding/release-channel flow)
- `docs/wasm_bundler_compatibility_matrix.md` (bundler contract and CI lane)
- `docs/wasm_flake_governance_and_forensics.md` (incident governance)
- `docs/doctor_logging_contract.md` (redaction and log-quality contracts)
