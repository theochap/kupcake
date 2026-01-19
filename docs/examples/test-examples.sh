#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
TIMEOUT=300  # 5 minutes per example
PASSED=0
FAILED=0
TOTAL=0

# Examples to test (must have run.sh)
EXAMPLES=(
    "basic-deployment"
    "mainnet-fork"
    "single-sequencer"
    "multi-sequencer"
    "custom-images"
    "fast-blocks"
    "local-mode"
)

echo "======================================"
echo "Kupcake Examples Test Suite"
echo "======================================"
echo ""
echo "This script validates all example scenarios"
echo "Each example will run with a ${TIMEOUT}s timeout"
echo ""

# Build kupcake if needed
if [ ! -f "$REPO_ROOT/target/release/kupcake" ]; then
    echo "Building kupcake..."
    cd "$REPO_ROOT" && cargo build --release
    echo ""
fi

# Function to cleanup a network
cleanup_network() {
    local network_name=$1
    echo "  Cleaning up network: $network_name"

    # Stop and remove containers
    docker ps -q --filter name="$network_name" | xargs -r docker stop > /dev/null 2>&1 || true
    docker ps -aq --filter name="$network_name" | xargs -r docker rm > /dev/null 2>&1 || true

    # Remove network
    docker network rm "${network_name}-network" > /dev/null 2>&1 || true

    # Remove data directory
    rm -rf "$REPO_ROOT/data-$network_name" > /dev/null 2>&1 || true
}

# Function to test an example
test_example() {
    local example_name=$1
    local example_dir="$SCRIPT_DIR/$example_name"

    ((TOTAL++))

    if [ ! -d "$example_dir" ]; then
        echo -e "${RED}✗ SKIPPED${NC}: $example_name (directory not found)"
        ((FAILED++))
        return 1
    fi

    if [ ! -f "$example_dir/run.sh" ]; then
        echo -e "${RED}✗ SKIPPED${NC}: $example_name (run.sh not found)"
        ((FAILED++))
        return 1
    fi

    echo ""
    echo "Testing: $example_name"
    echo "----------------------------------------"

    # Make script executable
    chmod +x "$example_dir/run.sh"

    # Run with timeout and capture output
    local network_name="kup-example-${example_name}"

    if timeout "$TIMEOUT" bash -c "cd '$example_dir' && yes '' | ./run.sh" > /dev/null 2>&1; then
        # Check if containers are running
        local container_count=$(docker ps -q --filter name="$network_name" | wc -l)

        if [ "$container_count" -gt 0 ]; then
            echo -e "${GREEN}✅ PASSED${NC}: $example_name ($container_count containers running)"
            ((PASSED++))

            # Cleanup
            cleanup_network "$network_name"
        else
            echo -e "${RED}✗ FAILED${NC}: $example_name (no containers running)"
            ((FAILED++))
            cleanup_network "$network_name"
            return 1
        fi
    else
        local exit_code=$?
        if [ $exit_code -eq 124 ]; then
            echo -e "${RED}✗ TIMEOUT${NC}: $example_name (exceeded ${TIMEOUT}s)"
        else
            echo -e "${RED}✗ FAILED${NC}: $example_name (exit code: $exit_code)"
        fi
        ((FAILED++))
        cleanup_network "$network_name"
        return 1
    fi
}

# Run tests for each example
for example in "${EXAMPLES[@]}"; do
    test_example "$example"
done

# Summary
echo ""
echo "======================================"
echo "Test Results Summary"
echo "======================================"
echo -e "Total:  $TOTAL"
echo -e "${GREEN}Passed: $PASSED${NC}"
echo -e "${RED}Failed: $FAILED${NC}"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}All examples passed!${NC} 🎉"
    exit 0
else
    echo -e "${RED}Some examples failed.${NC}"
    exit 1
fi
