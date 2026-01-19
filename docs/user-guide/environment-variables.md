# Environment Variables

**Target Audience**: Operators | CI/CD Users

All Kupcake CLI options can be configured using environment variables with the `KUP_` prefix.

## Why Use Environment Variables?

- **CI/CD Integration**: Configure deployments without modifying scripts
- **Consistency**: Same configuration across multiple runs
- **Security**: Keep sensitive values out of command history
- **Convenience**: Set defaults without typing long commands

## Precedence Rules

When the same option is specified multiple ways, Kupcake uses this order (highest to lowest):

1. **CLI Arguments** (e.g., `--network my-network`)
2. **Environment Variables** (e.g., `KUP_NETWORK_NAME=my-network`)
3. **Configuration File** (if `--config` is used)
4. **Built-in Defaults**

## Network Configuration

### `KUP_NETWORK_NAME`

Custom network name.

```bash
export KUP_NETWORK_NAME=my-testnet
kupcake
# Equivalent to: kupcake --network my-testnet
```

### `KUP_L1`

L1 chain source (chain name or RPC URL).

```bash
# Fork Sepolia
export KUP_L1=sepolia

# Fork Mainnet
export KUP_L1=mainnet

# Custom RPC
export KUP_L1=https://eth-mainnet.g.alchemy.com/v2/YOUR-KEY

# Local mode (no fork)
unset KUP_L1
# or don't set it at all
```

### `KUP_L2_CHAIN`

L2 chain identifier.

```bash
export KUP_L2_CHAIN=42069
kupcake
# Equivalent to: kupcake --l2-chain 42069
```

## Deployment Behavior

### `KUP_REDEPLOY`

Force contract redeployment.

```bash
export KUP_REDEPLOY=true
kupcake
# Equivalent to: kupcake --redeploy
```

### `KUP_OUTDATA`

Output data directory path.

```bash
export KUP_OUTDATA=/tmp/kupcake-data
kupcake
```

### `KUP_NO_CLEANUP`

Skip container cleanup on exit.

```bash
export KUP_NO_CLEANUP=true
kupcake
# Containers keep running after Ctrl+C
```

### `KUP_DETACH`

Run in detached mode.

```bash
export KUP_DETACH=true
kupcake
# Deploys and exits immediately
```

### `KUP_PUBLISH_ALL_PORTS`

Publish all exposed ports to random host ports.

```bash
export KUP_PUBLISH_ALL_PORTS=true
kupcake
```

## Chain Configuration

### `KUP_BLOCK_TIME`

Block time in seconds.

```bash
export KUP_BLOCK_TIME=2
kupcake
# 2-second blocks
```

### `KUP_L2_NODES`

Total number of L2 nodes.

```bash
export KUP_L2_NODES=7
kupcake
```

### `KUP_SEQUENCERS`

Number of sequencer nodes.

```bash
export KUP_SEQUENCERS=3
kupcake
```

### `KUP_CONFIG`

Path to configuration file.

```bash
export KUP_CONFIG=./saved-config.toml
kupcake
```

## Logging

### `KUP_VERBOSITY`

Logging verbosity level.

**Values**: `off`, `error`, `warn`, `info`, `debug`, `trace`

```bash
export KUP_VERBOSITY=debug
kupcake -v debug  # CLI arg takes precedence
```

## Docker Image Overrides

All Docker images and tags can be overridden via environment variables.

### Anvil (L1)

```bash
export KUP_ANVIL_IMAGE=ghcr.io/foundry-rs/foundry
export KUP_ANVIL_TAG=nightly
```

### op-reth (L2 Execution)

```bash
export KUP_OP_RETH_IMAGE=ghcr.io/op-rs/op-reth
export KUP_OP_RETH_TAG=v1.0.0
```

### kona-node (L2 Consensus)

```bash
export KUP_KONA_NODE_IMAGE=ghcr.io/op-rs/kona
export KUP_KONA_NODE_TAG=v0.5.0
```

### op-batcher

```bash
export KUP_OP_BATCHER_IMAGE=ghcr.io/ethereum-optimism/op-batcher
export KUP_OP_BATCHER_TAG=latest
```

### op-proposer

```bash
export KUP_OP_PROPOSER_IMAGE=ghcr.io/ethereum-optimism/op-proposer
export KUP_OP_PROPOSER_TAG=latest
```

### op-challenger

```bash
export KUP_OP_CHALLENGER_IMAGE=ghcr.io/ethereum-optimism/op-challenger
export KUP_OP_CHALLENGER_TAG=latest
```

### op-conductor

```bash
export KUP_OP_CONDUCTOR_IMAGE=ghcr.io/ethereum-optimism/op-conductor
export KUP_OP_CONDUCTOR_TAG=latest
```

### op-deployer

```bash
export KUP_OP_DEPLOYER_IMAGE=ghcr.io/ethereum-optimism/op-deployer
export KUP_OP_DEPLOYER_TAG=latest
```

### Prometheus

```bash
export KUP_PROMETHEUS_IMAGE=prom/prometheus
export KUP_PROMETHEUS_TAG=latest
```

