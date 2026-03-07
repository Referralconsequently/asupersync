#!/usr/bin/env bash
# WASM QA evidence matrix smoke runner (WASM-QA 3qv04.8.1)
#
# Usage:
#   bash ./scripts/run_wasm_qa_evidence_smoke.sh --list
#   bash ./scripts/run_wasm_qa_evidence_smoke.sh --scenario WASM-QA-SMOKE-LAYERS --dry-run
#   bash ./scripts/run_wasm_qa_evidence_smoke.sh --scenario WASM-QA-SMOKE-LAYERS --execute
#
# Bundle schema: wasm-qa-evidence-smoke-bundle-v1
# Report schema: wasm-qa-evidence-smoke-run-report-v1

set -euo pipefail

ARTIFACT="artifacts/wasm_qa_evidence_matrix_v1.json"
RCH_BIN="${RCH_BIN:-rch}"
MODE=""
SCENARIO=""
WASM_PROFILE="${WASM_PROFILE:-wasm-browser-dev}"
BROWSER_ID="${BROWSER_ID:-headless-smoke}"
PACKAGE_NAME="${PACKAGE_NAME:-@asupersync/browser-core}"
MODULE_FINGERPRINT="${WASM_MODULE_FINGERPRINT:-unknown}"
EVIDENCE_ID="${EVIDENCE_ID:-L8-REPRO-COMMAND}"
LAYER_ID="${LAYER_ID:-L8}"
TOOL_NAME="${TOOL_NAME:-smoke-runner}"

usage() {
  echo "Usage: $0 --list | --scenario <ID> (--dry-run | --execute)"
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --list)   MODE="list"; shift ;;
    --scenario) SCENARIO="$2"; shift 2 ;;
    --dry-run)  MODE="dry-run"; shift ;;
    --execute)  MODE="execute"; shift ;;
    *) usage ;;
  esac
done

[[ -z "$MODE" ]] && usage

BUNDLE_SCHEMA=$(jq -r '.runner_bundle_schema_version // "wasm-qa-evidence-smoke-bundle-v1"' "$ARTIFACT")
REPORT_SCHEMA=$(jq -r '.runner_report_schema_version // "wasm-qa-evidence-smoke-run-report-v1"' "$ARTIFACT")
ARTIFACT_BUNDLE_SCHEMA=$(jq -r '.artifact_bundle_schema_version // "wasm-qa-artifact-bundle-v1"' "$ARTIFACT")
LOG_SCHEMA=$(jq -r '.e2e_log_schema_version // "wasm-qa-e2e-log-v1"' "$ARTIFACT")
RETENTION_SCHEMA=$(jq -r '.retention_policy.schema_version // "wasm-qa-artifact-retention-v1"' "$ARTIFACT")

retention_days_for_class() {
  local cls="$1"
  local days
  days=$(jq -r --arg cls "$cls" '.retention_policy.classes[]? | select(.class == $cls) | .min_days' "$ARTIFACT" | head -n1)
  if [[ -z "${days}" || "${days}" == "null" ]]; then
    case "$cls" in
      hot) days=30 ;;
      warm) days=14 ;;
      cold) days=7 ;;
      *) days=7 ;;
    esac
  fi
  printf '%s' "$days"
}

retention_until_utc() {
  local cls="$1"
  local days
  days=$(retention_days_for_class "$cls")
  date -u -d "+${days} days" +%Y-%m-%dT%H:%M:%SZ
}

if [[ "$MODE" == "list" ]]; then
  echo "=== WASM QA Evidence Matrix Smoke Scenarios ==="
  jq -r '.smoke_scenarios[] | "  \(.scenario_id): \(.description)"' "$ARTIFACT"
  exit 0
fi

[[ -z "$SCENARIO" ]] && { echo "error: --scenario required with --dry-run/--execute"; exit 1; }

COMMAND=$(jq -r --arg sid "$SCENARIO" '.smoke_scenarios[] | select(.scenario_id == $sid) | .command' "$ARTIFACT")
DESCRIPTION=$(jq -r --arg sid "$SCENARIO" '.smoke_scenarios[] | select(.scenario_id == $sid) | .description' "$ARTIFACT")

if [[ -z "$COMMAND" || "$COMMAND" == "null" ]]; then
  echo "error: unknown scenario $SCENARIO"
  exit 1
fi

RUN_ID="run_$(date +%Y%m%d_%H%M%S)"
OUTDIR="target/wasm-qa-evidence-smoke/$RUN_ID/$SCENARIO"
mkdir -p "$OUTDIR"
BUNDLE_MANIFEST_PATH="$OUTDIR/bundle_manifest.json"
RUN_REPORT_PATH="$OUTDIR/run_report.json"
RUN_LOG_PATH="$OUTDIR/run.log"
EVENTS_PATH="$OUTDIR/events.ndjson"
touch "$EVENTS_PATH"

