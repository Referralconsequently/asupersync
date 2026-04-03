#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
ARTIFACT_ROOT="${STUB_SCAN_ARTIFACT_ROOT:-${PROJECT_ROOT}/artifacts}"
ARTIFACT_PATH_ROOT="${STUB_SCAN_ARTIFACT_PATH_ROOT:-${ARTIFACT_ROOT}}"
EVENTS_FILE="${ARTIFACT_ROOT}/stub_resolution_scan_events.ndjson"
SUMMARY_FILE="${ARTIFACT_ROOT}/stub_resolution_scan_summary.json"
EVENTS_PATH_FIELD="${ARTIFACT_PATH_ROOT}/stub_resolution_scan_events.ndjson"
SUMMARY_PATH_FIELD="${ARTIFACT_PATH_ROOT}/stub_resolution_scan_summary.json"
TMP_EVENTS="$(mktemp)"
TMP_SUMMARY="$(mktemp)"
BEAD_ID="asupersync-v2ofj7.10.6"
TRACK_ID="Z"
PROFILE_FAMILY="stub-resolution-scan"
COMMAND_STRING="bash ${SCRIPT_DIR}/$(basename "$0")"
CONFIG_SNAPSHOT_REF="TESTING.md::Shared Validation Contract (asupersync-ay6qvw)"
STARTED_TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

mkdir -p "$ARTIFACT_ROOT"
: >"$TMP_EVENTS"

CHECKS_TOTAL=0
FAILURES=0

json_bool() {
    if [[ "$1" -eq 1 ]]; then
        printf 'true'
    else
        printf 'false'
    fi
}

record_event() {
    local check_id="$1"
    local status="$2"
    local subject="$3"
    local detail="$4"
    local observed_outcome="passed"
    local exit_code=0

    if [[ "$status" != "pass" ]]; then
        observed_outcome="failed"
        exit_code=1
    fi

    jq -nc \
        --arg schema_version "stub-resolution-scan-event-v1" \
        --arg bead_id "$BEAD_ID" \
        --arg track_id "$TRACK_ID" \
        --arg scenario_id "$check_id" \
        --arg validation_surface "scan" \
        --arg profile_family "$PROFILE_FAMILY" \
        --argjson feature_flags '["scan"]' \
        --arg seed_or_fixture_id "none" \
        --arg config_snapshot_ref "$CONFIG_SNAPSHOT_REF" \
        --arg command "$COMMAND_STRING" \
        --arg expected_outcome "zero violations" \
        --arg observed_outcome "$observed_outcome" \
        --arg artifact_path "$SUMMARY_PATH_FIELD" \
        --arg replay_pointer "$COMMAND_STRING" \
        --arg execution_backend "local" \
        --arg evidence_owner "$BEAD_ID" \
        --arg subject "$subject" \
        --arg detail "$detail" \
        --arg exit_code "$exit_code" \
        '{
          schema_version: $schema_version,
          bead_id: $bead_id,
          track_id: $track_id,
          scenario_id: $scenario_id,
          validation_surface: $validation_surface,
          profile_family: $profile_family,
          feature_flags: $feature_flags,
          seed_or_fixture_id: $seed_or_fixture_id,
          config_snapshot_ref: $config_snapshot_ref,
          command: $command,
          expected_outcome: $expected_outcome,
          observed_outcome: $observed_outcome,
          exit_code: ($exit_code | tonumber),
          artifact_path: $artifact_path,
          replay_pointer: $replay_pointer,
          rch_routed: false,
          execution_backend: $execution_backend,
          evidence_owner: $evidence_owner,
          check_id: $scenario_id,
          subject: $subject,
          detail: $detail
        }' >>"$TMP_EVENTS"
}

report_pass() {
    local check_id="$1"
    local subject="$2"
    local detail="$3"
    CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
    printf '[PASS] %s\n' "$subject"
    record_event "$check_id" "pass" "$subject" "$detail"
}

report_fail() {
    local check_id="$1"
    local subject="$2"
    local detail="$3"
    CHECKS_TOTAL=$((CHECKS_TOTAL + 1))
    FAILURES=$((FAILURES + 1))
    printf '[FAIL] %s\n' "$subject"
    printf '       %s\n' "$detail"
    record_event "$check_id" "fail" "$subject" "$detail"
}

