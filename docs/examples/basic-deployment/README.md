# Basic Deployment Example

**Demonstrates**: Simplest possible Kupcake deployment with all defaults

## What This Example Does

This example runs Kupcake with minimal configuration:
- Local mode (no L1 fork)
- Random L1 and L2 chain IDs
- Default node configuration (2 sequencers + 3 validators)
- Detached mode (deploy and exit)

## Running the Example

```bash
./run.sh
```

The script will:
1. Build kupcake if not already built
2. Deploy the network with default settings
3. Exit immediately (detached mode)
4. Show you how to verify and cleanup

## What Gets Deployed

| Component | Count | Purpose |
|-----------|-------|---------|
| Anvil (L1) | 1 | Local L1 blockchain |
| op-reth | 5 | L2 execution clients (2 seq + 3 val) |
| kona-node | 5 | L2 consensus clients |
| op-batcher | 1 | Batch L2 txs to L1 |
| op-proposer | 1 | Propose L2 state roots |
| op-challenger | 1 | Challenge invalid proposals |
| op-conductor | 1 | Coordinate sequencers |
| Prometheus | 1 | Metrics collection |
| Grafana | 1 | Metrics visualization |

**Total**: ~15 containers

## Expected Output

```
Kupcake Example: Basic Deployment
This example demonstrates: Simplest deployment with all defaults

Press Enter to continue or Ctrl+C to cancel...

Running kupcake with:
  --network kup-example-basic
  --detach

Creating Docker network: kup-example-basic-network
Starting Anvil (L1)...
Deploying OP Stack contracts...
Starting L2 nodes...
Starting infrastructure services...
Starting monitoring...

Deployment complete!

Example completed!
Next steps:
  - Check containers: docker ps --filter name=kup-example-basic
  - View Grafana: http://localhost:3000 (admin/admin)
  - View logs: docker logs kup-example-basic-anvil
  - Cleanup: kupcake cleanup kup-example-basic
```

## Verifying the Deployment

### Check Running Containers

```bash
docker ps --filter name=kup-example-basic
```

Should show ~15 containers running.

### Check Grafana

Open http://localhost:3000 in your browser.

**Login**: `admin` / `admin`

Navigate to Dashboards → Browse and select an OP Stack dashboard.

### Check Container Logs

```bash
# L1 logs
docker logs kup-example-basic-anvil

# Sequencer logs
docker logs kup-example-basic-op-reth-sequencer-1

# Batcher logs
docker logs kup-example-basic-op-batcher
```

### Check Data Directory

```bash
ls -la ./data-kup-example-basic/
```

Should contain:
- `Kupcake.toml` - Saved configuration
- `anvil/` - L1 data
- `l2-stack/` - L2 and contract data
- `monitoring/` - Prometheus and Grafana data

## Interacting with the Network

### RPC Endpoints

- **L1 (Anvil)**: http://localhost:8545
- **L2 Sequencer 1**: http://localhost:9545
- **L2 Sequencer 2**: http://localhost:9645

### Test Accounts

Check `./data-kup-example-basic/anvil/anvil.json` for funded test accounts.

### Send a Transaction

Using curl:

```bash
# Get chain ID
curl -X POST http://localhost:9545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'

# Get latest block number
curl -X POST http://localhost:9545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

## Cleanup

Stop and remove all containers:

```bash
kupcake cleanup kup-example-basic
```

Or using Docker directly:

```bash
docker stop $(docker ps -q --filter name=kup-example-basic)
docker rm $(docker ps -aq --filter name=kup-example-basic)
docker network rm kup-example-basic-network
```

Remove data directory (optional):

```bash
rm -rf ./data-kup-example-basic
```

## Variations

### Run in Foreground Instead

```bash
kupcake --network kup-example-basic
# Press Ctrl+C to stop (auto-cleanup)
```

### Fork Sepolia

```bash
kupcake --network kup-example-sepolia --l1 sepolia --detach
```

### Custom Chain ID

```bash
kupcake --network kup-example-42069 --l2-chain 42069 --detach
```

## Troubleshooting

### Port Already in Use

If you see:
```
Error: Bind for 0.0.0.0:8545 failed: port is already allocated
```

**Solution**: Use a different network name:
```bash
kupcake --network kup-example-unique --detach
```

### Container Fails to Start

Check logs:
```bash
docker logs kup-example-basic-<container-name>
```

Common causes:
- Previous deployment not cleaned up
- Docker resource limits reached

**Solution**: Clean up and retry:
```bash
kupcake cleanup kup-example-basic
./run.sh
```

## Next Steps

- Try [Mainnet Fork Example](../mainnet-fork/) for realistic testing
- Try [Fast Blocks Example](../fast-blocks/) for rapid iteration
- Read [Understanding Output](../../getting-started/understanding-output.md) to learn what each component does
- Explore [CLI Reference](../../user-guide/cli-reference.md) for all options

## Related Documentation

- [Quickstart Guide](../../getting-started/quickstart.md)
- [First Deployment](../../getting-started/first-deployment.md)
- [CLI Reference](../../user-guide/cli-reference.md)
