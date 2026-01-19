#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

echo "Kupcake Example: Single Sequencer"
echo "This example demonstrates: Deploying with one sequencer (no op-conductor)"
echo ""
echo "What this will do:"
echo "  - Deploy 1 sequencer + 2 validators"
echo "  - No op-conductor (not needed)"
echo "  - Lower resource usage"
echo ""
read -p "Press Enter to continue or Ctrl+C to cancel..."

if [ ! -f "$REPO_ROOT/target/release/kupcake" ]; then
    echo ""
    echo "Building kupcake..."
    cd "$REPO_ROOT" && cargo build --release
fi

echo ""
echo "Running kupcake with:"
echo "  --network kup-example-single-sequencer"
echo "  --sequencer-count 1"
echo "  --l2-nodes 3"
echo "  --detach"
echo ""

"$REPO_ROOT/target/release/kupcake" \
    --network kup-example-single-sequencer \
    --sequencer-count 1 \
    --l2-nodes 3 \
    --detach

echo ""
echo "✅ Example completed!"
echo ""
echo "Note: op-conductor was NOT deployed (single sequencer mode)"
echo ""
echo "Next steps:"
echo "  - Check containers: docker ps --filter name=kup-example-single-sequencer | wc -l"
echo "  - Cleanup: kupcake cleanup kup-example-single-sequencer"
echo ""
