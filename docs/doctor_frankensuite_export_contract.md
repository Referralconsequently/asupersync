# doctor_asupersync FrankenSuite Export Contract

This document defines the deterministic export contract for bead
`asupersync-2b4jj.5.3`.

## Command

```bash
asupersync --format json doctor franken-export \
  --fixture-id baseline_failure_path \
  --out-dir target/e2e-results/doctor_frankensuite_export/artifacts
```

Optional report-file mode:

```bash
asupersync --format json doctor franken-export \
  --report path/to/core_report.json \
  --out-dir target/e2e-results/doctor_frankensuite_export/artifacts
```

## Output Schema (`doctor-frankensuite-export-v1`)

Required top-level fields:

- `schema_version` (`doctor-frankensuite-export-v1`)
- `source_schema_version` (`doctor-core-report-v1`)
- `export_root`
- `exports` (non-empty array)
- `rerun_commands` (array, at least 2 commands)

Required `exports[*]` fields:

- `fixture_id`
- `report_id`
- `trace_id`
- `evidence_jsonl`
- `decision_json`
- `evidence_count`
- `decision_count`
- `validation_status` (`valid`)

## Artifact Semantics

- `evidence_jsonl`: newline-delimited serialized `franken_evidence::EvidenceLedger`.
- `decision_json`: deterministic JSON array of `franken_decision::DecisionAuditEntry`.
- Export order is stable:
  - evidence records sorted by `evidence_id`
  - decision records sorted by `finding_id`
- Identifier derivation uses stable hashing (`stable_u128`) for deterministic
  `TraceId` and `DecisionId` synthesis.

## Validation and Failure Behavior

- Input core reports are validated with `validate_core_diagnostics_report(...)`.
- Unsupported report schema versions fail closed.
- Malformed JSON report payloads fail closed.
- Artifact write failures fail closed with path-attributed CLI errors.

## E2E Coverage

Deterministic end-to-end validation:

```bash
bash scripts/test_doctor_frankensuite_export_e2e.sh
```

This suite verifies:

- repeat-run deterministic command output
- artifact count/path integrity
- parseable evidence/decision artifact payloads
- repeat-run deterministic artifact file contents
