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

### `health`

Check the health of a deployed network.

```bash
kupcake health <CONFIG>
```

**Arguments**:
- `<CONFIG>` - Network name or path to `Kupcake.toml` / outdata directory

**Behavior**:
- Loads the `Kupcake.toml` configuration
- Verifies all expected containers are running via Docker
- Queries L1 and L2 RPC endpoints to check chain IDs match the config
- Queries kona-node `optimism_syncStatus` for each consensus client
- Exits with code `0` if healthy, `1` if unhealthy

**Examples**:
```bash
# By network name (loads ./data-kup-nutty-songs/Kupcake.toml)
kupcake health kup-nutty-songs

# By directory path
kupcake health ./data-kup-nutty-songs/

# By config file path
kupcake health ./data-kup-nutty-songs/Kupcake.toml
```

### `faucet`

Send ETH to an L2 address via the OptimismPortal deposit mechanism.

```bash
kupcake faucet <CONFIG> --to <ADDRESS> [--amount <ETH>] [--wait]
```

**Arguments**:
- `<CONFIG>` - Network name or path to `Kupcake.toml` / outdata directory

**Options**:
- `--to <ADDRESS>` - L2 recipient address (0x-prefixed, 40 hex chars) **(required)**
- `--amount <ETH>` - Amount of ETH to send (default: `1.0`)
- `--wait` - Wait for the deposit to appear on L2 before returning

**Behavior**:
- Loads the `Kupcake.toml` configuration
- Reads the deployer account (index 0) from `anvil.json`
- Reads the `OptimismPortalProxy` address from `state.json`
- Calls `depositTransaction` on the portal via `eth_sendTransaction` (Anvil auto-signs)
- Optionally polls the L2 sequencer's `eth_getBalance` until the balance increases

**Examples**:
```bash
# Send 1 ETH (default) to an address
kupcake faucet kup-nutty-songs --to 0x70997970C51812dc3A010C7d01b50e0d17dc79C8

# Send 10 ETH and wait for it to appear on L2
kupcake faucet kup-nutty-songs --to 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 --amount 10 --wait

# Using a config file path
kupcake faucet ./data-kup-nutty-songs/Kupcake.toml --to 0xdead...beef --amount 0.5
```

### `spam`

Generate continuous L2 traffic using Flashbots Contender.

```bash
kupcake spam <CONFIG> [OPTIONS] [-- <EXTRA_ARGS>...]
```

**Arguments**:
- `<CONFIG>` - Network name or path to `Kupcake.toml` / outdata directory

**Options**:
- `--scenario <NAME|PATH>` - Scenario to run (default: `transfers`)
- `--tps <N>` - Transactions per second (default: `10`)
- `--duration <SECS>` - Duration in seconds (default: `30`, ignored with `--forever`)
- `--forever` - Run indefinitely until Ctrl+C
- `-a, --accounts <N>` - Number of spammer accounts (default: `10`)
- `--min-balance <ETH>` - Minimum balance for spammer accounts (default: `0.1`)
- `--fund-amount <ETH>` - ETH to fund the funder account on L2 (default: `100.0`)
- `--funder-account-index <N>` - Anvil account index for funding (default: `10`)
- `--report` - Generate a report after completion
- `--contender-image <IMAGE>` - Docker image for Contender (default: `flashbots/contender`, env: `KUP_CONTENDER_IMAGE`)
- `--contender-tag <TAG>` - Docker tag for Contender (default: `latest`, env: `KUP_CONTENDER_TAG`)
- `--target-node <N>` - Target sequencer index (default: `0`)

**Built-in Scenarios**:
- `transfers` - Simple ETH transfers between accounts
- `erc20` - ERC-20 token transfers
- `uni_v2` - Uniswap V2 swaps

**Behavior**:
- Loads the `Kupcake.toml` configuration
- Funds the funder account on L2 via the OptimismPortal deposit (faucet)
- Starts a Contender Docker container on the kupcake Docker network
- Streams Contender logs to stdout in real-time
- Cleans up the container on completion or Ctrl+C

