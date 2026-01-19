# Kupcake Examples

**Runnable, tested examples** demonstrating common Kupcake deployment scenarios.

## Overview

Each example includes:
- **README.md** - What the example demonstrates and expected output
- **run.sh** - Executable shell script with error handling
- **Configuration files** (optional) - `config.toml`, `.env.example`, etc.

All examples are tested and validated to ensure they work correctly.

## Quick Start

```bash
cd docs/examples/<example-name>
./run.sh
```

Each script will:
1. Build kupcake if needed
2. Run the deployment with specific flags
3. Explain what's happening
4. Show next steps or cleanup instructions

## Available Examples

### [Basic Deployment](basic-deployment/)

**Demonstrates**: Simplest possible deployment with all defaults

```bash
cd basic-deployment && ./run.sh
```

**What it does**:
- Runs in local mode (no L1 fork)
- Random L1 and L2 chain IDs
- Default configuration (2 sequencers + 3 validators)
- Detached mode for quick testing

**Use Case**: Quick testing, CI/CD, learning Kupcake basics

---

### [Mainnet Fork](mainnet-fork/)

**Demonstrates**: Forking Ethereum mainnet for realistic testing

```bash
cd mainnet-fork && ./run.sh
```

**What it does**:
- Forks Ethereum mainnet (chain ID 1)
- Uses custom L2 chain ID (42069)
- Production-like environment

**Use Case**: Testing with mainnet contract state, realistic gas prices

---

### [Single Sequencer](single-sequencer/)

**Demonstrates**: Deploying with only one sequencer (no op-conductor)

```bash
cd single-sequencer && ./run.sh
```

**What it does**:
- Deploys 1 sequencer + 2 validators
- No op-conductor (not needed for single sequencer)
- Simpler setup for development

**Use Case**: Local development, resource-constrained environments

---

### [Multi-Sequencer](multi-sequencer/)

**Demonstrates**: High-availability setup with multiple sequencers

```bash
cd multi-sequencer && ./run.sh
```

**What it does**:
- Deploys 3 sequencers + 4 validators
- op-conductor coordinates sequencers using Raft
- Leader election and failover

**Use Case**: Testing HA setups, understanding Raft consensus

---

### [Custom Images](custom-images/)

**Demonstrates**: Using custom Docker images for all services

```bash
cd custom-images && ./run.sh
```

**What it does**:
- Override default images with custom versions
- Use environment variables for image configuration
- Example `.env` file included

**Use Case**: Testing custom builds, using private registries

---

### [Fast Blocks](fast-blocks/)

**Demonstrates**: Configuring faster block times for development

```bash
cd fast-blocks && ./run.sh
```

**What it does**:
- Sets 1-second L1 block time
- Faster L2 block derivation
- Rapid feedback for testing

**Use Case**: Fast iteration during development, unit testing

---

### [Local Mode](local-mode/)

**Demonstrates**: Running without any L1 fork (local Anvil only)

```bash
cd local-mode && ./run.sh
```

**What it does**:
- No L1 fork (random L1 chain ID)
- Completely isolated from public networks
- Minimal resource usage

**Use Case**: Air-gapped testing, offline development

## Testing All Examples

Run the validation script to test all examples:

```bash
cd docs/examples
./test-examples.sh
```

This script:
- Runs each example with a timeout
- Cleans up containers after each test
- Reports pass/fail status
- Suitable for CI/CD integration

**Example output**:
```
Testing: basic-deployment
✅ PASSED: basic-deployment

Testing: mainnet-fork
✅ PASSED: mainnet-fork

Testing: single-sequencer
✅ PASSED: single-sequencer

All examples passed! (6/6)
```

## Example Script Template

All scripts follow this pattern:

```bash
#!/usr/bin/env bash
set -euo pipefail  # Exit on error, undefined var, pipe failure

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

echo "Kupcake Example: <NAME>"
echo "This example demonstrates: <DESCRIPTION>"
echo ""
read -p "Press Enter to continue or Ctrl+C to cancel..."

# Build if needed
if [ ! -f "$REPO_ROOT/target/release/kupcake" ]; then
    echo "Building kupcake..."
    cd "$REPO_ROOT" && cargo build --release
fi

# Run with specific flags
echo "Running kupcake with:"
echo "  --flag value"
echo ""

"$REPO_ROOT/target/release/kupcake" --flag value

echo ""
echo "Example completed!"
echo "Next steps:"
echo "  - Check Grafana at http://localhost:3000"
echo "  - View logs: docker logs <container-name>"
echo "  - Cleanup: kupcake cleanup <network-name>"
```

## Common Patterns

### Detached Mode for Quick Testing

```bash
kupcake --detach --network test
# Returns immediately, containers run in background
docker ps
kupcake cleanup test
```

### Using Configuration Files

```bash
# Save configuration
kupcake --network my-network
# config saved to ./data-my-network/Kupcake.toml

# Reload later
kupcake --config ./data-my-network/Kupcake.toml
```

### Environment Variables

```bash
# Set via environment
export KUP_L1=sepolia
export KUP_L2_CHAIN=42069
export KUP_NETWORK_NAME=my-test
kupcake
```

### Override Specific Settings

```bash
# Load config but change block time
kupcake --config ./saved-config.toml --block-time 1
```

## Cleanup

All examples use detached mode and recommend cleanup:

```bash
kupcake cleanup <network-name>
```

Or for foreground runs:
```bash
# Press Ctrl+C - containers are automatically cleaned up
```

## Troubleshooting

### Script Permission Denied

```bash
chmod +x run.sh
./run.sh
```

### Build Fails

Ensure you have Rust installed:
```bash
rustc --version  # Should be 1.75+
cargo build --release
```

### Docker Not Running

```bash
docker ps  # Test Docker connectivity
sudo systemctl start docker  # Linux
open -a Docker  # macOS
```

### Port Conflicts

Use a unique network name:
```bash
./run.sh
# Edit run.sh to use --network <unique-name>
```

## MCP Configuration

The [`mcp-config.json`](mcp-config.json) file provides a sample configuration for exposing Kupcake documentation to AI assistants via the Model Context Protocol.

This allows AI assistants (like Claude Desktop) to directly access and search Kupcake documentation.

**Quick Setup**:
1. Copy the content from `mcp-config.json`
2. Update the path to point to your Kupcake docs directory
3. Add to your Claude Desktop configuration
4. Restart Claude Desktop

See [MCP Integration Guide](../user-guide/mcp-integration.md) for complete setup instructions.

## Related Documentation

- [Quickstart Guide](../getting-started/quickstart.md) - First deployment walkthrough
- [CLI Reference](../user-guide/cli-reference.md) - All CLI options
- [Configuration File](../user-guide/configuration-file.md) - Kupcake.toml structure
- [MCP Integration](../user-guide/mcp-integration.md) - Expose docs to AI assistants
- [Troubleshooting](../user-guide/troubleshooting.md) - Common issues

## Contributing Examples

Want to add an example? Follow these guidelines:

1. Create a new directory in `docs/examples/<example-name>/`
2. Include `README.md` and `run.sh`
3. Use the script template above
4. Add error handling (`set -euo pipefail`)
5. Test your example with `./test-examples.sh`
6. Update this README with a link to your example

See [Contributing Guide](../developer-guide/contributing.md) for details.

## CI/CD Integration

Use these examples in CI/CD pipelines:

```yaml
# GitHub Actions example
- name: Test Kupcake Deployment
  run: |
    cd docs/examples/basic-deployment
    timeout 5m ./run.sh
    docker ps
    kupcake cleanup kup-example-basic
```

```bash
# GitLab CI example
script:
  - cd docs/examples/basic-deployment
  - timeout 5m ./run.sh
  - docker ps
  - kupcake cleanup kup-example-basic
```

## Next Steps

1. Try the [Basic Deployment](basic-deployment/) example
2. Explore [CLI Reference](../user-guide/cli-reference.md) for all options
3. Read [Architecture Overview](../architecture/overview.md) to understand how it works
4. Check [Service Documentation](../services/README.md) for component details
