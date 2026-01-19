#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

echo "Kupcake Example: Mainnet Fork"
echo "This example demonstrates: Forking Ethereum mainnet for realistic testing"
echo ""
echo "What this will do:"
echo "  - Fork Ethereum mainnet at the latest block"
echo "  - Deploy OP Stack L2 with chain ID 42069"
echo "  - Run in detached mode"
echo ""
read -p "Press Enter to continue or Ctrl+C to cancel..."

if [ ! -f "$REPO_ROOT/target/release/kupcake" ]; then
    echo ""
    echo "Building kupcake..."
    cd "$REPO_ROOT" && cargo build --release
fi

echo ""
echo "Running kupcake with:"
echo "  --network kup-example-mainnet"
echo "  --l1 mainnet"
echo "  --l2-chain 42069"
echo "  --detach"
echo ""

"$REPO_ROOT/target/release/kupcake" \
    --network kup-example-mainnet \
    --l1 mainnet \
    --l2-chain 42069 \
    --detach

echo ""
echo "✅ Example completed!"
echo ""
echo "Next steps:"
echo "  - Check L1 fork: curl -X POST http://localhost:8545 -d '{\"jsonrpc\":\"2.0\",\"method\":\"eth_chainId\",\"params\":[],\"id\":1}'"
echo "  - View Grafana: http://localhost:3000"
echo "  - Cleanup: kupcake cleanup kup-example-mainnet"
echo ""
