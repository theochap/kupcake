#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

echo "Kupcake Example: Local Mode"
echo "This example demonstrates: Running without L1 fork (local only)"
echo ""
echo "What this will do:"
echo "  - Run Anvil WITHOUT forking any chain"
echo "  - Generate random L1 chain ID"
echo "  - Completely isolated from public networks"
echo "  - No external RPC dependencies"
echo ""
read -p "Press Enter to continue or Ctrl+C to cancel..."

if [ ! -f "$REPO_ROOT/target/release/kupcake" ]; then
    echo ""
    echo "Building kupcake..."
    cd "$REPO_ROOT" && cargo build --release
fi

echo ""
echo "Running kupcake with:"
echo "  --network kup-example-local"
echo "  --detach"
echo "  (no --l1 flag = local mode)"
echo ""

"$REPO_ROOT/target/release/kupcake" \
    --network kup-example-local \
    --detach

echo ""
echo "✅ Example completed!"
echo ""
echo "Running in fully local mode (no L1 fork)"
echo ""
echo "Next steps:"
echo "  - Check L1 chain ID: curl -X POST http://localhost:8545 -d '{\"jsonrpc\":\"2.0\",\"method\":\"eth_chainId\",\"params\":[],\"id\":1}'"
echo "  - View Grafana: http://localhost:3000"
echo "  - Cleanup: kupcake cleanup kup-example-local"
echo ""
