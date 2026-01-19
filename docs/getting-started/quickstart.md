# Quickstart Guide

**Target Audience**: New Users
**Time to Complete**: 5 minutes
**Prerequisites**: Docker installed and running

## Deploy Your First L2 Chain in 3 Steps

### Step 1: Build Kupcake

```bash
git clone https://github.com/op-rs/kupcake.git
cd kupcake
cargo build --release
```

### Step 2: Run Kupcake

```bash
./target/release/kupcake
```

That's it! Kupcake will:
- Fork Ethereum Sepolia (by default)
- Generate a random L2 chain ID
- Deploy all OP Stack contracts
- Start 2 sequencers + 3 validators
- Launch monitoring (Prometheus + Grafana)

### Step 3: Verify It's Running

Open Grafana in your browser:
```
http://localhost:3000
```

**Default credentials**: `admin` / `admin`

You should see:
- ✅ Anvil (L1) producing blocks every 12 seconds
- ✅ L2 sequencers deriving and producing L2 blocks
- ✅ Batcher submitting batches to L1
- ✅ Proposer submitting state roots to L1

## What Just Happened?

Kupcake deployed a complete OP Stack network with:

| Component | Count | Purpose |
|-----------|-------|---------|
| Anvil (L1 Fork) | 1 | Forked Sepolia for local L1 |
| op-reth | 5 | L2 execution clients (2 sequencers, 3 validators) |
| kona-node | 5 | L2 consensus clients |
| op-batcher | 1 | Batches L2 transactions to L1 |
| op-proposer | 1 | Proposes L2 state roots to L1 |
| op-challenger | 1 | Challenges invalid state roots |
| op-conductor | 1 | Coordinates sequencers (multi-sequencer mode) |
| Prometheus | 1 | Metrics collection |
| Grafana | 1 | Metrics visualization |

All data is saved in `./data-kup-sepolia-<random-chain-id>/`.

## Common Variations

### Fork Mainnet Instead of Sepolia

```bash
./target/release/kupcake --l1 mainnet
```

### Run Locally Without Forking

```bash
./target/release/kupcake
```

Since the default was changed, omitting `--l1` runs in local mode with a random L1 chain ID.

### Use a Custom L2 Chain ID

```bash
./target/release/kupcake --l2-chain 42069
```

### Run with Custom Network Name

```bash
./target/release/kupcake --network my-testnet
```

Data will be saved to `./data-my-testnet/`.

### Keep Containers Running After Exit

```bash
./target/release/kupcake --detach
```

Kupcake will deploy everything and exit, leaving containers running in the background.

## Stopping and Cleaning Up

### If Running in Foreground (Default)

Press `Ctrl+C`. Kupcake will automatically:
1. Stop all containers
2. Remove all containers
3. Remove the Docker network
4. Keep the data directory intact

### If Running Detached (`--detach`)

Use the cleanup command:

```bash
./target/release/kupcake cleanup <network-name>
```

Example:
```bash
./target/release/kupcake cleanup kup-sepolia-12345
```

### Keep Containers Running on Exit

```bash
./target/release/kupcake --no-cleanup
```

Press `Ctrl+C` and containers will keep running. You can manage them with Docker:

```bash
docker ps
docker stop <container-name>
```

## Reloading a Deployment

Kupcake saves your configuration to `Kupcake.toml` in the data directory. You can reload it later:

```bash
./target/release/kupcake --config ./data-kup-sepolia-12345/Kupcake.toml
```

This allows you to:
- Resume a deployment
- Modify and redeploy
- Share configurations with others

## Next Steps

- [First Deployment Guide](first-deployment.md) - Detailed walkthrough with explanations
- [Understanding Output](understanding-output.md) - What each component does
- [CLI Reference](../user-guide/cli-reference.md) - All available options
- [Examples](../examples/README.md) - More deployment scenarios

## Troubleshooting

### Docker Not Running

```
Error: Cannot connect to the Docker daemon
```

**Solution**: Start Docker Desktop or the Docker daemon:
```bash
sudo systemctl start docker  # Linux
# or
open -a Docker  # macOS
```

### Port Already in Use

```
Error: Bind for 0.0.0.0:8545 failed: port is already allocated
```

**Solution**: Stop the process using that port or use a custom network name:
```bash
./target/release/kupcake --network my-unique-name
```

### Build Fails

```
error: linker `cc` not found
```

**Solution**: Install build dependencies:
```bash
# Ubuntu/Debian
sudo apt-get install build-essential

# macOS
xcode-select --install
```

For more issues, see the [Troubleshooting Guide](../user-guide/troubleshooting.md).
