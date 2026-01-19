# CLI Reference

**Target Audience**: Operators | Advanced Users
**Complete reference** for all Kupcake CLI commands and options

## Command Structure

```bash
kupcake [OPTIONS] [COMMAND]
```

If no command is specified, `deploy` is assumed (default command).

## Commands

### `deploy` (Default Command)

Deploy a new OP Stack L2 network.

```bash
kupcake deploy [OPTIONS]
kupcake [OPTIONS]  # deploy is implicit
```

### `cleanup`

Clean up containers and network by prefix.

```bash
kupcake cleanup <PREFIX>
```

**Arguments**:
- `<PREFIX>` - Network name prefix to clean up

**Behavior**:
- Stops all containers with names starting with `<PREFIX>`
- Removes all stopped containers
- Removes the Docker network `<PREFIX>-network`
- Does **not** delete the data directory

**Example**:
```bash
kupcake cleanup my-network
```

## Global Options

### `-v, --verbosity <LEVEL>`

Set logging verbosity level.

**Values**: `off`, `error`, `warn`, `info`, `debug`, `trace`
**Default**: `info`
**Environment Variable**: `KUP_VERBOSITY`

**Examples**:
```bash
kupcake -v debug
kupcake --verbosity trace
export KUP_VERBOSITY=debug && kupcake
```

## Deploy Command Options

### Network Configuration

#### `-n, --network <NAME>`

Custom name for the network.

**Default**: `kup-<l1-chain>-<l2-chain-id>`
**Environment Variable**: `KUP_NETWORK_NAME`
**Aliases**: `--name`

Determines:
- Container name prefix
- Docker network name (`<NAME>-network`)
- Data directory name (`./data-<NAME>/`)

**Examples**:
```bash
kupcake --network my-testnet
kupcake -n production-l2
export KUP_NETWORK_NAME=my-network && kupcake
```

#### `--l1 <SOURCE>`

L1 chain source - either a chain name or RPC URL.

**Values**:
- `sepolia` - Fork Ethereum Sepolia (chain ID 11155111)
- `mainnet` - Fork Ethereum Mainnet (chain ID 1)
- `https://...` - Custom RPC URL (chain ID detected via `eth_chainId`)
- *(omitted)* - Local mode with random L1 chain ID (no fork)

**Default**: Local mode (no fork)
**Environment Variable**: `KUP_L1`
**Aliases**: `--l1-chain`

**Public RPC Endpoints**:
- Sepolia: `https://ethereum-sepolia-rpc.publicnode.com`
- Mainnet: `https://ethereum-rpc.publicnode.com`

**Examples**:
```bash
kupcake --l1 sepolia
kupcake --l1 mainnet
kupcake --l1 https://eth-mainnet.g.alchemy.com/v2/YOUR-KEY
kupcake  # Local mode, no L1 fork
```

See: [L1 Sources Guide](l1-sources.md)

#### `--l2-chain <CHAIN>`

L2 chain identifier - either a known chain name or numeric chain ID.

**Values**:
- `op-sepolia` → Chain ID 11155420
- `op-mainnet` → Chain ID 10
- `base-sepolia` → Chain ID 84532
- `base-mainnet` → Chain ID 8453
- `<number>` - Custom chain ID (e.g., `42069`)
- *(omitted)* - Random chain ID generated

**Default**: Random chain ID
**Environment Variable**: `KUP_L2_CHAIN`
**Aliases**: `--l2`

**Examples**:
```bash
kupcake --l2-chain op-sepolia
kupcake --l2-chain 42069
kupcake  # Random chain ID
```

### Deployment Behavior

#### `--redeploy`

Force redeployment of all contracts, even if they already exist.

**Default**: `false` (reuse existing contracts if data directory exists)
**Environment Variable**: `KUP_REDEPLOY`

**Use Cases**:
- Reset contract state
- Deploy with updated contract code
- Fix broken deployment

**Example**:
```bash
kupcake --redeploy --config ./data-my-network/Kupcake.toml
```

#### `--outdata <PATH>`

Path to output data directory.

**Default**: `./data-<network-name>/`
**Environment Variable**: `KUP_OUTDATA`
**Aliases**: `--outdata`

