# Kupcake

A CLI tool to bootstrap a Rust-based OP Stack chain locally in seconds.

Kupcake spins up a local L1 (via Anvil fork), deploys all OP Stack contracts automatically, and starts a complete L2 node stack using **kona-node** and **op-reth**.

## Features

- **One-command deployment** — Deploy a complete OP Stack chain with a single command
- **Local L1 via Anvil** — Forks Sepolia or Mainnet using Foundry's Anvil
- **Automatic contract deployment** — Uses `op-deployer` to deploy all OP Stack contracts
- **Full L2 CL/EL stack** — Runs kona-node (consensus) + op-reth (execution) out of the box
- **Multi-sequencer support** — Multiple sequencers with op-conductor for Raft-based failover
- **Complete op-stack stack** — Includes op-batcher, op-proposer, op-challenger, and op-conductor
- **Built-in monitoring** — Prometheus + Grafana dashboards for metrics visualization
- **Config persistence** — Save and reload deployments via `Kupcake.toml`
- **Detach mode** — Deploy in the background and keep containers running
- **Custom images** — Override Docker images for any component
- **Docker-based** — No local toolchain required, just Docker

## Requirements

- [Docker](https://docs.docker.com/get-docker/) (running)
- Rust toolchain (for building from source)

## Quick Start

```bash
# Build
cargo build --release

# Run with defaults (forks Sepolia, random L2 chain ID)
./target/release/kupcake
```

That's it! Kupcake will:
1. Create a Docker network
2. Start Anvil forking Sepolia
3. Deploy OP Stack contracts via `op-deployer`
4. Generate `genesis.json` and `rollup.json`
5. Start L2 nodes (2 sequencers + 3 validators by default)
6. Start op-batcher, op-proposer, and op-challenger
7. Start op-conductor (coordinates sequencers via Raft)
8. Start Prometheus + Grafana monitoring stack

Once running, you'll have:
- **L1 (Anvil)** at `http://localhost:8545`
- **L2 (op-reth) HTTP** at `http://localhost:9545` (first sequencer)
- **L2 (op-reth) WS** at `ws://localhost:9546` (first sequencer)
- **Kona Node RPC** at `http://localhost:7545` (first sequencer)
- **Op Batcher RPC** at `http://localhost:8548`
- **Op Proposer RPC** at `http://localhost:8560`
- **Op Challenger RPC** at `http://localhost:8561`
- **Op Conductor RPC** at `http://localhost:8547` (first sequencer)
- **Prometheus** at `http://localhost:9099`
- **Grafana** at `http://localhost:3019` (admin/admin)

Additional sequencers and validators use dynamically assigned ports (see logs for URLs).

Press `Ctrl+C` to stop and clean up all containers, or use `--detach` to keep them running.

## Usage

Kupcake has two subcommands: `deploy` (default) and `cleanup`.

### Deploy Command

```
kupcake deploy [OPTIONS]

Options:
  -n, --network <NETWORK>           Custom network name [env: KUP_NETWORK_NAME]
  -v, --verbosity <VERBOSITY>       Log level [default: info] [env: KUP_VERBOSITY]
      --l1-rpc-provider <URL>       L1 RPC endpoint [default: public-node] [env: KUP_L1_RPC_URL]
      --l1-chain <CHAIN>            L1 chain (sepolia, mainnet, or chain ID) [default: sepolia]
      --l2-chain <CHAIN>            L2 chain ID (random if not set) [env: KUP_L2_CHAIN]
      --outdata <PATH>              Output directory [env: KUP_OUTDATA]
      --no-cleanup                  Keep containers running on exit [env: KUP_NO_CLEANUP]
      --detach                      Deploy and exit, leaving containers running [env: KUP_DETACH]
      --block-time <SECONDS>        L1/L2 block time [default: 12] [env: KUP_BLOCK_TIME]
      --l2-nodes <COUNT>            Total L2 nodes (sequencers + validators) [default: 5]
      --sequencer-count <COUNT>     Number of sequencers [default: 2]
      --config <PATH>               Load existing Kupcake.toml configuration
      --redeploy                    Force redeploy contracts even if state exists
      --<service>-image <IMAGE>     Custom Docker image for any service
      --<service>-tag <TAG>         Custom Docker tag for any service
  -h, --help                        Print help
```

### Cleanup Command

```
kupcake cleanup <PREFIX>

Stops and removes all containers with names starting with PREFIX,
then removes the Docker network (<PREFIX>-network).
```

## Examples

```bash
# Default deployment (forks Sepolia, 2 sequencers + 3 validators)
kupcake

# Fork Mainnet instead of Sepolia
kupcake --l1-chain mainnet

# Use a custom L2 chain ID
kupcake --l2-chain 42069

# Custom network name and output directory
kupcake --network my-testnet --outdata ./my-testnet-data

# Single sequencer setup (no op-conductor)
kupcake --sequencer-count 1 --l2-nodes 3

# Deploy and detach (leave running in background)
kupcake --detach

# Keep containers running after Ctrl+C (for debugging)
kupcake --no-cleanup

# Custom block time (2 second blocks)
kupcake --block-time 2

# Load from saved configuration
kupcake --config ./data-my-network/Kupcake.toml

# Custom Docker images
kupcake --op-reth-image ghcr.io/paradigmxyz/op-reth --op-reth-tag v1.0.0

# Verbose logging
kupcake -v debug

# Clean up containers from a previous deployment
kupcake cleanup my-testnet
```

## Multi-Sequencer Support

Kupcake supports running multiple sequencers coordinated by op-conductor using Raft consensus:

- The first sequencer is the initial Raft leader and starts active
- Additional sequencers start in stopped state, waiting for conductor to activate them
- Op-conductor manages leader election and sequencer failover

## Output

After running, you'll find these files in the output directory (`data-<network-name>/deployer/`):

| File | Description |
|------|-------------|
| `genesis.json` | L2 genesis configuration for execution clients |
| `rollup.json` | Rollup configuration for consensus clients |
| `intent.toml` | Deployment intent file used by op-deployer |
| `state.json` | Deployment state (contract addresses, etc.) |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                         Kupcake                             │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  1. Start Anvil (fork L1)                                   │
│     └─> Docker: ghcr.io/foundry-rs/foundry                  │
│                                                             │
│  2. Deploy OP Stack contracts                               │
│     └─> Docker: op-deployer init + apply                    │
│                                                             │
│  3. Generate config files                                   │
│     └─> Docker: op-deployer inspect genesis/rollup          │
│                                                             │
│  4. Start L2 nodes (sequencers + validators)                │
│     └─> Docker: op-reth (EL) + kona-node (CL) pairs         │
│                                                             │
│  5. Start op-stack services                                 │
│     └─> Docker: op-batcher, op-proposer, op-challenger      │
│                                                             │
│  6. Start op-conductor (if multi-sequencer)                 │
│     └─> Docker: op-conductor (Raft consensus)               │
│                                                             │
│  7. Start monitoring stack                                  │
│     └─> Docker: Prometheus + Grafana                        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Components

| Component | Image | Description |
|-----------|-------|-------------|
| Anvil | `ghcr.io/foundry-rs/foundry` | Local L1 chain (forks Sepolia/Mainnet) |
| op-deployer | `us-docker.pkg.dev/oplabs-tools-artifacts/images/op-deployer` | Deploys OP Stack contracts |
| op-reth | `ghcr.io/paradigmxyz/op-reth` | L2 execution client (EVM) |
| kona-node | `ghcr.io/op-rs/kona/kona-node` | L2 consensus client (sequencer/validator) |
| op-batcher | `us-docker.pkg.dev/oplabs-tools-artifacts/images/op-batcher` | Batches L2 transactions to L1 |
| op-proposer | `us-docker.pkg.dev/oplabs-tools-artifacts/images/op-proposer` | Proposes L2 output roots to L1 |
| op-challenger | `us-docker.pkg.dev/oplabs-tools-artifacts/images/op-challenger` | Monitors and challenges invalid proposals |
| op-conductor | `us-docker.pkg.dev/oplabs-tools-artifacts/images/op-conductor` | Coordinates sequencers via Raft consensus |
| Prometheus | `prom/prometheus` | Metrics collection and storage |
| Grafana | `grafana/grafana` | Metrics visualization and dashboards |

## Ports

Default ports for the first instance of each service. Additional instances use dynamically assigned ports.

| Service | Port | Protocol |
|---------|------|----------|
| Anvil (L1) | 8545 | HTTP |
| op-reth HTTP | 9545 | HTTP |
| op-reth WebSocket | 9546 | WS |
| op-reth Auth RPC | 9551 | HTTP |
| op-reth Metrics | 9001 | HTTP |
| kona-node RPC | 7545 | HTTP |
| kona-node Metrics | 7300 | HTTP |
| op-batcher RPC | 8548 | HTTP |
| op-batcher Metrics | 7301 | HTTP |
| op-proposer RPC | 8560 | HTTP |
| op-proposer Metrics | 7302 | HTTP |
| op-challenger RPC | 8561 | HTTP |
| op-challenger Metrics | 7303 | HTTP |
| op-conductor RPC | 8547 | HTTP |
| op-conductor Raft | 50050 | HTTP |
| Prometheus | 9099 | HTTP |
| Grafana | 3019 | HTTP |

## License

MIT
