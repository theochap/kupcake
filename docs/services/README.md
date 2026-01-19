# Service Documentation Overview

**Target Audience**: Developers | Advanced Users

Complete reference for all services deployed by Kupcake.

## Service Categories

### L1 Layer

#### [Anvil](anvil.md)
**Purpose**: Local Ethereum L1 fork
**Image**: `ghcr.io/foundry-rs/foundry`
**Port**: 8545

Local L1 blockchain using Foundry's Anvil. Can fork Sepolia, Mainnet, or run standalone.

### Contract Deployment

#### [op-deployer](op-deployer.md)
**Purpose**: Deploy OP Stack contracts to L1
**Image**: `ghcr.io/ethereum-optimism/op-deployer`

Deploys all OP Stack smart contracts to L1 using Foundry.

### L2 Layer

#### [op-reth](op-reth.md)
**Purpose**: L2 execution client (EVM)
**Image**: `ghcr.io/op-rs/op-reth`
**Ports**: 9545 (RPC), 9546 (WS), 9001 (metrics)

Rust implementation of Ethereum execution client, modified for OP Stack.

#### [kona-node](kona-node.md)
**Purpose**: L2 consensus client (derivation)
**Image**: `ghcr.io/op-rs/kona`
**Ports**: 7545 (RPC), 9002 (metrics)

OP Stack consensus client that derives L2 blocks from L1 data.

### Infrastructure Services

#### [op-batcher](op-batcher.md)
**Purpose**: Batch L2 transactions to L1
**Image**: `ghcr.io/ethereum-optimism/op-batcher`
**Ports**: 8548 (RPC), 7300 (metrics)

Compresses and submits L2 transaction batches to L1 as calldata.

#### [op-proposer](op-proposer.md)
**Purpose**: Propose L2 state roots to L1
**Image**: `ghcr.io/ethereum-optimism/op-proposer`
**Ports**: 8560 (RPC), 7300 (metrics)

Calculates and proposes L2 state roots to the L2OutputOracle contract on L1.

#### [op-challenger](op-challenger.md)
**Purpose**: Challenge invalid state root proposals
**Image**: `ghcr.io/ethereum-optimism/op-challenger`
**Ports**: 8561 (RPC), 7300 (metrics)

Monitors state root proposals and initiates dispute games for invalid proposals.

#### [op-conductor](op-conductor.md)
**Purpose**: Multi-sequencer coordination via Raft
**Image**: `ghcr.io/ethereum-optimism/op-conductor`
**Ports**: 8547 (RPC), 50050 (Raft)

Coordinates multiple sequencers using Raft consensus for high availability.

**Note**: Only deployed when `--sequencer-count > 1`.

### Monitoring

#### [Prometheus](prometheus.md)
**Purpose**: Metrics collection and storage
**Image**: `prom/prometheus`
**Port**: 9090

Time-series database that scrapes metrics from all services.

#### [Grafana](grafana.md)
**Purpose**: Metrics visualization and dashboards
**Image**: `grafana/grafana`
**Port**: 3000

Web UI for visualizing Prometheus metrics with pre-configured dashboards.

**Default credentials**: `admin` / `admin`

## Common Service Patterns

All services follow consistent patterns:

### Configuration

Each service has:
- **Builder type** - Configuration before deployment (e.g., `OpRethBuilder`)
- **Config type** - Serializable configuration (e.g., `OpRethConfig`)
- **Handler type** - Runtime handle to container (e.g., `OpRethHandler`)

### Docker Images

All images are configurable via:
- CLI arguments: `--<service>-image`, `--<service>-tag`
- Environment variables: `KUP_<SERVICE>_IMAGE`, `KUP_<SERVICE>_TAG`
- Configuration file: `Kupcake.toml`

### Metrics

Most services expose Prometheus metrics on a dedicated port:
- op-reth: 9001+
- kona-node: 9002+
- op-batcher, op-proposer, op-challenger: 7300
- op-conductor: 8080

### Networking

All services run in a custom Docker network (`<network-name>-network`):
- Internal communication uses container names as hostnames
- External access via port mappings to host

### Logging

All services log to stdout/stderr:
- View logs: `docker logs <container-name>`
- Follow logs: `docker logs -f <container-name>`

## Service Dependencies

```
┌─────────────────────────────────────────────────────────────┐
│ L1 Layer (Anvil)                                            │
│ - Produces blocks every <block-time> seconds                │
│ - Stores OP Stack contracts                                 │
└──────────────────┬──────────────────────────────────────────┘
                   │
                   │ (L1 data, contracts)
                   ↓
┌─────────────────────────────────────────────────────────────┐
│ L2 Consensus Layer (kona-node)                              │
│ - Fetches L1 blocks and batches                             │
│ - Derives L2 blocks from L1 data                            │
└──────────────────┬──────────────────────────────────────────┘
                   │
                   │ (Derived blocks via Engine API + JWT)
                   ↓
┌─────────────────────────────────────────────────────────────┐
│ L2 Execution Layer (op-reth)                                │
│ - Executes EVM transactions                                 │
│ - Maintains state                                            │
│ - Provides RPC for wallets                                  │
└──────────────────┬──────────────────────────────────────────┘
                   │
         ┌─────────┴─────────┬──────────────┬──────────────┐
         │                   │              │              │
         ↓                   ↓              ↓              ↓
   ┌──────────┐      ┌────────────┐  ┌────────────┐ ┌────────────┐
   │op-batcher│      │op-proposer │  │op-challenger│ │op-conductor│
   └──────────┘      └────────────┘  └────────────┘ └────────────┘
         │                   │              │              │
         ↓                   ↓              │              ↓
   Back to L1          Back to L1          │      Coordinate
                                            │      sequencers
                                            ↓
                                       Monitor L1
```

## Service Roles

### Sequencer vs. Validator

L2 nodes can be deployed in two roles:

**Sequencer**:
- Produces and sequences new L2 blocks
- Writes to local database
- Feeds batcher and proposer

**Validator**:
- Syncs and validates L2 blocks
- Read-only mode
- Verifies sequencer blocks

Configuration: `--sequencer-count` and `--l2-nodes`

## Port Allocation

Default port mappings:

| Service | Container Port | Host Port Range |
|---------|----------------|-----------------|
| Anvil | 8545 | 8545 |
| Sequencer 1 RPC | 8545 | 9545 |
| Sequencer 1 WS | 8546 | 9546 |
| Sequencer 2 RPC | 8545 | 9645 |
| Sequencer 2 WS | 8546 | 9646 |
| Validator 1 RPC | 8545 | 9745 |
| Prometheus | 9090 | 9090 |
| Grafana | 3000 | 3000 |

Sequencers increment by 100, validators continue the pattern.

## Service-Specific Documentation

For detailed documentation on each service, see the individual service pages:

- [Anvil](anvil.md) - L1 fork configuration and usage
- [op-deployer](op-deployer.md) - Contract deployment process
- [op-reth](op-reth.md) - Execution client configuration
- [kona-node](kona-node.md) - Consensus client configuration
- [op-batcher](op-batcher.md) - Transaction batching configuration
- [op-proposer](op-proposer.md) - State root proposal configuration
- [op-challenger](op-challenger.md) - Fault proof configuration
- [op-conductor](op-conductor.md) - Multi-sequencer coordination
- [Prometheus](prometheus.md) - Metrics collection setup
- [Grafana](grafana.md) - Dashboard configuration

## Related Documentation

- [Architecture Overview](../architecture/overview.md) - System architecture
- [User Guide](../user-guide/cli-reference.md) - CLI configuration options
- [Understanding Output](../getting-started/understanding-output.md) - What gets deployed
