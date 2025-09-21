#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
TMP_DIR=$(mktemp -d)
DATA_DIR="$TMP_DIR/server-data"
mkdir -p "$DATA_DIR"

find_free_port() {
  python3 - <<'PY'
import socket
sock = socket.socket()
sock.bind(('127.0.0.1', 0))
port = sock.getsockname()[1]
sock.close()
print(port)
PY
}

PORT=$(find_free_port)
SERVER_ADDR="127.0.0.1:${PORT}"

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

export MOSAIC_DATA_DIR="$DATA_DIR"
export MOSAIC_SERVER_ADDR="$SERVER_ADDR"

cargo run --example server --manifest-path "$ROOT_DIR/Cargo.toml" >/dev/null &
SERVER_PID=$!

# Give the server a moment to come up
sleep 1

cargo run --example client --manifest-path "$ROOT_DIR/Cargo.toml"

kill "$SERVER_PID" 2>/dev/null || true
wait "$SERVER_PID" 2>/dev/null || true
