#!/bin/bash
set -euo pipefail

ROOT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$ROOT_DIR"

(cd "$ROOT_DIR/rust" && cargo test --release -q)

node "$ROOT_DIR/tests/model.test.js"
"$ROOT_DIR/tests/qml.test.sh"
omarchy plugin validate "$ROOT_DIR"

echo "all tests passed"