check_no_stray_binaries_in_src() {
    local matches
    matches="$(find "${PROJECT_ROOT}/src" -type f \( -name '*.out' -o -name '*.exe' -o -name '*.o' -o -name '*.so' -o -name '*.dylib' \) -print 2>/dev/null | sort || true)"
    if [[ -z "$matches" ]]; then
        report_pass "ZR-SCAN-NO-STRAY-BINARIES" "No stray binary artifacts under src/" "src/ tree is source-only"
    else
        report_fail "ZR-SCAN-NO-STRAY-BINARIES" "Stray binary artifacts under src/" "$matches"
    fi
}

check_no_crate_level_dead_code_allow() {
    local matches
    matches="$(rg -n '^#!\[allow\(dead_code\)\]' "${PROJECT_ROOT}/src/lib.rs" || true)"
    if [[ -z "$matches" ]]; then
        report_pass "ZR-SCAN-NO-CRATE-DEAD-CODE" "src/lib.rs has no crate-level dead_code allow" "crate root preserves the global lint"
    else
        report_fail "ZR-SCAN-NO-CRATE-DEAD-CODE" "src/lib.rs has a crate-level dead_code allow" "$matches"
    fi
}

check_combinator_compile_errors_are_gated() {
    local failures_buffer=""
    while IFS= read -r path; do
        [[ -z "$path" ]] && continue
        if ! rg -q '#\[cfg\(not\(feature = "proc-macros"\)\)\]' "$path"; then
            failures_buffer+="${path}"$'\n'
        fi
    done < <(rg -l '^[[:space:]]*compile_error!' "${PROJECT_ROOT}/src/combinator" || true)

    if [[ -z "$failures_buffer" ]]; then
        report_pass "ZR-SCAN-GUARDED-COMPILE-ERRORS" "combinator compile_error! sites are cfg-guarded" "checked src/combinator macro surfaces"
    else
        report_fail "ZR-SCAN-GUARDED-COMPILE-ERRORS" "Found combinator compile_error! files without proc-macro cfg guard" "$(printf '%s' "$failures_buffer" | sed '/^$/d')"
    fi
}

check_transport_mock_is_gated() {
    local mock_line
    mock_line="$(rg -n 'pub mod mock;' "${PROJECT_ROOT}/src/transport/mod.rs" | head -n1 || true)"
    if [[ -z "$mock_line" ]]; then
        report_pass "ZR-SCAN-TRANSPORT-MOCK-GATED" "transport/mock is not publicly exported" "src/transport/mod.rs has no public mock export"
        return 0
    fi

    local line_no
    line_no="${mock_line%%:*}"
    local start_line=1
    if (( line_no > 2 )); then
        start_line=$((line_no - 2))
    fi
    local context
    context="$(sed -n "${start_line},${line_no}p" "${PROJECT_ROOT}/src/transport/mod.rs")"
    if grep -q 'cfg' <<<"$context"; then
        report_pass "ZR-SCAN-TRANSPORT-MOCK-GATED" "transport/mock export is cfg-gated" "$(printf '%s' "$mock_line")"
    else
        report_fail "ZR-SCAN-TRANSPORT-MOCK-GATED" "transport/mock export is not cfg-gated" "$(printf '%s\n%s' "$mock_line" "$context")"
    fi
}

check_no_conformance_dummy_panics() {
    local matches
    matches="$(rg -n 'panic!\("dummy' "${PROJECT_ROOT}/conformance/src/runner.rs" || true)"
    if [[ -z "$matches" ]]; then
        report_pass "ZR-SCAN-CONFORMANCE-DUMMY-PANIC" "Conformance runner has no panic!(\"dummy\") placeholders" "conformance/src/runner.rs is free of dummy panics"
    else
        report_fail "ZR-SCAN-CONFORMANCE-DUMMY-PANIC" "Conformance runner still has panic-based dummy placeholders" "$matches"
    fi
}

check_api_skeleton_moved_out_of_root() {
    if [[ -e "${PROJECT_ROOT}/asupersync_v4_api_skeleton.rs" ]]; then
        report_fail "ZR-SCAN-API-SKELETON-ROOT" "API skeleton still lives in project root" "expected docs/design/api_skeleton_v4.rs to be the historical location"
    else
        report_pass "ZR-SCAN-API-SKELETON-ROOT" "API skeleton is no longer in the project root" "historical reference is outside the compiled source tree"
    fi
}

check_no_skeleton_placeholders_in_src() {
    local matches
    matches="$(rg -n 'skeleton_placeholder!' "${PROJECT_ROOT}/src" || true)"
    if [[ -z "$matches" ]]; then
        report_pass "ZR-SCAN-SKELETON-PLACEHOLDERS" "No skeleton_placeholder! macros remain under src/" "runtime source tree is free of API skeleton sentinels"
    else
        report_fail "ZR-SCAN-SKELETON-PLACEHOLDERS" "Found skeleton_placeholder! macros under src/" "$matches"
    fi
}