**Examples**:
```bash
# Run basic ETH transfers at 10 TPS for 30 seconds (defaults)
kupcake spam kup-nutty-songs

# Run ERC-20 transfers at 100 TPS for 60 seconds
kupcake spam kup-nutty-songs --scenario erc20 --tps 100 --duration 60

# Run indefinitely until Ctrl+C
kupcake spam kup-nutty-songs --scenario transfers --tps 50 --forever

# Use a custom scenario file
kupcake spam kup-nutty-songs --scenario ./my-scenario.toml

# Target a specific sequencer and generate a report
kupcake spam kup-nutty-songs --target-node 1 --report

# Pass extra arguments to contender
kupcake spam kup-nutty-songs -- --verbose --seed 42
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

Force redeployment of all contracts, bypassing configuration hash checks.

**Default**: `false` (skip redeployment if configuration unchanged)
**Environment Variable**: `KUP_REDEPLOY`

**Behavior**:

By default, Kupcake computes a hash of deployment-relevant parameters (L1/L2 chain IDs, fork URL, etc.) and skips contract deployment if the configuration hasn't changed. This saves 30-60 seconds on subsequent runs.

The `--redeploy` flag bypasses this optimization and always redeploys contracts, even if the configuration is identical.

**When deployment is automatically skipped**:
- Configuration hash matches saved hash
- All deployment files exist (genesis.json, rollup.json, state.json)

**When deployment is automatically triggered**:
- Configuration hash changed (e.g., different L2 chain ID)
- Deployment version file missing or corrupted
- Data directory doesn't exist

**Use Cases**:
- Reset contract state
- Deploy with updated contract code
- Fix broken deployment
- Override automatic hash checking

**Example**:
```bash
# Redeploy even if configuration unchanged
kupcake --redeploy --config ./data-my-network/Kupcake.toml

# First run creates deployment version
kupcake --l2-chain 42069

# Second run skips deployment (config unchanged)
kupcake --l2-chain 42069

# Change triggers redeployment
kupcake --l2-chain 12345
```

#### `--snapshot <PATH>`

Restore the L2 network from an existing op-reth database snapshot instead of deploying contracts from scratch.

**Environment Variable**: `KUP_SNAPSHOT`

**Cannot be combined with**: `--redeploy`

**Requires**: `--l1` (fork mode must be set)

**Snapshot Directory Structure**:
```
snapshot-dir/
  rollup.json       # Required - rollup config for kona-node
  intent.toml       # Optional - generated via op-deployer if missing
  <reth-db-dir>/    # Required - the op-reth database (first subdirectory)
```

**Behavior**:
- Starts Anvil in fork mode against the specified L1
- Skips contract deployment (contracts already exist on the forked L1)
- Generates `genesis.json` via `op-deployer inspect genesis`
- Copies `rollup.json` from the snapshot directory
- Symlinks the reth database for the primary sequencer (use `--copy-snapshot` for a full copy)
- Only the primary sequencer is restored from the snapshot; validators sync via P2P
- `op-proposer` and `op-challenger` are skipped (no `state.json` available)

**Examples**:
```bash
# Restore from a snapshot directory
kupcake --l1 sepolia --snapshot /path/to/snapshot

# Restore with a full copy of the reth database
kupcake --l1 sepolia --snapshot /path/to/snapshot --copy-snapshot

# Combine with other options
kupcake --l1 sepolia --snapshot ./my-snapshot --no-cleanup --detach
```

#### `--copy-snapshot`

Copy the snapshot reth database instead of symlinking it.

**Default**: `false` (symlink)
**Environment Variable**: `KUP_COPY_SNAPSHOT`
**Requires**: `--snapshot`

By default, `--snapshot` creates a symlink to the original reth database to avoid duplicating potentially large databases (many GB). Use `--copy-snapshot` when you need an independent copy.

**Example**:
```bash
kupcake --l1 sepolia --snapshot /path/to/snapshot --copy-snapshot
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

#### `--spam [PRESET]`

Deploy and immediately start spamming with a named preset.

**Default**: Not enabled. When flag is present without a value, defaults to `light`.
**Environment Variable**: `KUP_SPAM`

**Cannot be combined with**: `--detach`

**Available Presets**:

| Preset    | Scenario    | TPS  | Accounts | Description                    |
|-----------|-------------|------|----------|--------------------------------|
| `light`   | transfers   | 10   | 5        | Light ETH transfer traffic     |
| `medium`  | transfers   | 50   | 20       | Moderate ETH transfer traffic  |
| `heavy`   | transfers   | 200  | 50       | Heavy ETH transfer traffic     |
| `erc20`   | erc20       | 50   | 20       | ERC-20 token transfers         |
| `uniswap` | uni_v2      | 20   | 10       | Uniswap V2 swap traffic        |
| `stress`  | transfers   | 500  | 100      | Stress test with high TPS      |

