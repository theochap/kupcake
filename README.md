# Kupcake

A CLI tool to bootstrap a Rust-based OP Stack chain locally in seconds.

Kupcake spins up a local L1 (via Anvil fork), deploys all OP Stack contracts automatically, and starts a complete L2 node stack using **kona-node** and **op-reth**.

## Features

- **One-command deployment** — Deploy a complete OP Stack chain with a single command
- **Local L1 via Anvil** — Forks Sepolia or Mainnet using Foundry's Anvil
- **Automatic contract deployment** — Uses `op-deployer` to deploy all OP Stack contracts
- **Full L2 node stack** — Runs kona-node (consensus) + op-reth (execution) out of the box
- **Complete sequencer stack** — Includes op-batcher, op-proposer, and op-challenger
- **Built-in monitoring** — Prometheus + Grafana dashboards for metrics visualization
- **Config generation** — Outputs `genesis.json` and `rollup.json` for L2 nodes
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
5. Start op-reth (L2 execution client)
6. Start kona-node (L2 consensus client in sequencer mode)
7. Start op-batcher, op-proposer, and op-challenger
8. Start Prometheus + Grafana monitoring stack

Once running, you'll have:
- **L1 (Anvil)** at `http://localhost:8545`
- **L2 (op-reth) HTTP** at `http://localhost:9545`
- **L2 (op-reth) WS** at `ws://localhost:9546`
- **Kona Node RPC** at `http://localhost:7545`
- **Op Batcher RPC** at `http://localhost:8548`
- **Op Proposer RPC** at `http://localhost:8560`
- **Op Challenger RPC** at `http://localhost:8561`
- **Prometheus** at `http://localhost:9099`
- **Grafana** at `http://localhost:3019` (admin/admin)

Press `Ctrl+C` to stop and clean up all containers.

## Usage

```
kupcake [OPTIONS]

Options:
  -v, --verbosity <VERBOSITY>    Log level [default: INFO] [env: KUP_VERBOSITY]
  -n, --network <NETWORK>        Custom network name [env: KUP_NETWORK_NAME]
      --l1-rpc-provider <URL>    L1 RPC endpoint [default: public-node] [env: KUP_L1_RPC_URL]
      --l1-chain <CHAIN>         L1 chain (sepolia, mainnet) [default: sepolia] [env: KUP_L1_CHAIN]
      --l2-chain <CHAIN>         L2 chain ID (random if not set) [env: KUP_L2_CHAIN]
      --outdata <PATH>           Output directory [env: KUP_OUTDATA]
      --no-cleanup               Keep containers running on exit [env: KUP_NO_CLEANUP]
  -h, --help                     Print help
  -V, --version                  Print version
```

## Examples

```bash
# Fork Mainnet instead of Sepolia
kupcake --l1-chain mainnet

# Use a custom L2 chain ID
kupcake --l2-chain 42069

# Custom network name and output directory
kupcake --network my-testnet --outdata ./my-testnet-data

# Keep containers running after exit (for debugging)
kupcake --no-cleanup

# Verbose logging
kupcake -v debug
```

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
│  4. Start L2 execution client                               │
│     └─> Docker: ghcr.io/paradigmxyz/op-reth                 │
│                                                             │
│  5. Start L2 consensus client (sequencer mode)              │
│     └─> Docker: ghcr.io/op-rs/kona/kona-node                │
│                                                             │
│  6. Start sequencer services                                │
│     └─> Docker: op-batcher, op-proposer, op-challenger      │
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
| kona-node | `ghcr.io/op-rs/kona/kona-node` | L2 consensus client (sequencer) |
| op-batcher | `us-docker.pkg.dev/oplabs-tools-artifacts/images/op-batcher` | Batches L2 transactions to L1 |
| op-proposer | `us-docker.pkg.dev/oplabs-tools-artifacts/images/op-proposer` | Proposes L2 output roots to L1 |
| op-challenger | `us-docker.pkg.dev/oplabs-tools-artifacts/images/op-challenger` | Monitors and challenges invalid proposals |
| Prometheus | `prom/prometheus` | Metrics collection and storage |
| Grafana | `grafana/grafana` | Metrics visualization and dashboards |

## Ports

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
| Prometheus | 9099 | HTTP |
| Grafana | 3019 | HTTP |

## License

MIT
