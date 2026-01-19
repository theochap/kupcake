#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

echo "Kupcake Example: Fast Blocks"
echo "This example demonstrates: Configuring 1-second block times"
echo ""
echo "What this will do:"
echo "  - L1 blocks every 1 second (vs. 12s default)"
echo "  - Faster L2 block derivation"
echo "  - Rapid feedback for testing"
echo ""
read -p "Press Enter to continue or Ctrl+C to cancel..."

if [ ! -f "$REPO_ROOT/target/release/kupcake" ]; then
    echo ""
    echo "Building kupcake..."
    cd "$REPO_ROOT" && cargo build --release
fi

echo ""
echo "Running kupcake with:"
echo "  --network kup-example-fast-blocks"
echo "  --block-time 1"
echo "  --detach"
echo ""

"$REPO_ROOT/target/release/kupcake" \
    --network kup-example-fast-blocks \
    --block-time 1 \
    --detach

echo ""
echo "✅ Example completed!"
echo ""
echo "Blocks are being produced every 1 second!"
echo ""
echo "Next steps:"
echo "  - Watch L1 blocks: watch -n 1 'curl -s -X POST http://localhost:8545 -d \"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"method\\\":\\\"eth_blockNumber\\\",\\\"params\\\":[],\\\"id\\\":1}\" | jq -r \".result\"'"
echo "  - View Grafana: http://localhost:3000"
echo "  - Cleanup: kupcake cleanup kup-example-fast-blocks"
echo ""
