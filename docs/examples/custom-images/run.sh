#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

echo "Kupcake Example: Custom Images"
echo "This example demonstrates: Using custom Docker images"
echo ""
echo "What this will do:"
echo "  - Override default images with custom versions"
echo "  - Use specific tags for reproducible builds"
echo "  - Example: Using 'latest' tags for all services"
echo ""
read -p "Press Enter to continue or Ctrl+C to cancel..."

if [ ! -f "$REPO_ROOT/target/release/kupcake" ]; then
    echo ""
    echo "Building kupcake..."
    cd "$REPO_ROOT" && cargo build --release
fi

echo ""
echo "Running kupcake with custom images:"
echo "  --op-reth-tag latest"
echo "  --kona-node-tag latest"
echo "  --detach"
echo ""
echo "Note: This example uses 'latest' tags, but you can use specific versions"
echo "      or custom registries. See .env.example for all options."
echo ""

"$REPO_ROOT/target/release/kupcake" \
    --network kup-example-custom-images \
    --op-reth-tag latest \
    --kona-node-tag latest \
    --anvil-tag latest \
    --detach

echo ""
echo "✅ Example completed!"
echo ""
echo "Next steps:"
echo "  - Check images: docker ps --filter name=kup-example-custom-images --format 'table {{.Names}}\t{{.Image}}'"
echo "  - Copy .env.example to .env and customize for your needs"
echo "  - Cleanup: kupcake cleanup kup-example-custom-images"
echo ""
