# Understanding What Gets Deployed

**Target Audience**: New Users
**Prerequisites**: Completed at least one deployment

This guide explains what Kupcake deploys, where files are stored, and how components interact.

## Deployment Components

### L1 Layer (Anvil)

**Container**: `<network-name>-anvil`
**Purpose**: Local Ethereum L1 fork or standalone chain

When you run Kupcake, Anvil provides:
- EVM-compatible blockchain
- Instant block finality
- Pre-funded test accounts
- Configurable block time (default: 12 seconds)

**Forking Modes**:
- `--l1 sepolia` - Fork Ethereum Sepolia testnet
- `--l1 mainnet` - Fork Ethereum mainnet
- No `--l1` flag - Local mode with random chain ID (no fork)

**Data Location**: `./data-<network-name>/anvil/`
- `anvil.json` - Test account information (addresses, private keys)
- `state.json` - Periodic state snapshots

### L1 Smart Contracts (op-deployer)

**Container**: `<network-name>-op-deployer-init` and `<network-name>-op-deployer-apply`
**Purpose**: Deploy OP Stack contracts to L1

The op-deployer runs in two phases:

1. **Init Phase**: Generates deployment intent and genesis configuration
2. **Apply Phase**: Deploys contracts to L1 using Foundry

**Deployed Contracts**:
- `L1CrossDomainMessenger` - Message passing between L1 and L2
- `L1StandardBridge` - ETH and ERC20 token bridging
- `OptimismPortal` - Main entry point for deposits and withdrawals
- `L2OutputOracle` - Stores L2 state root proposals
- `SystemConfig` - System configuration parameters
- `DisputeGameFactory` - Creates fault proof dispute games
- And many more...

**Data Location**: `./data-<network-name>/l2-stack/`
- `intent.toml` - Deployment intent configuration
- `state.json` - Deployed contract addresses
- `genesis.json` - L2 genesis state
- `rollup.json` - Rollup configuration for consensus clients

### L2 Execution Layer (op-reth)

**Containers**: `<network-name>-op-reth-sequencer-{1,2}` and `<network-name>-op-reth-validator-{1,2,3}`
**Purpose**: EVM execution and state management

op-reth is the Rust implementation of the Ethereum execution client, modified for OP Stack:
- Executes EVM transactions
- Maintains state trie
- Provides JSON-RPC API for wallets and dapps
- Stores blockchain data locally

**Roles**:
- **Sequencers**: Produce and sequence new L2 blocks
- **Validators**: Validate and sync L2 blocks (read-only)

**Default Configuration**:
- 2 sequencers (with op-conductor coordination)
- 3 validators

**Data Location**: `./data-<network-name>/l2-stack/reth-data-{container-name}/`
- Each op-reth instance has its own data directory
- Stores blockchain database, state, and receipts

### L2 Consensus Layer (kona-node)

**Containers**: `<network-name>-kona-node-sequencer-{1,2}` and `<network-name>-kona-node-validator-{1,2,3}`
**Purpose**: L1 data derivation and consensus

kona-node implements the OP Stack derivation pipeline:
- Fetches L1 blocks and batches from Anvil
- Derives L2 blocks from L1 data
- Feeds derived blocks to op-reth via Engine API
- Handles reorgs and sequencer coordination

**Key Responsibilities**:
- Monitor L1 for batch submissions
- Derive L2 blocks from L1 data
- Communicate with op-reth using authenticated Engine API (JWT)

**JWT Authentication**: Each kona-node uses a shared JWT secret with its paired op-reth
**Data Location**: `./data-<network-name>/l2-stack/jwt-{container-name}.hex`

### Transaction Batcher (op-batcher)

**Container**: `<network-name>-op-batcher`
**Purpose**: Submit L2 transaction batches to L1

The batcher:
1. Monitors the sequencer for new L2 blocks
2. Compresses and batches L2 transactions
3. Submits batches to L1 as calldata
4. Uses one of Anvil's pre-funded accounts

**Batching Frequency**: Depends on L2 block production rate and batch size thresholds

**Why It Matters**: Without the batcher, L2 data wouldn't be posted to L1 (data availability)

### State Root Proposer (op-proposer)

**Container**: `<network-name>-op-proposer`
**Purpose**: Submit L2 state root proposals to L1

