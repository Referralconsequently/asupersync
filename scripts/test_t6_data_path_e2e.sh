#!/usr/bin/env bash
# T6.13 Data-Path E2E Test Runner (asupersync-2oh2u.6.13)
#
# Runs the T6 end-to-end data-path tests covering database pool lifecycle,
# retry/fault-injection, error classification parity, messaging error variants,
# and full warm→acquire→fault→recover workflows.
#
# Usage:
#   ./scripts/test_t6_data_path_e2e.sh [test_filter]
#
# Environment Variables:
#   TEST_LOG_LEVEL - error|warn|info|debug|trace (default: trace)
#   RUST_LOG       - tracing filter (default: asupersync=debug)
#   RUST_BACKTRACE - 1 to enable backtraces (default: 1)
#   TEST_SEED      - deterministic seed override (default: 0xDEADBEEF)
#
# Pass/Fail Semantics:
#   PASS when cargo test exits 0 and no failure patterns are detected.
#   FAIL when cargo test is non-zero or any failure pattern is detected.
#
# Artifact Bundle:
#   summary.json + suite log + extracted seeds/correlation IDs under
#   target/e2e-results/t6-data-path/artifacts_<timestamp>/.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="${PROJECT_ROOT}/target/e2e-results/t6-data-path"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
RUN_STARTED_TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
LOG_FILE="${OUTPUT_DIR}/t6_data_path_e2e_${TIMESTAMP}.log"
ARTIFACT_DIR="${OUTPUT_DIR}/artifacts_${TIMESTAMP}"
TEST_FILTER="${1:-}"

export TEST_LOG_LEVEL="${TEST_LOG_LEVEL:-trace}"
export RUST_LOG="${RUST_LOG:-asupersync=debug}"
export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"
export TEST_SEED="${TEST_SEED:-0xDEADBEEF}"

mkdir -p "$OUTPUT_DIR" "$ARTIFACT_DIR"

echo "==================================================================="
echo "         Asupersync T6 Data-Path E2E Tests (T6.13)                "
echo "==================================================================="
echo ""
echo "Config:"
echo "  TEST_LOG_LEVEL:  ${TEST_LOG_LEVEL}"
echo "  RUST_LOG:        ${RUST_LOG}"
echo "  TEST_SEED:       ${TEST_SEED}"
echo "  Timestamp:       ${TIMESTAMP}"
echo "  Output:          ${LOG_FILE}"
echo "  Artifacts:       ${ARTIFACT_DIR}"
echo ""

# --- Section: Build Check ---
echo ">>> [1/5] Pre-flight: checking compilation..."
if ! cargo check --test e2e_t6_data_path --features "sqlite,postgres,mysql" 2>"${ARTIFACT_DIR}/compile_errors.log"; then
    echo "  FATAL: compilation failed — see ${ARTIFACT_DIR}/compile_errors.log"
    exit 1
fi
echo "  OK"

# --- Section: Run Tests ---
echo ""
echo ">>> [2/5] Running T6 data-path E2E tests..."

TEST_RESULT=0
CARGO_ARGS=(--test e2e_t6_data_path --features "sqlite,postgres,mysql")
RUN_ARGS=(--nocapture --test-threads=1)

if [ -n "$TEST_FILTER" ]; then
    RUN_ARGS+=("$TEST_FILTER")
fi

pushd "$PROJECT_ROOT" >/dev/null
if timeout 300 cargo test "${CARGO_ARGS[@]}" -- "${RUN_ARGS[@]}" 2>&1 | tee "$LOG_FILE"; then
    TEST_RESULT=0
else
    TEST_RESULT=$?
fi
popd >/dev/null

# --- Section: Failure Pattern Analysis ---
echo ""
echo ">>> [3/5] Checking output for failure patterns..."

PATTERN_FAILURES=0

check_pattern() {
    local pattern="$1"
    local label="$2"
    if grep -q "$pattern" "$LOG_FILE" 2>/dev/null; then
        echo "  ERROR: ${label}"
        grep -n "$pattern" "$LOG_FILE" | head -5 > "${ARTIFACT_DIR}/${label// /_}.txt" 2>/dev/null || true
        ((PATTERN_FAILURES++)) || true
    fi
}

check_pattern "panicked at"          "panic detected"
check_pattern "assertion failed"     "assertion failure"
check_pattern "test result: FAILED"  "cargo reported failures"
check_pattern "pool exhausted"       "pool exhaustion"
check_pattern "Busy loop detected"   "busy loop detected"
check_pattern "Task leak detected"   "task leak detected"
check_pattern "OBLIGATION TOKEN"     "obligation token leak"
check_pattern "resource.*leak"       "resource leak"

if [ "$PATTERN_FAILURES" -eq 0 ]; then
    echo "  No failure patterns found"
fi

# --- Section: Reliability Signal Extraction ---
echo ""
echo ">>> [4/5] Extracting reliability signals..."

