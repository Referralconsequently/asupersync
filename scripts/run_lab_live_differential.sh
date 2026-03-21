#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
LOCAL_BIN="${ROOT_DIR}/target/debug/asupersync"

if [[ -x "${LOCAL_BIN}" ]]; then
  exec "${LOCAL_BIN}" lab differential "$@"
fi

cd "${ROOT_DIR}"
exec rch exec -- cargo run --features cli --bin asupersync -- lab differential "$@"