The proposer:
1. Monitors the sequencer for new L2 blocks
2. Calculates state roots
3. Submits proposals to the `L2OutputOracle` contract on L1
4. Submits approximately every 10-20 L2 blocks (configurable)

**Why It Matters**: L1 needs L2 state roots to process withdrawals and verify state

### Fault Proof Challenger (op-challenger)

**Container**: `<network-name>-op-challenger`
**Purpose**: Challenge invalid state root proposals

The challenger:
- Monitors state root proposals on L1
- Validates proposals by re-deriving L2 state
- Initiates dispute games if it detects an invalid proposal

**In Production**: Ensures malicious sequencers can't submit fraudulent state roots

**In Local Testing**: Usually doesn't need to challenge (sequencer is honest)

### Multi-Sequencer Coordinator (op-conductor)

**Container**: `<network-name>-op-conductor` (only when `--sequencer-count > 1`)
**Purpose**: Coordinate multiple sequencers using Raft consensus

When you deploy multiple sequencers, op-conductor:
- Elects a leader sequencer using Raft
- Ensures only one sequencer is active at a time
- Handles leader failover automatically
- Prevents conflicting L2 blocks

**Raft Cluster**:
- Sequencer 1 (index 0): Initial leader, starts active
- Sequencers 2+ : Start in stopped state, wait for conductor

**Why It Matters**: Enables high-availability sequencer setups

### Monitoring (Prometheus + Grafana)

**Containers**: `<network-name>-prometheus` and `<network-name>-grafana`
**Purpose**: Metrics collection and visualization

**Prometheus**:
- Scrapes metrics from all services every 15 seconds
- Stores time-series metrics data
- Provides query API

**Grafana**:
- Visualizes metrics from Prometheus
- Pre-configured OP Stack dashboards
- Default credentials: `admin` / `admin`

**Access**:
- Prometheus: http://localhost:9090
- Grafana: http://localhost:3000

**Data Location**: `./data-<network-name>/monitoring/`

## File System Structure

After deployment, your data directory looks like this:

```
./data-<network-name>/
├── Kupcake.toml                          # Saved deployment configuration
│
├── anvil/                                # L1 (Anvil) data
│   ├── anvil.json                        # Test accounts (addresses, private keys)
│   └── state.json                        # L1 state snapshots
│
├── l2-stack/                             # L2 and contract deployment data
│   ├── genesis.json                      # L2 genesis configuration
│   ├── rollup.json                       # Rollup config (used by kona-node)
│   ├── intent.toml                       # op-deployer deployment intent
│   ├── state.json                        # Deployed L1 contract addresses
│   │
│   ├── jwt-<network>-kona-node-sequencer-1.hex   # JWT secrets
│   ├── jwt-<network>-kona-node-sequencer-2.hex
│   ├── jwt-<network>-kona-node-validator-1.hex
│   ├── ...
│   │
│   └── reth-data-<network>-op-reth-sequencer-1/  # op-reth databases
│       reth-data-<network>-op-reth-sequencer-2/
│       reth-data-<network>-op-reth-validator-1/
│       ...
│
└── monitoring/                           # Monitoring data
    ├── prometheus.yml                    # Prometheus scrape configuration
    └── grafana/                          # Grafana dashboards and data
```

## Port Mappings

By default, Kupcake exposes these ports on your host:

| Service | Port | Purpose |
|---------|------|---------|
| Anvil (L1) | 8545 | L1 RPC |
| Sequencer 1 RPC | 9545 | L2 RPC (primary) |
| Sequencer 1 WS | 9546 | L2 WebSocket |
| Sequencer 2 RPC | 9645 | L2 RPC (secondary) |
| Sequencer 2 WS | 9646 | L2 WebSocket |
| Validator 1 RPC | 9745 | L2 RPC (read-only) |
| Prometheus | 9090 | Metrics API |
| Grafana | 3000 | Dashboards |

Additional validators and sequencers increment port numbers.

**Custom Port Publishing**: Use `--publish-all-ports` to publish all container ports to random host ports.

## Docker Networking

All containers run in an isolated Docker network: `<network-name>-network`

**Container-to-Container Communication**:
- Containers use container names as hostnames
- Example: kona-node connects to `<network-name>-anvil:8545` for L1 RPC
- No host port mapping needed for internal communication

