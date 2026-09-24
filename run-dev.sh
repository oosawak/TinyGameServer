#!/usr/bin/env bash
set -euo pipefail
export RUST_LOG=${RUST_LOG:-info}

cargo build --workspace
cargo run -p gameforge-supervisor &
SUPERVISOR_PID=$!
trap 'kill $SUPERVISOR_PID 2>/dev/null || true' EXIT
sleep 1
cargo run -p gameforge-gateway
