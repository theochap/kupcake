#!/bin/bash
# Local Binary Deployment Example
#
# This example demonstrates deploying with a locally-built kona-node binary.
#
# Prerequisites:
# - Kona source code cloned and built
# - Set KONA_PATH environment variable to the kona directory

set -euo pipefail

# Configuration
NETWORK_NAME="example-local-binary"
KONA_PATH="${KONA_PATH:-../../../kona}"  # Default to assuming kona is a sibling directory
BINARY_PATH="${KONA_PATH}/target/release/kona-node"

echo "========================================="
echo "Local Binary Deployment Example"
echo "========================================="
echo

# Check if kona directory exists
if [ ! -d "$KONA_PATH" ]; then
    echo "❌ Error: Kona directory not found at: $KONA_PATH"
    echo
    echo "Please set KONA_PATH to your kona directory:"
    echo "  export KONA_PATH=/path/to/kona"
    echo "  ./run.sh"
    echo
    echo "Or clone kona:"
    echo "  git clone https://github.com/anton-rs/kona"
    echo "  cd kona"
    echo "  cargo build --release --bin kona-node"
    exit 1
fi

# Check if binary exists
if [ ! -f "$BINARY_PATH" ]; then
    echo "⚙️  Building kona-node binary..."
    echo "This may take several minutes on first build."
    echo
    cd "$KONA_PATH"
    cargo build --release --bin kona-node
    cd - > /dev/null
else
    echo "✓ Found existing kona-node binary"
fi

# Verify binary is executable and for Linux
echo
echo "📋 Binary Information:"
file "$BINARY_PATH"
ls -lh "$BINARY_PATH"

# Cleanup any existing deployment
echo
echo "🧹 Cleaning up any existing deployment..."
kupcake cleanup "$NETWORK_NAME" 2>/dev/null || true

# Deploy with local binary
echo
echo "🚀 Deploying with local kona-node binary..."
echo "Network: $NETWORK_NAME"
echo "Binary: $BINARY_PATH"
echo

kupcake \
  --network "$NETWORK_NAME" \
  --kona-node-binary "$BINARY_PATH" \
  --l2-nodes 2 \
  --sequencer-count 1 \
  --block-time 2 \
  --publish-all-ports \
  --detach

echo
echo "✅ Deployment complete!"
echo

# Show generated Docker images
echo "📦 Generated Docker Images:"
docker images --filter "reference=kupcake-${NETWORK_NAME}-*-local*"
echo

# Get kona-node port
echo "🔍 Getting kona-node RPC port..."
KONA_CONTAINER="${NETWORK_NAME}-kona-node"
KONA_PORT=$(docker port "$KONA_CONTAINER" 7545 2>/dev/null | cut -d: -f2 || echo "N/A")

if [ "$KONA_PORT" != "N/A" ]; then
    echo "✓ Kona-node RPC available at: http://localhost:$KONA_PORT"
    echo

    # Wait for node to be ready
    echo "⏳ Waiting for node to be ready..."
    sleep 5

    # Query sync status
    echo
    echo "📊 Sync Status:"
    curl -s -X POST "http://localhost:$KONA_PORT" \
      -H "Content-Type: application/json" \
      -d '{
        "jsonrpc": "2.0",
        "method": "optimism_syncStatus",
        "params": [],
        "id": 1
      }' | jq '{
        unsafe_l2: .result.unsafe_l2.number,
        safe_l2: .result.safe_l2.number,
        finalized_l2: .result.finalized_l2.number
      }'
else
    echo "⚠️  Could not determine kona-node port"
fi

echo
echo "========================================="
echo "Example Complete!"
echo "========================================="
echo
echo "View logs:"
echo "  docker logs $NETWORK_NAME-kona-node"
echo
echo "Query node:"
echo "  curl -X POST http://localhost:$KONA_PORT -H 'Content-Type: application/json' \\"
echo "    -d '{\"jsonrpc\":\"2.0\",\"method\":\"optimism_syncStatus\",\"params\":[],\"id\":1}'"
echo
echo "Clean up:"
echo "  kupcake cleanup $NETWORK_NAME"
echo
