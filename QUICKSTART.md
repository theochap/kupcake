# Quick Start Guide

Get Op-Reth and Kona-Node running in 3 steps.

## 1. Setup Environment

```bash
# Copy the sample config
cp env.sample .env

# Edit .env and set your L1 endpoints (REQUIRED!)
# At minimum, set these two variables:
# L1_PROVIDER_RPC=https://your-l1-rpc-endpoint
# L1_BEACON_API=https://your-l1-beacon-endpoint

# Or export them directly:
export L1_PROVIDER_RPC="https://ethereum-sepolia-rpc.publicnode.com"
export L1_BEACON_API="https://ethereum-sepolia-beacon-api.publicnode.com"
```

## 2. Start Stack

```bash
# Using the helper script (recommended)
./stack.sh up

# Or using docker compose directly
./generate-jwt.sh
docker compose up -d
```

## 3. Verify It's Running

```bash
# Check status
./stack.sh status

# View logs
./stack.sh logs

# Test RPC endpoint
./stack.sh test-rpc
```

## Using Specific Tags

```bash
# Start with specific versions
OP_RETH_TAG=v1.2.3 KONA_TAG=v0.5.0 ./stack.sh up

# Or set in .env file:
# OP_RETH_TAG=v1.2.3
# KONA_TAG=v0.5.0
```

## Available Images

Find available tags on GitHub Container Registry:

- **Op-Reth**: https://github.com/paradigmxyz/reth/pkgs/container/op-reth
- **Kona-Node**: https://github.com/op-rs/kona/pkgs/container/kona%2Fkona-node

## Useful Commands

```bash
# View logs for specific service
./stack.sh logs op-reth
./stack.sh logs kona-node

# See all options
./stack.sh help

# Stop (keeps data)
./stack.sh down

# Stop and remove data
./stack.sh down -v

# Open shell in container
./stack.sh shell op-reth

# Show metrics endpoints
./stack.sh metrics
```

## Default Endpoints

| Service | Endpoint | Description |
|---------|----------|-------------|
| Op-Reth RPC | http://localhost:8545 | JSON-RPC |
| Op-Reth Engine | http://localhost:8551 | Engine API |
| Op-Reth Metrics | http://localhost:9001/metrics | Prometheus metrics |
| Kona RPC | http://localhost:5060 | Rollup node RPC |
| Kona Metrics | http://localhost:9002/metrics | Prometheus metrics |
| Prometheus | http://localhost:9091 | Metrics server (optional) |
| Grafana | http://localhost:3001 | Dashboard UI (optional) |

## Testing the Setup

```bash
# Get current block number
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Using cast (foundry)
cast block-number --rpc-url http://localhost:8545

# Check Op-Reth metrics
curl http://localhost:9001/metrics | grep reth_sync

# Check Kona metrics
curl http://localhost:9002/metrics | grep kona
```

## Common Issues

### "L1_PROVIDER_RPC must be set"
Set the required environment variables (see step 1 above).

### "No such file or directory: jwt.hex"
Run `./generate-jwt.sh` to create the JWT secret.

### Port already in use
Change ports in `.env` or `env.sample`:
```bash
OP_RETH_RPC_PORT=8546
KONA_RPC_PORT=5061
```

### Containers keep restarting
Check logs: `./stack.sh logs`

Common causes:
- Wrong L1 endpoints
- Network connectivity issues
- Insufficient disk space
- Wrong chain configuration

## Switching Networks

### OP Mainnet
```bash
export OP_RETH_CHAIN=optimism
export KONA_CHAIN=optimism
export OP_RETH_SEQUENCER_HTTP=https://mainnet-sequencer.optimism.io/
export L1_PROVIDER_RPC=https://ethereum-mainnet-rpc-url
export L1_BEACON_API=https://ethereum-mainnet-beacon-url
```

### Base Sepolia
```bash
export OP_RETH_CHAIN=base-sepolia
export KONA_CHAIN=base-sepolia
export OP_RETH_SEQUENCER_HTTP=https://sepolia-sequencer.base.org/
```

### Base Mainnet
```bash
export OP_RETH_CHAIN=base
export KONA_CHAIN=base
export OP_RETH_SEQUENCER_HTTP=https://mainnet-sequencer.base.org/
export L1_PROVIDER_RPC=https://ethereum-mainnet-rpc-url
export L1_BEACON_API=https://ethereum-mainnet-beacon-url
```

## Need More Help?

See the full [README.md](./README.md) for detailed documentation.

