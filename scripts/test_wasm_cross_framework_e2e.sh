#!/usr/bin/env bash
# Cross-framework browser E2E runner (asupersync-umelq.18.3)
#
# Validates browser-oriented end-to-end behavior across vanilla, React, and
# Next.js reference surfaces with deterministic, artifactized step logs.
#
# Coverage intent:
# - initialization and orchestration checks
# - cancellation/loser-drain behavior
# - negative-path assertions
# - hostile timing/tab-suspension style stress
# - recovery + replay artifact coverage

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="${PROJECT_ROOT}/target/e2e-results/wasm_cross_framework"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
RUN_STARTED_TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
ARTIFACT_DIR="${OUTPUT_DIR}/artifacts_${TIMESTAMP}"
SUMMARY_FILE="${ARTIFACT_DIR}/summary.json"
STEP_NDJSON="${ARTIFACT_DIR}/steps.ndjson"
SUITE_ID="wasm_cross_framework_e2e"
SCENARIO_ID="E2E-SUITE-WASM-CROSS-FRAMEWORK"
SUITE_TIMEOUT="${SUITE_TIMEOUT:-1200}"
STEP_TIMEOUT="${STEP_TIMEOUT:-360}"
RCH_SCAN_TIMEOUT="${RCH_SCAN_TIMEOUT:-420}"
RCH_BIN="${RCH_BIN:-$HOME/.local/bin/rch}"

export TEST_LOG_LEVEL="${TEST_LOG_LEVEL:-info}"
export RUST_LOG="${RUST_LOG:-asupersync=info}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"
export TEST_SEED="${TEST_SEED:-0xDEADBEEF}"
FAULT_MATRIX_MODE="${FAULT_MATRIX_MODE:-reduced}"

case "${FAULT_MATRIX_MODE}" in
    reduced|full)
        ;;
    *)
        echo "FATAL: FAULT_MATRIX_MODE must be one of: reduced, full" >&2
        exit 1
        ;;
esac

if [[ ! -x "${RCH_BIN}" ]]; then
    echo "FATAL: rch is required and was not found/executable at: ${RCH_BIN}" >&2
    exit 1
fi

mkdir -p "${OUTPUT_DIR}" "${ARTIFACT_DIR}"

echo "==================================================================="
echo "         Asupersync WASM Cross-Framework Browser E2E               "
echo "==================================================================="
echo "Config:"
echo "  RCH_BIN:          ${RCH_BIN}"
echo "  TEST_LOG_LEVEL:   ${TEST_LOG_LEVEL}"
echo "  RUST_LOG:         ${RUST_LOG}"
echo "  TEST_SEED:        ${TEST_SEED}"
echo "  FAULT_MATRIX_MODE:${FAULT_MATRIX_MODE}"
echo "  SUITE_TIMEOUT:    ${SUITE_TIMEOUT}s"
echo "  STEP_TIMEOUT:     ${STEP_TIMEOUT}s"
echo "  Artifacts:        ${ARTIFACT_DIR}"
echo ""

STEP_IDS=(
    "vanilla.scheduler_ready_handoff_limit"
    "vanilla.cancel_preempts_ready_burst"
    "react.strict_mode_double_invocation"
    "react.concurrent_restart_loser_drain"
    "next.bootstrap_ssr_to_hydration"
    "next.negative_cache_revalidation_rejected"
    "next.recovery_cancelled_bootstrap_retry"
    "next.recovery_hydration_mismatch_rehydrate"
    "wasm.host_interruption_tab_suspension"
    "wasm.host_interruption_cancel_drain"
    "vanilla.browser_replay_report_artifact"
    "vanilla.browser_replay_schedule_fuzz_corpus"
    "vanilla.browser_replay_delta_drift_bundle"
)

STEP_FRAMEWORK=(
    "vanilla"
    "vanilla"
    "react"
    "react"
    "next"
    "next"
    "next"
    "next"
    "vanilla"
    "vanilla"
    "vanilla"
    "vanilla"
    "vanilla"
)

STEP_CATEGORY=(
    "initialization_orchestration"
    "cancellation_race"
    "strict_mode_cleanup"
    "loser_drain"
    "bootstrap_flow"
    "negative_path"
    "recovery_path"
    "recovery_path"
    "hostile_timing"
    "hostile_timing"
    "replay_artifact"
    "replay_artifact"
    "replay_artifact"
)