**Examples**:
```bash
kupcake --outdata /tmp/kupcake-data
kupcake --outdata ./custom-dir
```

#### `--no-cleanup`

Skip cleanup of Docker containers when the program exits.

**Default**: `false` (cleanup containers on exit)
**Environment Variable**: `KUP_NO_CLEANUP`

**Behavior**:
- Containers keep running after Ctrl+C
- Network remains active
- Useful for debugging or keeping network alive

**Example**:
```bash
kupcake --no-cleanup
# Press Ctrl+C - containers keep running
docker ps  # See running containers
```

#### `--detach`

Run in detached mode - deploy and exit, leaving containers running.

**Default**: `false` (run in foreground)
**Environment Variable**: `KUP_DETACH`

**Behavior**:
- Deploy all services
- Exit immediately
- Containers continue running in background

**Example**:
```bash
kupcake --detach
# Returns to prompt immediately
docker ps  # Verify containers running
```

Use `kupcake cleanup <network-name>` to stop later.

#### `--publish-all-ports`

Publish all exposed container ports to random host ports.

**Default**: `false` (use fixed port mappings)
**Environment Variable**: `KUP_PUBLISH_ALL_PORTS`

**Behavior**:
- Equivalent to `docker run -P`
- Docker assigns random available ports
- Useful to avoid port conflicts
- Check assigned ports with `docker ps`

**Example**:
```bash
kupcake --publish-all-ports
docker ps  # See actual port mappings
```

### Chain Configuration

#### `--block-time <SECONDS>`

Block time in seconds for both L1 (Anvil) and L2 derivation.

**Default**: `12` (Ethereum mainnet block time)
**Environment Variable**: `KUP_BLOCK_TIME`

**Affects**:
- Anvil L1 block production interval
- kona-node `l1_slot_duration` parameter

**Examples**:
```bash
kupcake --block-time 2   # Fast blocks (2s)
kupcake --block-time 12  # Mainnet-like (12s)
```

#### `--l2-nodes <COUNT>`

Total number of L2 nodes to deploy.

**Default**: `5`
**Environment Variable**: `KUP_L2_NODES`
**Aliases**: `--nodes`

This is the total of sequencers + validators.

**Formula**: `validators = l2_nodes - sequencer_count`

**Examples**:
```bash
kupcake --l2-nodes 3 --sequencer-count 1  # 1 seq + 2 val
kupcake --l2-nodes 10 --sequencer-count 3 # 3 seq + 7 val
```

#### `--sequencer-count <COUNT>`

Number of sequencer nodes to deploy.

**Default**: `2`
**Environment Variable**: `KUP_SEQUENCERS`
**Aliases**: `--sequencers`

**Constraints**:
- Must be at least `1`
- Must be at most `l2_nodes`

**Behavior**:
- If `> 1`: op-conductor is deployed for coordination
- If `= 1`: op-conductor is **not** deployed (single sequencer mode)

**Examples**:
```bash
kupcake --sequencer-count 1  # Single sequencer (no conductor)
kupcake --sequencer-count 3  # Multi-sequencer with conductor
```

See: [Multi-Sequencer Guide](multi-sequencer.md)

### Configuration File

#### `--config <PATH>`

Path to an existing `Kupcake.toml` configuration file.

**Environment Variable**: `KUP_CONFIG`
**Aliases**: `--conf`

**Behavior**:
- Load saved configuration instead of generating from CLI args
- CLI args can override config file values
- Useful for repeatable deployments

**Example**:
```bash
kupcake --config ./data-my-network/Kupcake.toml
kupcake --config ./saved-config.toml --redeploy
```

See: [Configuration File Guide](configuration-file.md)

## Docker Image Overrides

Override default Docker images for any service.

**Format**: `--<service>-image <IMAGE>` and `--<service>-tag <TAG>`

### Anvil (L1)

```bash
--anvil-image <IMAGE>   # Default: ghcr.io/foundry-rs/foundry
--anvil-tag <TAG>       # Default: latest
```

**Environment Variables**: `KUP_ANVIL_IMAGE`, `KUP_ANVIL_TAG`

