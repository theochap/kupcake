#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

echo "Kupcake Example: Multi-Sequencer"
echo "This example demonstrates: High-availability with multiple sequencers"
echo ""
echo "What this will do:"
echo "  - Deploy 3 sequencers + 4 validators"
echo "  - Deploy op-conductor for Raft coordination"
echo "  - Sequencer 1 starts as active leader"
echo "  - Sequencers 2-3 start in standby mode"
echo ""
read -p "Press Enter to continue or Ctrl+C to cancel..."

if [ ! -f "$REPO_ROOT/target/release/kupcake" ]; then
    echo ""
    echo "Building kupcake..."
    cd "$REPO_ROOT" && cargo build --release
fi

echo ""
echo "Running kupcake with:"
echo "  --network kup-example-multi-sequencer"
echo "  --sequencer-count 3"
echo "  --l2-nodes 7"
echo "  --detach"
echo ""

"$REPO_ROOT/target/release/kupcake" \
    --network kup-example-multi-sequencer \
    --sequencer-count 3 \
    --l2-nodes 7 \
    --detach

echo ""
echo "✅ Example completed!"
echo ""
echo "Multi-sequencer setup with op-conductor coordination"
echo ""
echo "Next steps:"
echo "  - View conductor logs: docker logs kup-example-multi-sequencer-op-conductor"
echo "  - Check leader status: docker logs kup-example-multi-sequencer-op-conductor | grep leader"
echo "  - Test failover: docker stop kup-example-multi-sequencer-op-reth-sequencer-1"
echo "  - Cleanup: kupcake cleanup kup-example-multi-sequencer"
echo ""