STEP_HINTS=(
    "Inspect browser scheduler handoff controls in src/runtime/scheduler/three_lane.rs."
    "Inspect cancel-lane preemption ordering in scheduler browser path."
    "Inspect React provider strict-mode lifecycle accounting in tests/react_wasm_strictmode_harness.rs."
    "Inspect loser-drain + cancellation transitions for concurrent render restart."
    "Inspect Next bootstrap state transitions in src/web/nextjs_bootstrap.rs."
    "Inspect invalid-command guardrails in Next bootstrap command handling."
    "Inspect retry recovery flow for cancelled bootstrap transitions."
    "Inspect hydration mismatch recovery/reset semantics."
    "Inspect obligation ledger behavior under tab-suspension style timing gaps."
    "Inspect cancellation drain invariants under host interruption timing."
    "Inspect browser replay artifact/report generation pipeline in tests/replay_e2e_suite.rs."
    "Inspect schedule-permutation fuzz corpus artifact generation (schedule_permutation_fuzz_corpus.json) in tests/replay_e2e_suite.rs."
    "Inspect golden replay-delta drift triage bundle generation (golden_trace_replay_delta_triage_bundle.json) in tests/replay_e2e_suite.rs."
)

STEP_COMMANDS=(
    "cargo test --test scheduler_browser_determinism browser_ready_handoff_limit_bounds_burst_size -- --nocapture"
    "cargo test --test scheduler_browser_determinism browser_cancel_preempts_ready_burst -- --nocapture"
    "cargo test --test react_wasm_strictmode_harness strict_mode_double_invocation_is_leak_free_and_cancel_correct -- --nocapture"
    "cargo test --test react_wasm_strictmode_harness concurrent_render_restart_pattern_cancels_and_drains_losers -- --nocapture"
    "cargo test --test nextjs_bootstrap_harness ssr_to_hydration_bootstrap_flow_is_deterministic -- --nocapture"
    "cargo test --test nextjs_bootstrap_harness cache_revalidation_before_hydration_is_rejected -- --nocapture"
    "cargo test --test nextjs_bootstrap_harness cancelled_bootstrap_supports_retryable_recovery_path -- --nocapture"
    "cargo test --test nextjs_bootstrap_harness hydration_mismatch_recovers_via_rehydrate_path -- --nocapture"
    "cargo test --test obligation_wasm_parity wasm_host_interruption_tab_suspension_multi_obligation -- --nocapture"
    "cargo test --test obligation_wasm_parity wasm_host_interruption_during_cancel_drain -- --nocapture"
    "cargo test --test replay_e2e_suite browser_replay_report_artifact_e2e -- --nocapture"
    "cargo test --test replay_e2e_suite schedule_permutation_fuzz_regression_corpus_artifact -- --nocapture"
    "cargo test --test replay_e2e_suite golden_trace_replay_delta_report_flags_fixture_drift -- --nocapture"
)

STEP_FAULT_PROFILE=(
    "none"
    "none"
    "none"
    "none"
    "none"
    "none"
    "none"
    "none"
    "none"
    "none"
    "none"
    "none"
    "none"
)

append_fault_step() {
    local step_id="$1"
    local framework="$2"
    local category="$3"
    local hint="$4"
    local command="$5"
    local fault_profile="$6"

    STEP_IDS+=("$step_id")
    STEP_FRAMEWORK+=("$framework")
    STEP_CATEGORY+=("$category")
    STEP_HINTS+=("$hint")
    STEP_COMMANDS+=("$command")
    STEP_FAULT_PROFILE+=("$fault_profile")
}

append_fault_step \
    "network.fault_latency_spike_websocket_recovery" \
    "vanilla" \
    "network_fault_injection" \
    "Validate websocket path resilience under deterministic latency spike profile with structured fault metadata." \
    "ASUPERSYNC_TEST_FAULT_PROFILE=latency_spike ASUPERSYNC_TEST_FAULT_SEED=${TEST_SEED} cargo test --test e2e_websocket -- --nocapture" \
    "latency_spike"
append_fault_step \
    "network.fault_packet_loss_transport_recovery" \
    "vanilla" \
    "network_fault_injection" \
    "Validate transport path resilience under deterministic packet-loss profile with structured fault metadata." \
    "ASUPERSYNC_TEST_FAULT_PROFILE=packet_loss_05pct ASUPERSYNC_TEST_FAULT_SEED=${TEST_SEED} cargo test --test e2e_transport -- --nocapture" \
    "packet_loss_05pct"