**Host-to-Container Communication**:
- Use exposed ports (e.g., `http://localhost:8545` for Anvil)
- Containers are isolated from other Docker networks

## Configuration Persistence

### Kupcake.toml

The deployment configuration is saved to `./data-<network-name>/Kupcake.toml`:

```toml
[deployer]
network_name = "my-network"
l1_chain_id = 11155111
l2_chain_id = 42069
block_time = 12
# ... and all other settings
```

**Reload Configuration**:
```bash
./target/release/kupcake --config ./data-my-network/Kupcake.toml
```

This allows you to:
- Resume a deployment
- Share configurations
- Modify and redeploy (e.g., change Docker images)

## Component Interaction Flow

Here's how the components work together:

```
┌─────────────────────────────────────────────────────────────────┐
│ L1 Layer (Anvil)                                                │
│ - Produces blocks every 12s                                     │
│ - Stores OP Stack contracts                                     │
│ - Receives batches from op-batcher                              │
│ - Receives state proposals from op-proposer                     │
└──────────────────┬──────────────────────────────────────────────┘
                   │
                   │ L1 data (blocks, batches, proposals)
                   ↓
┌─────────────────────────────────────────────────────────────────┐
│ L2 Consensus Layer (kona-node)                                  │
│ - Fetches L1 blocks                                             │
│ - Derives L2 blocks from L1 batches                             │
│ - Sends derived blocks to op-reth via Engine API                │
└──────────────────┬──────────────────────────────────────────────┘
                   │
                   │ Derived L2 blocks (Engine API + JWT)
                   ↓
┌─────────────────────────────────────────────────────────────────┐
│ L2 Execution Layer (op-reth)                                    │
│ - Executes EVM transactions                                     │
│ - Maintains state trie                                          │
│ - Provides JSON-RPC for wallets/dapps                           │
│ - Sequencers produce new blocks                                 │
│ - Validators sync and validate                                  │
└──────────────────┬──────────────────────────────────────────────┘
                   │
         ┌─────────┴─────────┬──────────────┐
         │                   │              │
         ↓                   ↓              ↓
   ┌──────────┐      ┌────────────┐  ┌────────────┐
   │op-batcher│      │op-proposer │  │op-conductor│
   └──────────┘      └────────────┘  └────────────┘
         │                   │              │
         │ Batches           │ State roots  │ Leader election
         ↓                   ↓              ↓
   Back to L1          Back to L1    Coordinate sequencers
```

## Resource Usage

Typical resource consumption for default deployment (2 seq + 3 val):

- **CPU**: 2-4 cores (bursty during block production)
- **Memory**: 4-6 GB RAM
- **Disk**: 5-10 GB (grows with blockchain data)
- **Network**: Minimal (local Docker network)

## Cleanup and Data Retention

### On Normal Shutdown (Ctrl+C)

Kupcake automatically:
- ✅ Stops all containers
- ✅ Removes all containers
- ✅ Removes Docker network
- ✅ **Keeps** all data in `./data-<network-name>/`

### With `--no-cleanup` Flag

```bash
./target/release/kupcake --no-cleanup
```

On Ctrl+C:
- ❌ Containers keep running
- ❌ Network remains active
- ✅ Data directory intact

### Manual Cleanup

```bash
# Stop and remove all containers + network
./target/release/kupcake cleanup <network-name>

# Remove data directory
rm -rf ./data-<network-name>
```

## Next Steps

- [CLI Reference](../user-guide/cli-reference.md) - Customize your deployment
- [Multi-Sequencer Setup](../user-guide/multi-sequencer.md) - Advanced sequencer configuration
- [Monitoring Guide](../user-guide/monitoring.md) - Understanding metrics and dashboards
- [Architecture Overview](../architecture/overview.md) - Technical deep dive

## Summary

A Kupcake deployment creates:
- **1 L1 chain** (Anvil)
- **1 Contract deployment** (op-deployer)
- **N L2 nodes** (op-reth + kona-node pairs)
- **3 Infrastructure services** (batcher, proposer, challenger)
- **1 Coordinator** (op-conductor, if multiple sequencers)
- **2 Monitoring services** (Prometheus, Grafana)

All working together to provide a complete, production-like OP Stack L2 environment for local development and testing.
