#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

echo "Kupcake Example: Basic Deployment"
echo "This example demonstrates: Simplest deployment with all defaults"
echo ""
echo "What this will do:"
echo "  - Deploy a complete OP Stack L2 network"
echo "  - Use local mode (no L1 fork)"
echo "  - Run in detached mode (exits immediately)"
echo "  - Network name: kup-example-basic"
echo ""
read -p "Press Enter to continue or Ctrl+C to cancel..."

# Build if needed
if [ ! -f "$REPO_ROOT/target/release/kupcake" ]; then
    echo ""
    echo "Building kupcake (this may take a few minutes)..."
    cd "$REPO_ROOT" && cargo build --release
fi

# Run with specific flags
echo ""
echo "Running kupcake with:"
echo "  --network kup-example-basic"
echo "  --detach"
echo ""

"$REPO_ROOT/target/release/kupcake" \
    --network kup-example-basic \
    --detach

echo ""
echo "✅ Example completed!"
echo ""
echo "Deployment is running in the background."
echo ""
echo "Next steps:"
echo "  1. Check containers:  docker ps --filter name=kup-example-basic"
echo "  2. View Grafana:      http://localhost:3000 (admin/admin)"
echo "  3. View L1 logs:      docker logs kup-example-basic-anvil"
echo "  4. View sequencer:    docker logs kup-example-basic-op-reth-sequencer-1"
echo "  5. Cleanup:           kupcake cleanup kup-example-basic"
echo ""
echo "Data saved to: ./data-kup-example-basic/"
echo ""