check_stub_resolution_probe_module_exists() {
    if [[ -f "${PROJECT_ROOT}/tests/stub_resolution_audit.rs" ]]; then
        report_pass "ZR-SCAN-PROBE-MODULE" "tests/stub_resolution_audit.rs exists" "probe module is available for cargo test --test stub_resolution_audit"
    else
        report_fail "ZR-SCAN-PROBE-MODULE" "tests/stub_resolution_audit.rs is missing" "Z0a probe module is not present"
    fi
}

check_no_unimplemented_in_examples_and_tests() {
    local matches
    if command -v ast-grep >/dev/null 2>&1; then
        matches="$(ast-grep run -l Rust -p 'unimplemented!()' "${PROJECT_ROOT}/examples" "${PROJECT_ROOT}/tests" 2>/dev/null || true)"
    else
        matches="$(rg -n '^[^"]*unimplemented!\(\)' "${PROJECT_ROOT}/examples" "${PROJECT_ROOT}/tests" || true)"
    fi
    if [[ -z "$matches" ]]; then
        report_pass "ZR-SCAN-NO-HARNESS-UNIMPLEMENTED" "No unimplemented!() remains in examples/ or tests/" "harness surfaces are non-panicking"
    else
        report_fail "ZR-SCAN-NO-HARNESS-UNIMPLEMENTED" "Found unimplemented!() in examples/ or tests/" "$matches"
    fi
}

check_no_stray_binaries_in_src
check_no_crate_level_dead_code_allow
check_combinator_compile_errors_are_gated
check_transport_mock_is_gated
check_no_conformance_dummy_panics
check_api_skeleton_moved_out_of_root
check_no_skeleton_placeholders_in_src
check_stub_resolution_probe_module_exists
check_no_unimplemented_in_examples_and_tests

ENDED_TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
EXIT_CODE=0
OBSERVED_OUTCOME="passed"
if (( FAILURES > 0 )); then
    EXIT_CODE=1
    OBSERVED_OUTCOME="failed"
fi

jq -nc \
    --arg schema_version "stub-resolution-scan-summary-v1" \
    --arg bead_id "$BEAD_ID" \
    --arg track_id "$TRACK_ID" \
    --arg scenario_id "ZR-SCAN-SUMMARY" \
    --arg validation_surface "scan" \
    --arg profile_family "$PROFILE_FAMILY" \
    --argjson feature_flags '["scan"]' \
    --arg seed_or_fixture_id "none" \
    --arg config_snapshot_ref "$CONFIG_SNAPSHOT_REF" \
    --arg command "$COMMAND_STRING" \
    --arg expected_outcome "zero violations" \
    --arg observed_outcome "$OBSERVED_OUTCOME" \
    --arg artifact_path "$SUMMARY_PATH_FIELD" \
    --arg replay_pointer "$COMMAND_STRING" \
    --arg execution_backend "local" \
    --arg evidence_owner "$BEAD_ID" \
    --arg events_path "$EVENTS_PATH_FIELD" \
    --arg started_ts "$STARTED_TS" \
    --arg ended_ts "$ENDED_TS" \
    --arg checks_total "$CHECKS_TOTAL" \
    --arg failures "$FAILURES" \
    --arg exit_code "$EXIT_CODE" \
    '{
      schema_version: $schema_version,
      bead_id: $bead_id,
      track_id: $track_id,
      scenario_id: $scenario_id,
      validation_surface: $validation_surface,
      profile_family: $profile_family,
      feature_flags: $feature_flags,
      seed_or_fixture_id: $seed_or_fixture_id,
      config_snapshot_ref: $config_snapshot_ref,
      command: $command,
      expected_outcome: $expected_outcome,
      observed_outcome: $observed_outcome,
      exit_code: ($exit_code | tonumber),
      artifact_path: $artifact_path,
      replay_pointer: $replay_pointer,
      rch_routed: false,
      execution_backend: $execution_backend,
      evidence_owner: $evidence_owner,
      checks_total: ($checks_total | tonumber),
      failures: ($failures | tonumber),
      started_ts: $started_ts,
      ended_ts: $ended_ts,
      events_path: $events_path
    }' >"$TMP_SUMMARY"

mv "$TMP_EVENTS" "$EVENTS_FILE"
mv "$TMP_SUMMARY" "$SUMMARY_FILE"

printf '\nSummary: %s\n' "$SUMMARY_FILE"
printf 'Events:  %s\n' "$EVENTS_FILE"
exit "$EXIT_CODE"