if [[ "${FAULT_MATRIX_MODE}" == "full" ]]; then
    append_fault_step \
        "network.fault_disconnect_reconnect_websocket" \
        "react" \
        "network_fault_injection" \
        "Validate reconnect behavior under deterministic disconnect/reconnect fault profile." \
        "ASUPERSYNC_TEST_FAULT_PROFILE=disconnect_reconnect ASUPERSYNC_TEST_FAULT_SEED=${TEST_SEED} cargo test --test e2e_websocket -- --nocapture" \
        "disconnect_reconnect"
    append_fault_step \
        "network.fault_timeout_race_signal_path" \
        "next" \
        "network_fault_injection" \
        "Validate timeout-race behavior under deterministic timeout profile and replay-ready logs." \
        "ASUPERSYNC_TEST_FAULT_PROFILE=timeout_race ASUPERSYNC_TEST_FAULT_SEED=${TEST_SEED} cargo test --test e2e_signal -- --nocapture" \
        "timeout_race"
    append_fault_step \
        "network.fault_partial_io_transport_path" \
        "next" \
        "network_fault_injection" \
        "Validate partial read/write path under deterministic partial-io profile." \
        "ASUPERSYNC_TEST_FAULT_PROFILE=partial_io ASUPERSYNC_TEST_FAULT_SEED=${TEST_SEED} cargo test --test e2e_transport -- --nocapture" \
        "partial_io"
fi

json_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

append_step_row() {
    local row_json="$1"
    printf '%s\n' "$row_json" >> "${STEP_NDJSON}"
}

collect_trace_pointers() {
    local log_path="$1"
    grep -Eo 'artifacts/[A-Za-z0-9._/\-]+' "$log_path" 2>/dev/null \
        | sort -u \
        | head -n 12 \
        | paste -sd',' - \
        || true
}

EXIT_CODE=0
FAILED_STEP_IDS=()
FRAMEWORKS_COVERED=()
FAULT_PROFILES_EXECUTED=()
: > "${STEP_NDJSON}"

SUITE_START_EPOCH="$(date +%s)"

for idx in "${!STEP_IDS[@]}"; do
    step_id="${STEP_IDS[$idx]}"
    framework="${STEP_FRAMEWORK[$idx]}"
    category="${STEP_CATEGORY[$idx]}"
    hint="${STEP_HINTS[$idx]}"
    command_base="${STEP_COMMANDS[$idx]}"
    fault_profile="${STEP_FAULT_PROFILE[$idx]:-none}"
    target_dir_step="${step_id//[^a-zA-Z0-9]/_}"
    target_dir="/tmp/rch-wasm-cross-${TIMESTAMP}-${target_dir_step}"
    command="${RCH_BIN} exec -- env CARGO_TARGET_DIR=${target_dir} ${command_base}"
    step_log="${ARTIFACT_DIR}/${step_id}.log"

    FRAMEWORKS_COVERED+=("${framework}")
    if [[ "${fault_profile}" != "none" ]]; then
        FAULT_PROFILES_EXECUTED+=("${fault_profile}")
    fi
    started_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    step_start="$(date +%s)"

    echo ">>> [step $((idx + 1))/${#STEP_IDS[@]}] ${step_id}"

    set +e
    timeout "${STEP_TIMEOUT}" bash -lc "${command}" >"${step_log}" 2>&1
    step_rc=$?
    set -e

    step_end="$(date +%s)"
    ended_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    duration_ms=$(((step_end - step_start) * 1000))
    outcome="pass"

    if [[ "${step_rc}" -ne 0 ]]; then
        outcome="fail"
        EXIT_CODE=1
        FAILED_STEP_IDS+=("${step_id}")
    fi

    trace_pointer_csv="$(collect_trace_pointers "${step_log}")"
    if [[ -z "${trace_pointer_csv}" ]]; then
        trace_pointer_json="[]"
    else
        trace_pointer_json="$(printf '%s\n' "${trace_pointer_csv}" | tr ',' '\n' | jq -Rsc 'split("\n") | map(select(length>0))')"
    fi

    step_row="$(
        jq -cn \
            --arg schema_version "wasm-cross-framework-step-v1" \
            --arg suite_id "${SUITE_ID}" \
            --arg scenario_id "${SCENARIO_ID}" \
            --arg step_id "${step_id}" \
            --arg framework "${framework}" \
            --arg category "${category}" \
            --arg command "${command}" \
            --arg repro_command "${command}" \
            --arg started_at "${started_at}" \
            --arg ended_at "${ended_at}" \
            --arg outcome "${outcome}" \
            --arg log_path "${step_log}" \
            --arg remediation_hint "${hint}" \
            --arg fault_profile "${fault_profile}" \
            --arg fault_matrix_mode "${FAULT_MATRIX_MODE}" \
            --arg fault_seed "${TEST_SEED}" \
            --argjson exit_code "${step_rc}" \
            --argjson duration_ms "${duration_ms}" \
            --argjson trace_artifacts "${trace_pointer_json}" \
            '{
               schema_version: $schema_version,
               suite_id: $suite_id,
               scenario_id: $scenario_id,
               step_id: $step_id,
               framework: $framework,
               category: $category,
               command: $command,
               repro_command: $repro_command,
               started_at: $started_at,
               ended_at: $ended_at,
               duration_ms: $duration_ms,
               exit_code: $exit_code,
               outcome: $outcome,
               log_path: $log_path,
               trace_artifacts: $trace_artifacts,
               remediation_hint: $remediation_hint,
               fault_profile: $fault_profile,
               fault_matrix_mode: $fault_matrix_mode,
               fault_seed: $fault_seed
             }'
    )"
    append_step_row "${step_row}"

    if [[ "${EXIT_CODE}" -ne 0 ]]; then
        echo "  ERROR: ${step_id} failed (exit=${step_rc})"
        break
    fi
