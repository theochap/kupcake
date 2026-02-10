# Querying and Interacting with the Network

**Target Audience**: Operators, Developers

This guide covers how to query your Kupcake devnet, send transactions, and verify that the network is healthy.

## Prerequisites

- A running Kupcake deployment (see [Quickstart](../getting-started/quickstart.md))
- [Foundry](https://getfoundry.sh/) installed (`cast` CLI)
- Optionally: `jq` for JSON formatting

## Finding Your Endpoints

Kupcake randomly generates network names, chain IDs, and port assignments unless you explicitly set them. After deployment, the startup output prints all available endpoints:

```
=== Host-accessible endpoints (curl from your terminal) ===
L1 (Anvil) RPC:       http://localhost:61428/
L2 (op-batcher) RPC:  http://localhost:61447/
Grafana:              http://localhost:61465/

=== Internal Docker network endpoints ===
L1 (Anvil) RPC:       http://kup-nutty-songs-anvil:8545/
L2 sequencer (op-reth) HTTP:    http://kup-nutty-songs-op-reth:9545/
...
```

You can also discover endpoints from running containers:

```bash
# List all containers with their port mappings
docker ps --filter "name=kup-" --format "table {{.Names}}\t{{.Ports}}"

# Get ports for a specific container
docker port <container-name>
```

The saved configuration file at `{outdata}/Kupcake.toml` contains the chain IDs and network name for your deployment.

Throughout this guide, replace the placeholder variables with your actual values from the deployment output:

- `$L1_RPC` — Anvil (L1) host URL
- `$L2_RPC` — op-reth (L2 sequencer) host URL
- `$KONA_RPC` — kona-node (consensus) host URL

## Anvil Test Accounts

Anvil generates test accounts on startup. Account addresses and private keys are saved to `{outdata}/anvil/anvil.json`. The accounts are assigned specific roles in the OP Stack:

| Account Index | Role |
|---------------|------|
| 0 | Admin / Deployer |
| 1 | Batcher |
| 2 | Proposer |
| 3 | Challenger |

To extract the first account's private key:

```bash
jq -r '.private_keys[0]' {outdata}/anvil/anvil.json
```

## Querying the L1 (Anvil)

Anvil provides a standard Ethereum JSON-RPC interface.

```bash
# Get the current L1 block number
cast block-number --rpc-url $L1_RPC

# Get L1 chain ID
cast chain-id --rpc-url $L1_RPC

# Check an account balance (replace with an address from anvil.json)
cast balance <ADDRESS> --rpc-url $L1_RPC --ether

# Get the latest block
cast block latest --rpc-url $L1_RPC

# Send ETH on L1 (replace with a private key from anvil.json)
cast send --rpc-url $L1_RPC \
  --private-key <PRIVATE_KEY> \
  --value 1ether \
  <RECIPIENT_ADDRESS>
```

## Querying the L2 (op-reth)

op-reth exposes standard Ethereum JSON-RPC on the L2 chain.

```bash
# Get L2 chain ID
cast chain-id --rpc-url $L2_RPC

# Get the latest L2 block number
cast block-number --rpc-url $L2_RPC

# Get an L2 block with full details
cast block latest --rpc-url $L2_RPC

# Get L2 gas price
cast gas-price --rpc-url $L2_RPC
```

## Querying the Consensus Layer (kona-node)

kona-node provides OP Stack-specific RPC methods for rollup status.

### Sync Status

The most important health check — shows where each safety level is at:

```bash
cast rpc optimism_syncStatus --rpc-url $KONA_RPC | jq
```

Key fields in the response:
- **`unsafe_l2`**: Latest L2 block from the sequencer (tip of the chain)
- **`safe_l2`**: L2 blocks derived from submitted L1 batches
- **`finalized_l2`**: L2 blocks backed by finalized L1 data
- **`head_l1`**: Latest L1 block the node is aware of

A healthy network shows all of these advancing over time, with `unsafe_l2` leading and `finalized_l2` trailing.

### Rollup Configuration

```bash
cast rpc optimism_rollupConfig --rpc-url $KONA_RPC | jq
```

Returns the full rollup configuration including chain IDs, genesis info, fork activation times, and contract addresses.

### Output at Block

```bash
# Get the output root at a specific L2 block number
cast rpc optimism_outputAtBlock 0x10 --rpc-url $KONA_RPC | jq
```

## Sending L2 Transactions

In local mode (no L1 forking), L2 accounts start with no balance. You can fund them via op-reth's dev RPC:

```bash
# Fund an L2 account (100 ETH in hex wei)
cast rpc anvil_setBalance <ADDRESS> 0x56BC75E2D63100000 --rpc-url $L2_RPC

# Send a simple ETH transfer on L2 (use private key from anvil.json)
cast send --rpc-url $L2_RPC \
  --private-key <PRIVATE_KEY> \
  --value 0.1ether \
  <RECIPIENT_ADDRESS>
```

## Checking Network Health

A healthy Kupcake network has:
1. **L1 blocks advancing** — Anvil mining at the configured `block_time`
2. **L2 blocks advancing** — `unsafe_l2` number increasing
3. **Batches being submitted** — `safe_l2` following `unsafe_l2`
4. **L1 data finalizing** — `finalized_l2` advancing

### Built-in Health Check

The easiest way to check network health is the `kupcake health` command, which verifies containers, chain IDs, and block production in one step:

```bash
# By network name
kupcake health kup-nutty-songs

# By config path
kupcake health ./data-kup-nutty-songs/
```

The command exits with code `0` if healthy, `1` if unhealthy, making it suitable for CI/CD scripts.

### Manual Health Check

```bash
# 1. Check L1 is producing blocks
cast block-number --rpc-url $L1_RPC

# 2. Check L2 is producing blocks
cast block-number --rpc-url $L2_RPC

# 3. Check sync status — the core health indicator
cast rpc optimism_syncStatus --rpc-url $KONA_RPC | jq '{
  l1_head: .head_l1.number,
  l2_unsafe: .unsafe_l2.number,
  l2_safe: .safe_l2.number,
  l2_finalized: .finalized_l2.number
}'
```

### What to Look For

| Check | Healthy | Unhealthy |
|-------|---------|-----------|
| L1 blocks | Increasing every `block_time` seconds | Stuck at same number |
| L2 unsafe blocks | Increasing (faster than L1) | Not advancing |
| L2 safe blocks | Trailing unsafe by a few blocks | Stuck at 0 or not advancing |
| L2 finalized blocks | Trailing safe, advancing steadily | Stuck at 0 |
| Safe-to-unsafe gap | Small (< 20 blocks) | Growing continuously |

### Container Health

```bash
# Check all containers are running
docker ps --filter "name=kup-" --format "table {{.Names}}\t{{.Status}}"

# Check container logs for errors (replace <network> with your network name)
docker logs kup-<network>-kona-node --tail 20
docker logs kup-<network>-op-batcher --tail 20
docker logs kup-<network>-op-reth --tail 20
```

## Monitoring with Prometheus and Grafana

### Grafana Dashboards

Open your Grafana host URL in a browser (default credentials: `admin`/`admin`).

### Prometheus Queries

Each service exposes a `/metrics` endpoint for Prometheus scraping:

| Service | Container Metrics Port |
|---------|----------------------|
| kona-node | 7300 |
| op-batcher | 7301 |
| op-proposer | 7302 |
| op-reth | 9001 |

## Related Documentation

- [Port Management](port-management.md) — Port allocation patterns and conflict resolution
- [Monitoring](monitoring.md) — Prometheus and Grafana configuration
- [Troubleshooting](troubleshooting.md) — Common issues and solutions
