# Op-Reth + Kona Node Docker Setup

This directory contains a Docker Compose setup for running Op-Reth (Optimism execution client) and Kona-Node (consensus client) together with configurable version tags.

## Quick Start

### 1. Generate JWT Secret

First, generate a JWT secret for authentication between op-reth and kona-node:

```bash
./generate-jwt.sh
```

This creates a `jwttoken/jwt.hex` file required by both services.

### 2. Set Required Environment Variables

Create a `.env` file or export these required variables:

```bash
# L1 Execution Layer RPC endpoint (REQUIRED)
export L1_PROVIDER_RPC="https://ethereum-sepolia-rpc.publicnode.com"

# L1 Beacon API endpoint (REQUIRED)
export L1_BEACON_API="https://ethereum-sepolia-beacon-api.publicnode.com"
```

### 3. Start the Stack

```bash
# Use latest tags (default)
docker compose up -d

# Or specify custom tags
OP_RETH_TAG=v1.2.3 KONA_TAG=v0.5.0 docker compose up -d
```

### 4. Check Logs

```bash
# All services
docker compose logs -f

# Just op-reth
docker compose logs -f op-reth

# Just kona-node
docker compose logs -f kona-node
```

### 5. Stop the Stack

```bash
docker compose down

# To also remove volumes (WARNING: deletes blockchain data)
docker compose down -v
```

## Configuration

### Image Tags

Set custom tags for either service:

```bash
# Op-Reth tag
export OP_RETH_TAG=v1.2.3

# Kona tag
export KONA_TAG=v0.5.0

# Or use different image repositories
export OP_RETH_IMAGE=ghcr.io/myorg/op-reth
export KONA_IMAGE=ghcr.io/myorg/kona-node
```

### Chain Configuration

Change the chain being synced (default: `optimism-sepolia`):

```bash
# For Optimism Mainnet
export OP_RETH_CHAIN=optimism
export KONA_CHAIN=optimism
export OP_RETH_SEQUENCER_HTTP=https://mainnet-sequencer.optimism.io/

# For Base Sepolia
export OP_RETH_CHAIN=base-sepolia
export KONA_CHAIN=base-sepolia
export OP_RETH_SEQUENCER_HTTP=https://sepolia-sequencer.base.org/

# For Base Mainnet
export OP_RETH_CHAIN=base
export KONA_CHAIN=base
export OP_RETH_SEQUENCER_HTTP=https://mainnet-sequencer.base.org/
```

### Port Configuration

Customize exposed ports:

```bash
# Op-Reth ports
export OP_RETH_METRICS_PORT=9001
export OP_RETH_DISCOVERY_PORT=30303
export OP_RETH_RPC_PORT=8545
export OP_RETH_ENGINE_PORT=8551

# Kona ports
export KONA_DISCOVERY_PORT=9223
export KONA_METRICS_PORT=9002
export KONA_RPC_PORT=5060
```

### Logging

Adjust logging verbosity:

```bash
# Op-Reth logging
export OP_RETH_RUST_LOG=debug

# Kona logging (filter by crate)
export KONA_RUST_LOG="engine_builder=trace,runtime=debug,kona=info"
```

## Default Ports

| Port    | Service                     |
|---------|----------------------------|
| `30303` | Op-Reth discovery (TCP/UDP)|
| `9001`  | Op-Reth metrics            |
| `8545`  | Op-Reth RPC                |
| `8551`  | Op-Reth engine             |
| `9223`  | Kona-Node discovery (TCP/UDP)|
| `9002`  | Kona-Node metrics          |
| `5060`  | Kona-Node RPC              |
| `9091`  | Prometheus (optional)      |
| `3001`  | Grafana (optional)         |

## Monitoring (Optional)

The `docker-compose.yml` includes commented sections for Prometheus and Grafana. To enable monitoring:

1. Uncomment the `prometheus` and `grafana` services in `docker-compose.yml`
2. Uncomment the volume definitions at the bottom
3. Create prometheus and grafana config directories:
   ```bash
   mkdir -p prometheus grafana/{datasources,dashboards}
   ```
4. Configure Prometheus and Grafana (see examples in `kona/docker/recipes/kona-node/`)

## Connecting to Services

### Op-Reth RPC

```bash
# Get latest block number
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Using cast (foundry)
cast block-number --rpc-url http://localhost:8545
```

### Kona-Node RPC

```bash
curl http://localhost:5060/health
```

### Metrics

```bash
# Op-Reth metrics
curl http://localhost:9001/metrics

# Kona-Node metrics  
curl http://localhost:9002/metrics
```

## Building from Local Source

If you want to build images from local source code instead of using pre-built images:

### Build Op-Reth

```bash
cd reth
docker build -f DockerfileOp -t op-reth:local .
export OP_RETH_IMAGE=op-reth
export OP_RETH_TAG=local
```

### Build Kona-Node

```bash
cd kona
docker build -f docker/apps/kona_app_generic.dockerfile \
  --build-arg REPO_LOCATION=local \
  --build-arg BIN_TARGET=kona-node \
  --build-arg BUILD_PROFILE=release \
  -t kona-node:local .
export KONA_IMAGE=kona-node
export KONA_TAG=local
```

## Troubleshooting

### JWT Secret Issues

If you see authentication errors, ensure:
1. The JWT file exists: `cat jwttoken/jwt.hex`
2. Both services are using the same JWT file
3. The file is readable (it's mounted read-only)

### Network Issues

If services can't communicate:
```bash
# Check if both containers are on the same network
docker network inspect reth-kona-stack_reth-kona-network

# Verify op-reth is reachable from kona-node
docker compose exec kona-node curl -X POST http://op-reth:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

### Database Issues

If you need to resync from scratch:
```bash
# Stop services and remove volumes
docker compose down -v

# Restart
docker compose up -d
```

### Check Sync Status

```bash
# Op-Reth sync status
cast rpc eth_syncing --rpc-url http://localhost:8545

# View logs for sync progress
docker compose logs -f op-reth | grep -i "sync\|block\|import"
```

## Volume Locations

Data is persisted in Docker volumes:
- `op_reth_data`: Op-Reth blockchain database
- `op_reth_logs`: Op-Reth log files
- `kona_data`: Kona-Node P2P bootstore and state

To inspect volume location:
```bash
docker volume inspect reth-kona-stack_op_reth_data
```

## Advanced Configuration

### Custom Command Arguments

To pass additional flags, modify the `command` section in `docker-compose.yml`:

```yaml
# Example: Add tracing to op-reth
command: >
  node
  --datadir /db
  --chain optimism-sepolia
  --trace
  --trace.filter "*"
  # ... rest of command
```

### Using Host Networking

For better performance, you can use host networking:

```yaml
network_mode: host
```

Note: Port mappings won't work with host networking; services bind directly to host ports.

## References

- [Op-Reth Documentation](https://github.com/paradigmxyz/reth)
- [Kona Documentation](https://github.com/op-rs/kona)
- [Optimism Docs](https://docs.optimism.io)
- [Base Docs](https://docs.base.org)