All presets run indefinitely until Ctrl+C.

**Behavior**:
- Deploys the full OP Stack network
- Funds a spammer account on L2 via the faucet
- Starts Contender with the preset configuration
- Ctrl+C stops both spam and the network (unless `--no-cleanup` is set)

**Examples**:
```bash
# Deploy + light spam (default preset)
kupcake --spam

# Deploy + heavy workload
kupcake --spam heavy

# Deploy + DeFi workload
kupcake --spam uniswap

# Deploy + spam, keep containers running after Ctrl+C
kupcake --spam heavy --no-cleanup
```

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

#### `--genesis-timestamp <UNIX_TIMESTAMP>`

Manually specify the L2 genesis timestamp (Unix timestamp in seconds).

**Default**: Automatically calculated
**Environment Variable**: `KUP_GENESIS_TIMESTAMP`

**Automatic Calculation**:
- When forking L1: `latest_block_timestamp - (block_time * block_number)`
- In local mode: Current Unix timestamp

This option overrides the automatic calculation and sets an explicit genesis timestamp.

**Use Cases**:
- Testing with specific timestamps
- Aligning genesis with external events
- Reproducing specific blockchain states
- Deterministic deployments for CI/CD

**Examples**:
```bash
# Use a specific timestamp (January 19, 2026 12:00:00 UTC)
kupcake --genesis-timestamp 1768464000

# Combine with L1 fork and custom timestamp
kupcake --l1 sepolia --genesis-timestamp 1768464000

# Local mode with custom timestamp
kupcake --genesis-timestamp 1768464000
```

**Notes**:
- The timestamp is included in the deployment configuration hash
- Changing the timestamp will trigger contract redeployment
- The timestamp should be reasonable for the target L1 chain

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

#### `--flashblocks`

Enable flashblocks support.

**Default**: `false`
**Environment Variable**: `KUP_FLASHBLOCKS`

**Behavior**:
- Sequencer nodes use op-rbuilder (a fork of op-reth with flashblocks capabilities) instead of op-reth
- Validator nodes continue using op-reth
- Kona-node's built-in flashblocks relay connects the sequencer's op-rbuilder to validator nodes

**Data Flow**:
```
Sequencer:
  op-rbuilder (flashblocks WS on port 1111)
       ↓
  sequencer kona-node (relay on port 1112)
       ↓
  validator kona-node (subscribes to relay)
       ↓
  validator op-reth (unchanged)
```

**Examples**:
```bash
kupcake --flashblocks
kupcake --flashblocks --l2-nodes 3 --sequencer-count 1
kupcake --flashblocks --op-rbuilder-tag v0.4.0
```

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
--op-reth-image <IMAGE> # Default: us-docker.pkg.dev/oplabs-tools-artifacts/images/op-reth
--op-reth-tag <TAG>     # Default: develop
```

**Environment Variables**: `KUP_OP_RETH_IMAGE`, `KUP_OP_RETH_TAG`

### kona-node (L2 Consensus)

```bash
--kona-node-image <IMAGE> # Default: us-docker.pkg.dev/oplabs-tools-artifacts/images/kona-node
--kona-node-tag <TAG>     # Default: develop
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

### op-rbuilder (Flashblocks Execution)

```bash
--op-rbuilder-image <IMAGE> # Default: ghcr.io/flashbots/op-rbuilder
--op-rbuilder-tag <TAG>     # Default: v0.3.2-rc3
```

**Environment Variables**: `KUP_OP_RBUILDER_IMAGE`, `KUP_OP_RBUILDER_TAG`

Used when `--flashblocks` is enabled. Replaces op-reth for sequencer nodes only.

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

Deploy services from local binaries or source directories instead of Docker images.

**Format**: `--<service>-binary <PATH>`

The `<PATH>` can be either:
- **A file path** (pre-built binary): Must be a Linux ELF executable. Kupcake validates the binary format and creates a lightweight Docker image from it.
- **A directory path** (Rust source): Must contain a `Cargo.toml`. Kupcake runs `cargo build --release --bin <service>` automatically. On macOS, it detects Docker's platform and cross-compiles for the correct Linux target.