### op-reth (L2 Execution)

```bash
--op-reth-image <IMAGE> # Default: ghcr.io/op-rs/op-reth
--op-reth-tag <TAG>     # Default: latest
```

**Environment Variables**: `KUP_OP_RETH_IMAGE`, `KUP_OP_RETH_TAG`

### kona-node (L2 Consensus)

```bash
--kona-node-image <IMAGE> # Default: ghcr.io/op-rs/kona
--kona-node-tag <TAG>     # Default: latest
```

**Environment Variables**: `KUP_KONA_NODE_IMAGE`, `KUP_KONA_NODE_TAG`

### op-batcher

```bash
--op-batcher-image <IMAGE> # Default: ghcr.io/ethereum-optimism/op-batcher
--op-batcher-tag <TAG>     # Default: latest
```

**Environment Variables**: `KUP_OP_BATCHER_IMAGE`, `KUP_OP_BATCHER_TAG`

### op-proposer

```bash
--op-proposer-image <IMAGE> # Default: ghcr.io/ethereum-optimism/op-proposer
--op-proposer-tag <TAG>     # Default: latest
```

**Environment Variables**: `KUP_OP_PROPOSER_IMAGE`, `KUP_OP_PROPOSER_TAG`

### op-challenger

```bash
--op-challenger-image <IMAGE> # Default: ghcr.io/ethereum-optimism/op-challenger
--op-challenger-tag <TAG>     # Default: latest
```

**Environment Variables**: `KUP_OP_CHALLENGER_IMAGE`, `KUP_OP_CHALLENGER_TAG`

### op-conductor

```bash
--op-conductor-image <IMAGE> # Default: ghcr.io/ethereum-optimism/op-conductor
--op-conductor-tag <TAG>     # Default: latest
```

**Environment Variables**: `KUP_OP_CONDUCTOR_IMAGE`, `KUP_OP_CONDUCTOR_TAG`

### op-deployer

```bash
--op-deployer-image <IMAGE> # Default: ghcr.io/ethereum-optimism/op-deployer
--op-deployer-tag <TAG>     # Default: latest
```

**Environment Variables**: `KUP_OP_DEPLOYER_IMAGE`, `KUP_OP_DEPLOYER_TAG`

### Prometheus

```bash
--prometheus-image <IMAGE> # Default: prom/prometheus
--prometheus-tag <TAG>     # Default: latest
```

**Environment Variables**: `KUP_PROMETHEUS_IMAGE`, `KUP_PROMETHEUS_TAG`

### Grafana

```bash
--grafana-image <IMAGE> # Default: grafana/grafana
--grafana-tag <TAG>     # Default: latest
```

**Environment Variables**: `KUP_GRAFANA_IMAGE`, `KUP_GRAFANA_TAG`

### Examples

```bash
# Use specific op-reth version
kupcake --op-reth-tag v1.0.0

# Use custom registry
kupcake --op-reth-image myregistry.io/op-reth --op-reth-tag custom

# Override multiple images
kupcake \
  --op-reth-tag v1.0.0 \
  --kona-node-tag v0.5.0 \
  --anvil-tag nightly
```

See: [Docker Images Guide](docker-images.md)

## Local Binary Deployment

Deploy services from local binaries instead of Docker images.

**Format**: `--<service>-binary <PATH>`

When a binary path is provided, Kupcake creates a lightweight Docker image from the binary using `debian:trixie-slim` as the base. The image is cached based on the binary's SHA256 hash.

### op-reth

```bash
--op-reth-binary <PATH>
```

**Environment Variable**: `KUP_OP_RETH_BINARY`

Deploy op-reth from a local binary:

```bash
kupcake --op-reth-binary ./op-reth/target/release/op-reth
```

### kona-node

```bash
--kona-node-binary <PATH>
```

**Environment Variable**: `KUP_KONA_NODE_BINARY`

Deploy kona-node from a local binary:

```bash
kupcake --kona-node-binary ./kona/target/release/kona-node
```

### op-batcher

```bash
--op-batcher-binary <PATH>
```

**Environment Variable**: `KUP_OP_BATCHER_BINARY`

Deploy op-batcher from a local binary:

```bash
kupcake --op-batcher-binary ./optimism/op-batcher/bin/op-batcher
```

### op-proposer

```bash
--op-proposer-binary <PATH>
```

**Environment Variable**: `KUP_OP_PROPOSER_BINARY`

Deploy op-proposer from a local binary:

```bash
kupcake --op-proposer-binary ./optimism/op-proposer/bin/op-proposer
```

### op-challenger

```bash
--op-challenger-binary <PATH>
```

**Environment Variable**: `KUP_OP_CHALLENGER_BINARY`

Deploy op-challenger from a local binary:

```bash
kupcake --op-challenger-binary ./optimism/op-challenger/bin/op-challenger
```

### op-conductor

```bash
--op-conductor-binary <PATH>
```

**Environment Variable**: `KUP_OP_CONDUCTOR_BINARY`

Deploy op-conductor from a local binary:

```bash
kupcake --op-conductor-binary ./optimism/op-conductor/bin/op-conductor
```

### Examples

Deploy with single local binary:

```bash
kupcake --kona-node-binary ./kona/target/release/kona-node
```

Deploy with multiple local binaries:

```bash
kupcake \
  --op-reth-binary ./op-reth/target/release/op-reth \
  --kona-node-binary ./kona/target/release/kona-node \
  --op-batcher-binary ./optimism/op-batcher/bin/op-batcher
```

Mix local binaries with Docker images:

```bash
kupcake \
  --kona-node-binary ./kona/target/release/kona-node \
  --op-reth-tag v1.0.0 \
  --op-batcher-tag latest
```

**Binary Requirements**:
- Must be compiled for Linux (the Docker container OS)
- Must be compatible with GLIBC 2.38 or earlier
- Must be executable (`chmod +x`)

**See**: [Docker Images Guide - Local Binary Deployment](docker-images.md#local-binary-deployment)

## Environment Variables

All CLI options can be set via environment variables with the `KUP_` prefix:

```bash
export KUP_NETWORK_NAME=my-network
export KUP_L1=sepolia
export KUP_L2_CHAIN=42069
export KUP_BLOCK_TIME=2
export KUP_VERBOSITY=debug

kupcake  # Uses environment variables
```

**Precedence** (highest to lowest):
1. CLI arguments
2. Environment variables
3. Config file (if `--config` specified)
4. Defaults

## Common Usage Patterns

### Minimal Deployment (Local Mode)

```bash
kupcake
```

### Sepolia Fork with Custom Chain ID

```bash
kupcake --l1 sepolia --l2-chain 42069
```

### Single Sequencer (No Conductor)

```bash
kupcake --sequencer-count 1 --l2-nodes 3
```

### Multi-Sequencer with High Availability

```bash
kupcake --sequencer-count 3 --l2-nodes 7
```

### Fast Block Times for Testing

```bash
kupcake --block-time 1
```

### Detached Mode for CI/CD

```bash
kupcake --detach --network ci-test
# Run tests...
kupcake cleanup ci-test
```

### Keep Running for Debugging

```bash
kupcake --no-cleanup -v debug
# Ctrl+C - containers keep running
docker logs <network>-anvil
```

### Custom Images for Development

```bash
kupcake \
  --op-reth-image localhost:5000/op-reth \
  --op-reth-tag dev \
  --kona-node-image localhost:5000/kona \
  --kona-node-tag dev
```

### Load and Modify Existing Config

```bash
kupcake --config ./data-my-network/Kupcake.toml --block-time 2
```

## Exit Codes

- `0` - Success
- `1` - Error (deployment failed, invalid arguments, etc.)

## Related Documentation

- [Environment Variables Guide](environment-variables.md) - Detailed environment variable reference
- [Configuration File Guide](configuration-file.md) - Kupcake.toml structure
- [Docker Images Guide](docker-images.md) - Custom image usage
- [Multi-Sequencer Guide](multi-sequencer.md) - Multi-sequencer setup
- [Troubleshooting](troubleshooting.md) - Common issues

## See Also

```bash
kupcake --help         # Built-in help
kupcake deploy --help  # Deploy command help
kupcake cleanup --help # Cleanup command help
```
