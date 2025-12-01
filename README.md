# Kupcake

A CLI tool to bootstrap a Rust-based OP Stack chain locally in seconds.

Kupcake spins up a local L1 (via Anvil fork) and deploys all OP Stack contracts automatically, generating the configuration files needed to run your L2 nodes.

## Features

- **One-command deployment** — Deploy a complete OP Stack chain with a single command
- **Local L1 via Anvil** — Forks Sepolia or Mainnet using Foundry's Anvil
- **Automatic contract deployment** — Uses `op-deployer` to deploy all OP Stack contracts
- **Config generation** — Outputs `genesis.json` and `rollup.json` ready for your L2 nodes
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
4. Generate `genesis.json` and `rollup.json` in the output directory

Press `Ctrl+C` to stop and clean up containers.

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
└─────────────────────────────────────────────────────────────┘
```

## Next Steps

After kupcake completes, you can:

1. **Start L2 execution client** (e.g., op-reth, op-geth) with `genesis.json`
2. **Start L2 consensus client** (e.g., kona-node, op-node) with `rollup.json`
3. **Configure your network** with the deployed contract addresses from `state.json`

## License

MIT