done

SUITE_END_EPOCH="$(date +%s)"
RUN_ENDED_TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
TOTAL_DURATION_MS=$(((SUITE_END_EPOCH - SUITE_START_EPOCH) * 1000))

frameworks_json="$(printf '%s\n' "${FRAMEWORKS_COVERED[@]}" | sort -u | jq -Rsc 'split("\n") | map(select(length>0))')"
failed_steps_json="$(printf '%s\n' "${FAILED_STEP_IDS[@]-}" | jq -Rsc 'split("\n") | map(select(length>0))')"
fault_profiles_json="$(printf '%s\n' "${FAULT_PROFILES_EXECUTED[@]-}" | sort -u | jq -Rsc 'split("\n") | map(select(length>0))')"
steps_recorded="$(wc -l < "${STEP_NDJSON}" | tr -d ' ')"
status="passed"
if [[ "${EXIT_CODE}" -ne 0 ]]; then
    status="failed"
fi

REPRO_COMMAND="TEST_LOG_LEVEL=${TEST_LOG_LEVEL} RUST_LOG=${RUST_LOG} TEST_SEED=${TEST_SEED} bash ${SCRIPT_DIR}/$(basename "$0")"

jq -n \
    --arg schema_version "e2e-suite-summary-v3" \
    --arg suite_id "${SUITE_ID}" \
    --arg scenario_id "${SCENARIO_ID}" \
    --arg seed "${TEST_SEED}" \
    --arg started_ts "${RUN_STARTED_TS}" \
    --arg ended_ts "${RUN_ENDED_TS}" \
    --arg status "${status}" \
    --arg repro_command "${REPRO_COMMAND}" \
    --arg artifact_path "${SUMMARY_FILE}" \
    --arg suite_script "${SCRIPT_DIR}/$(basename "$0")" \
    --arg log_file "${ARTIFACT_DIR}" \
    --arg artifact_dir "${ARTIFACT_DIR}" \
    --arg step_log_ndjson "${STEP_NDJSON}" \
    --arg fault_matrix_mode "${FAULT_MATRIX_MODE}" \
    --argjson duration_ms "${TOTAL_DURATION_MS}" \
    --argjson tests_passed "$((steps_recorded - ${#FAILED_STEP_IDS[@]}))" \
    --argjson tests_failed "${#FAILED_STEP_IDS[@]}" \
    --argjson exit_code "${EXIT_CODE}" \
    --argjson frameworks "${frameworks_json}" \
    --argjson fault_profiles "${fault_profiles_json}" \
    --argjson failed_steps "${failed_steps_json}" \
    --argjson step_count "${steps_recorded}" \
    '{
       "schema_version": "e2e-suite-summary-v3",
       "suite_id": $suite_id,
       "scenario_id": $scenario_id,
       "seed": $seed,
       "started_ts": $started_ts,
       "ended_ts": $ended_ts,
       duration_ms: $duration_ms,
       "status": $status,
       "repro_command": $repro_command,
       "artifact_path": $artifact_path,
       suite: $suite_id,
       timestamp: $ended_ts,
       test_log_level: env.TEST_LOG_LEVEL,
       tests_passed: $tests_passed,
       tests_failed: $tests_failed,
       exit_code: $exit_code,
       suite_script: $suite_script,
       replay_command: $repro_command,
       log_file: $log_file,
       artifact_dir: $artifact_dir,
       step_log_ndjson: $step_log_ndjson,
       fault_matrix_mode: $fault_matrix_mode,
       fault_profiles: $fault_profiles,
       fault_step_count: ($fault_profiles | length),
       frameworks_covered: $frameworks,
       failed_steps: $failed_steps,
       step_count: $step_count
     }' > "${SUMMARY_FILE}"

echo ""
echo "Summary: ${SUMMARY_FILE}"
echo "Step log NDJSON: ${STEP_NDJSON}"

if [[ "${EXIT_CODE}" -ne 0 ]]; then
    exit 1
fi

exit 0