emit_event() {
  local event_kind="$1"
  local verdict="$2"
  local exit_code="$3"
  local failure_reason="$4"
  local retention_class="$5"
  local retention_until="$6"
  jq -nc \
    --arg schema_version "$LOG_SCHEMA" \
    --arg event_kind "$event_kind" \
    --arg scenario_id "$SCENARIO" \
    --arg run_id "$RUN_ID" \
    --arg timestamp_utc "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg evidence_id "$EVIDENCE_ID" \
    --arg layer "$LAYER_ID" \
    --arg tool "$TOOL_NAME" \
    --arg wasm_profile "$WASM_PROFILE" \
    --arg browser "$BROWSER_ID" \
    --arg package_name "$PACKAGE_NAME" \
    --arg module_fingerprint "$MODULE_FINGERPRINT" \
    --arg verdict "$verdict" \
    --argjson command_exit_code "$exit_code" \
    --arg failure_reason "$failure_reason" \
    --arg repro_command "$COMMAND" \
    --arg bundle_manifest_path "$BUNDLE_MANIFEST_PATH" \
    --arg artifact_path "$OUTDIR" \
    --arg retention_class "$retention_class" \
    --arg retention_until_utc "$retention_until" \
    '{
      schema_version,
      event_kind,
      scenario_id,
      run_id,
      timestamp_utc,
      evidence_id,
      layer,
      tool,
      wasm_profile,
      browser,
      package_name,
      module_fingerprint,
      verdict,
      command_exit_code,
      failure_reason,
      repro_command,
      bundle_manifest_path,
      artifact_path,
      retention_class,
      retention_until_utc
    }' >> "$EVENTS_PATH"
}

write_bundle_manifest() {
  local retention_class="$1"
  local retention_until="$2"
  jq -nc \
    --arg schema "$BUNDLE_SCHEMA" \
    --arg artifact_bundle_schema_version "$ARTIFACT_BUNDLE_SCHEMA" \
    --arg log_schema_version "$LOG_SCHEMA" \
    --arg retention_schema_version "$RETENTION_SCHEMA" \
    --arg scenario_id "$SCENARIO" \
    --arg description "$DESCRIPTION" \
    --arg run_id "$RUN_ID" \
    --arg mode "$MODE" \
    --arg command "$COMMAND" \
    --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg bundle_manifest_path "$BUNDLE_MANIFEST_PATH" \
    --arg run_report_path "$RUN_REPORT_PATH" \
    --arg run_log_path "$RUN_LOG_PATH" \
    --arg events_path "$EVENTS_PATH" \
    --arg retention_class "$retention_class" \
    --arg retention_until_utc "$retention_until" \
    '{
      schema,
      artifact_bundle_schema_version,
      log_schema_version,
      retention_schema_version,
      scenario_id,
      description,
      run_id,
      mode,
      command,
      timestamp,
      bundle_manifest_path,
      run_report_path,
      run_log_path,
      events_path,
      retention_class,
      retention_until_utc,
      required_layout: ["bundle_manifest.json", "run_report.json", "run.log", "events.ndjson"]
    }' > "$BUNDLE_MANIFEST_PATH"
}

write_run_report() {
  local exit_code="$1"
  local verdict="$2"
  local failure_reason="$3"
  local retention_class="$4"
  local retention_until="$5"
  jq -nc \
    --arg schema "$REPORT_SCHEMA" \
    --arg scenario_id "$SCENARIO" \
    --arg run_id "$RUN_ID" \
    --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg verdict "$verdict" \
    --arg failure_reason "$failure_reason" \
    --arg artifact_path "$OUTDIR" \
    --arg events_path "$EVENTS_PATH" \
    --arg retention_class "$retention_class" \
    --arg retention_until_utc "$retention_until" \
    --argjson exit_code "$exit_code" \
    '{
      schema,
      scenario_id,
      run_id,
      exit_code,
      verdict,
      failure_reason,
      artifact_path,
      events_path,
      retention_class,
      retention_until_utc,
      timestamp
    }' > "$RUN_REPORT_PATH"
}

if [[ "$MODE" == "dry-run" ]]; then
  RETENTION_CLASS="cold"
  RETENTION_UNTIL="$(retention_until_utc "$RETENTION_CLASS")"
  : > "$RUN_LOG_PATH"
  write_bundle_manifest "$RETENTION_CLASS" "$RETENTION_UNTIL"
  write_run_report 0 "skip" "dry-run mode" "$RETENTION_CLASS" "$RETENTION_UNTIL"
  emit_event "dry_run" "skip" 0 "dry-run mode" "$RETENTION_CLASS" "$RETENTION_UNTIL"
  echo "[dry-run] $SCENARIO: $DESCRIPTION"
  echo "[dry-run] command: $COMMAND"
  echo "[dry-run] bundle: $BUNDLE_MANIFEST_PATH"
  echo "[dry-run] report: $RUN_REPORT_PATH"
  exit 0
fi

echo "=== Executing $SCENARIO ==="
echo "  $DESCRIPTION"
echo "  command: $COMMAND"

emit_event "scenario_start" "blocked" -1 "" "warm" "$(retention_until_utc warm)"
EXITCODE=0
eval "$COMMAND" > "$RUN_LOG_PATH" 2>&1 || EXITCODE=$?

if [[ $EXITCODE -eq 0 ]]; then
  VERDICT="pass"
  FAILURE_REASON=""
  RETENTION_CLASS="warm"
else
  VERDICT="fail"
  FAILURE_REASON="command exited with status $EXITCODE"
  RETENTION_CLASS="hot"
fi
RETENTION_UNTIL="$(retention_until_utc "$RETENTION_CLASS")"

write_bundle_manifest "$RETENTION_CLASS" "$RETENTION_UNTIL"
write_run_report "$EXITCODE" "$VERDICT" "$FAILURE_REASON" "$RETENTION_CLASS" "$RETENTION_UNTIL"
emit_event "scenario_finish" "$VERDICT" "$EXITCODE" "$FAILURE_REASON" "$RETENTION_CLASS" "$RETENTION_UNTIL"

if [[ $EXITCODE -eq 0 ]]; then
  echo "  PASS (exit 0)"
else
  echo "  FAIL (exit $EXITCODE)"
  tail -20 "$RUN_LOG_PATH"
fi

exit $EXITCODE