Built images are cached based on the binary's SHA256 hash.

### op-reth

```bash
--op-reth-binary <PATH>
```

**Environment Variable**: `KUP_OP_RETH_BINARY`

Deploy op-reth from a local binary or source directory:

```bash
# From pre-built binary
kupcake --op-reth-binary ./op-reth/target/release/op-reth

# From source directory (auto-builds, cross-compiles on macOS)
kupcake --op-reth-binary ./op-reth
```

### kona-node

```bash
--kona-node-binary <PATH>
```

**Environment Variable**: `KUP_KONA_NODE_BINARY`

Deploy kona-node from a local binary or source directory:

```bash
# From pre-built binary
kupcake --kona-node-binary ./kona/target/release/kona-node

# From source directory (auto-builds, cross-compiles on macOS)
kupcake --kona-node-binary ./kona
```

### op-batcher

```bash
--op-batcher-binary <PATH>
```

**Environment Variable**: `KUP_OP_BATCHER_BINARY`

Deploy op-batcher from a local binary or source directory:

```bash
kupcake --op-batcher-binary ./optimism/op-batcher/bin/op-batcher
```

### op-proposer

```bash
--op-proposer-binary <PATH>
```

**Environment Variable**: `KUP_OP_PROPOSER_BINARY`

Deploy op-proposer from a local binary or source directory:

```bash
kupcake --op-proposer-binary ./optimism/op-proposer/bin/op-proposer
```

### op-challenger

```bash
--op-challenger-binary <PATH>
```

**Environment Variable**: `KUP_OP_CHALLENGER_BINARY`

Deploy op-challenger from a local binary or source directory:

```bash
kupcake --op-challenger-binary ./optimism/op-challenger/bin/op-challenger
```

### op-conductor

```bash
--op-conductor-binary <PATH>
```

**Environment Variable**: `KUP_OP_CONDUCTOR_BINARY`

Deploy op-conductor from a local binary or source directory:

```bash
kupcake --op-conductor-binary ./optimism/op-conductor/bin/op-conductor
```

### op-rbuilder

```bash
--op-rbuilder-binary <PATH>
```

**Environment Variable**: `KUP_OP_RBUILDER_BINARY`

Deploy op-rbuilder from a local binary or source directory (used when `--flashblocks` is enabled):

```bash
kupcake --flashblocks --op-rbuilder-binary ./op-rbuilder/target/release/op-rbuilder
```

### Examples

Build from source directory (recommended on macOS):

```bash
kupcake --kona-node-binary ./kona
```

Deploy with a pre-built Linux binary:

```bash
kupcake --kona-node-binary ./kona/target/release/kona-node
```

Deploy with multiple local binaries:

```bash
kupcake \
  --op-reth-binary ./op-reth \
  --kona-node-binary ./kona \
  --op-batcher-binary ./optimism/op-batcher/bin/op-batcher
```

Mix local binaries with Docker images:

```bash
kupcake \
  --kona-node-binary ./kona \
  --op-reth-tag v1.0.0 \
  --op-batcher-tag latest
```

**Binary Requirements** (for pre-built binaries):
- Must be a Linux ELF executable (macOS Mach-O binaries are rejected with a helpful error)
- Must be compatible with GLIBC 2.38 or earlier
- Must be executable (`chmod +x`)

**Source Directory Requirements** (for build-from-source):
- Must contain a `Cargo.toml`
- On macOS, requires a one-time toolchain setup: `rustup target add aarch64-unknown-linux-gnu` and `brew install messense/macos-cross-toolchains/aarch64-unknown-linux-gnu` (see [Docker Images Guide - macOS Cross-Compilation Setup](docker-images.md#macos-cross-compilation-setup))

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

### Deploy with Traffic Generation

```bash
kupcake --spam              # Light spam (default)
kupcake --spam heavy        # Heavy workload
kupcake --spam uniswap      # DeFi workload
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

### Restore from Snapshot

```bash
# Symlink reth database (fast, default)
kupcake --l1 sepolia --snapshot /path/to/snapshot

# Copy reth database (independent copy)
kupcake --l1 sepolia --snapshot /path/to/snapshot --copy-snapshot
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