# Extract correlation IDs
grep -oE "correlation_id[= ]+T6\.13:[a-z0-9_:.-]+" "$LOG_FILE" > "${ARTIFACT_DIR}/correlation_ids.txt" 2>/dev/null || true
CORRELATION_COUNT=$(wc -l < "${ARTIFACT_DIR}/correlation_ids.txt" 2>/dev/null || echo "0")
echo "  Correlation IDs extracted: ${CORRELATION_COUNT}"

# Extract retry attempts
grep -oE "attempts[= ]+[0-9]+" "$LOG_FILE" > "${ARTIFACT_DIR}/retry_attempts.txt" 2>/dev/null || true

# Extract pool stats snapshots
grep -oE "total[= ]+[0-9]+.*idle[= ]+[0-9]+.*active[= ]+[0-9]+" "$LOG_FILE" > "${ARTIFACT_DIR}/pool_stats.txt" 2>/dev/null || true

# Extract error classifications
grep -oE "(is_transient|is_retryable|is_connection_error)[= ]+[a-z]+" "$LOG_FILE" > "${ARTIFACT_DIR}/error_classifications.txt" 2>/dev/null || true

# Extract seeds
grep -oE "seed[= ]+0x[0-9a-fA-F]+" "$LOG_FILE" > "${ARTIFACT_DIR}/seeds.txt" 2>/dev/null || true

echo "  Reliability signals collected"

# --- Section: Artifact Collection ---
echo ""
echo ">>> [5/5] Collecting artifacts..."

PASSED=$(grep -c "^test .* ok$" "$LOG_FILE" 2>/dev/null || echo "0")
FAILED=$(grep -c "^test .* FAILED$" "$LOG_FILE" 2>/dev/null || echo "0")
SUITE_ID="t6_data_path_e2e"
SCENARIO_ID="E2E-SUITE-T6-DATA-PATH"
SUMMARY_FILE="${ARTIFACT_DIR}/summary.json"
REPRO_COMMAND="TEST_LOG_LEVEL=${TEST_LOG_LEVEL} RUST_LOG=${RUST_LOG} TEST_SEED=${TEST_SEED} bash ${SCRIPT_DIR}/$(basename "$0")"
RUN_ENDED_TS="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
SUITE_STATUS="failed"
if [ "$TEST_RESULT" -eq 0 ] && [ "$PATTERN_FAILURES" -eq 0 ]; then
    SUITE_STATUS="passed"
fi
FAILURE_CLASS="test_or_pattern_failure"
if [ "$SUITE_STATUS" = "passed" ]; then
    FAILURE_CLASS="none"
fi

cat > "${SUMMARY_FILE}" << ENDJSON
{
  "schema_version": "e2e-suite-summary-v3",
  "suite_id": "${SUITE_ID}",
  "scenario_id": "${SCENARIO_ID}",
  "seed": "${TEST_SEED}",
  "started_ts": "${RUN_STARTED_TS}",
  "ended_ts": "${RUN_ENDED_TS}",
  "status": "${SUITE_STATUS}",
  "failure_class": "${FAILURE_CLASS}",
  "repro_command": "${REPRO_COMMAND}",
  "artifact_path": "${SUMMARY_FILE}",
  "suite": "${SUITE_ID}",
  "timestamp": "${TIMESTAMP}",
  "test_log_level": "${TEST_LOG_LEVEL}",
  "tests_passed": ${PASSED},
  "tests_failed": ${FAILED},
  "exit_code": ${TEST_RESULT},
  "pattern_failures": ${PATTERN_FAILURES},
  "correlation_ids_extracted": ${CORRELATION_COUNT},
  "log_file": "${LOG_FILE}",
  "artifact_dir": "${ARTIFACT_DIR}",
  "bead_id": "asupersync-2oh2u.6.13",
  "track": "T6",
  "domains": ["database", "messaging", "pool", "retry", "error_classification"]
}
ENDJSON

echo "  Summary: ${SUMMARY_FILE}"

# --- Summary ---
echo ""
echo "==================================================================="
echo "                T6 DATA-PATH E2E SUMMARY                          "
echo "==================================================================="
echo "  Bead:     asupersync-2oh2u.6.13"
echo "  Seed:     ${TEST_SEED}"
echo "  Passed:   ${PASSED}"
echo "  Failed:   ${FAILED}"
echo "  Patterns: ${PATTERN_FAILURES} failure patterns"
echo "  Corr IDs: ${CORRELATION_COUNT}"
echo ""

if [ "$TEST_RESULT" -eq 0 ] && [ "$PATTERN_FAILURES" -eq 0 ]; then
    echo "  Status: PASSED"
else
    echo "  Status: FAILED"
    echo "  Logs:   ${LOG_FILE}"
    echo "  Artifacts: ${ARTIFACT_DIR}"
fi
echo "==================================================================="

# Clean up empty artifact files
find "$ARTIFACT_DIR" -name "*.txt" -empty -delete 2>/dev/null || true

if [ "$TEST_RESULT" -ne 0 ] || [ "$PATTERN_FAILURES" -ne 0 ]; then
    exit 1
fi