### Grafana

```bash
export KUP_GRAFANA_IMAGE=grafana/grafana
export KUP_GRAFANA_TAG=latest
```

## Local Binary Paths

Deploy services from local binaries instead of Docker images.

**Note**: When a binary path is set, the corresponding image and tag variables are ignored.

### op-reth

```bash
export KUP_OP_RETH_BINARY=./op-reth/target/release/op-reth
```

Deploy op-reth from a local binary.

### kona-node

```bash
export KUP_KONA_NODE_BINARY=./kona/target/release/kona-node
```

Deploy kona-node from a local binary.

### op-batcher

```bash
export KUP_OP_BATCHER_BINARY=./optimism/op-batcher/bin/op-batcher
```

Deploy op-batcher from a local binary.

### op-proposer

```bash
export KUP_OP_PROPOSER_BINARY=./optimism/op-proposer/bin/op-proposer
```

Deploy op-proposer from a local binary.

### op-challenger

```bash
export KUP_OP_CHALLENGER_BINARY=./optimism/op-challenger/bin/op-challenger
```

Deploy op-challenger from a local binary.

### op-conductor

```bash
export KUP_OP_CONDUCTOR_BINARY=./optimism/op-conductor/bin/op-conductor
```

Deploy op-conductor from a local binary.

### Example: Development with Local Binaries

```bash
#!/bin/bash
# dev-deploy.sh - Deploy with locally built components

# Build services locally
cd ~/kona && cargo build --release --bin kona-node
cd ~/op-reth && cargo build --release

# Set binary paths
export KUP_KONA_NODE_BINARY=~/kona/target/release/kona-node
export KUP_OP_RETH_BINARY=~/op-reth/target/release/op-reth

# Network config
export KUP_NETWORK_NAME=dev-local
export KUP_BLOCK_TIME=2
export KUP_PUBLISH_ALL_PORTS=true
export KUP_DETACH=true

# Deploy
kupcake
```

**Binary Requirements**:
- Must be compiled for Linux
- Must be compatible with GLIBC 2.38 or earlier
- Must be executable (`chmod +x`)

**See**: [Docker Images Guide - Local Binary Deployment](docker-images.md#local-binary-deployment)

## Complete Example: CI/CD Configuration

```bash
#!/bin/bash
# ci-deploy.sh - Example CI/CD deployment script

# Network configuration
export KUP_NETWORK_NAME=ci-test-${CI_PIPELINE_ID}
export KUP_L1=sepolia
export KUP_L2_CHAIN=42069

# Deployment behavior
export KUP_DETACH=true
export KUP_OUTDATA=/tmp/kupcake-${CI_PIPELINE_ID}

# Fast blocks for testing
export KUP_BLOCK_TIME=1

# Single sequencer (faster startup)
export KUP_SEQUENCERS=1
export KUP_L2_NODES=2

# Debug logging
export KUP_VERBOSITY=debug

# Run deployment
kupcake

# Run tests...
pytest tests/

# Cleanup
kupcake cleanup ci-test-${CI_PIPELINE_ID}
rm -rf /tmp/kupcake-${CI_PIPELINE_ID}
```

## Using .env Files

Create a `.env` file for consistent configuration:

```bash
# .env
KUP_NETWORK_NAME=my-dev-network
KUP_L1=sepolia
KUP_L2_CHAIN=42069
KUP_BLOCK_TIME=2
KUP_VERBOSITY=info
KUP_OP_RETH_TAG=latest
KUP_KONA_NODE_TAG=latest
```

Load and run:

```bash
source .env
kupcake
```

Or use `env` command:

```bash
env $(cat .env | xargs) kupcake
```

## Viewing Current Environment

```bash
# Show all KUP_* environment variables
env | grep KUP_

# Example output:
# KUP_NETWORK_NAME=my-network
# KUP_L1=sepolia
# KUP_VERBOSITY=debug
```

## Unsetting Environment Variables

```bash
# Unset a single variable
unset KUP_L1

# Unset all KUP_* variables
unset $(env | grep '^KUP_' | cut -d= -f1)
```

## Common Patterns

### Development Setup

```bash
# dev.env
KUP_NETWORK_NAME=dev
KUP_BLOCK_TIME=1
KUP_SEQUENCERS=1
KUP_L2_NODES=2
KUP_VERBOSITY=debug
```

### Production-Like Setup

```bash
# prod.env
KUP_NETWORK_NAME=prod-test
KUP_L1=mainnet
KUP_BLOCK_TIME=12
KUP_SEQUENCERS=3
KUP_L2_NODES=7
KUP_VERBOSITY=info
```

### Custom Registry

```bash
# registry.env
KUP_OP_RETH_IMAGE=myregistry.io/op-reth
KUP_OP_RETH_TAG=dev
KUP_KONA_NODE_IMAGE=myregistry.io/kona
KUP_KONA_NODE_TAG=dev
```

## Related Documentation

- [CLI Reference](cli-reference.md) - All command-line options
- [Configuration File](configuration-file.md) - Using Kupcake.toml
- [Custom Images Example](../examples/custom-images/) - .env file usage
