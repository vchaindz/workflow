#!/bin/bash
# Record TUI demo with asciinema
# Usage: bash tmp/demo.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CAST_FILE="$SCRIPT_DIR/workflow_tui_demo.cast"

# Ensure release binary is up to date
echo "Building release binary..."
cargo build --release -q

# Remove old recording if present
rm -f "$CAST_FILE"

echo "Recording TUI demo..."
asciinema rec \
  -t "workflow — interactive TUI demo" \
  --idle-time-limit 2 \
  --cols 100 \
  --rows 30 \
  "$CAST_FILE" \
  -c "expect $SCRIPT_DIR/demo.exp"

echo ""
echo "Recording saved to: $CAST_FILE"
echo "Play back with: asciinema play $CAST_FILE"
echo "Upload with:    asciinema upload $CAST_FILE"
