# Your First Deployment

**Target Audience**: New Users
**Time to Complete**: 10-15 minutes
**Prerequisites**: Kupcake built and Docker running

This guide walks you through your first deployment with detailed explanations of what's happening at each step.

## Overview

You'll deploy a complete OP Stack L2 network that includes:
- A local L1 (Anvil fork of Sepolia)
- OP Stack smart contracts deployed on L1
- Multiple L2 nodes (sequencers and validators)
- Transaction batching and state root proposals
- Monitoring infrastructure

## Step-by-Step Deployment

### Step 1: Choose Your Configuration

For your first deployment, let's use local mode (no L1 fork):

```bash
./target/release/kupcake \
  --network my-first-l2 \
  --l2-chain 42069
```

This command:
- `--network my-first-l2` - Names your network (data saved to `./data-my-first-l2/`)
- `--l2-chain 42069` - Sets L2 chain ID to 42069

### Step 2: Watch the Deployment

Kupcake will output detailed logs as it deploys. Here's what happens:

#### Phase 1: Network Setup (5-10 seconds)
```
Creating Docker network: my-first-l2-network
```

Kupcake creates an isolated Docker network for all containers.

#### Phase 2: L1 Setup (10-15 seconds)
```
Starting Anvil (L1 fork)
L1 RPC available at: http://localhost:8545
```

Anvil starts and begins producing L1 blocks. In local mode, it uses a random chain ID.

#### Phase 3: Contract Deployment (30-60 seconds)
```
Deploying OP Stack contracts...
Running op-deployer init...
Running op-deployer apply...
Contracts deployed successfully
```

This deploys all OP Stack contracts to the L1:
- `L1CrossDomainMessenger`
- `L1StandardBridge`
- `OptimismPortal`
- `L2OutputOracle`
- `SystemConfig`
- And more...

Contract addresses are saved to `./data-my-first-l2/l2-stack/state.json`.

#### Phase 4: L2 Node Startup (20-30 seconds)
```
Starting op-reth (execution client) for sequencer-1
Starting kona-node (consensus client) for sequencer-1
Starting op-reth (execution client) for sequencer-2
Starting kona-node (consensus client) for sequencer-2
...
```

Each L2 node consists of two containers:
- **op-reth**: Execution layer (EVM, state, transactions)
- **kona-node**: Consensus layer (block derivation, L1 data fetching)

#### Phase 5: Service Startup (10-15 seconds)
```
Starting op-batcher
Starting op-proposer
Starting op-challenger
Starting op-conductor
```

- **op-batcher**: Batches L2 transactions and submits them to L1
- **op-proposer**: Proposes L2 state roots to L1
- **op-challenger**: Challenges invalid state roots (fault proofs)
- **op-conductor**: Coordinates multiple sequencers using Raft consensus

#### Phase 6: Monitoring Setup (5-10 seconds)
```
Starting Prometheus
Starting Grafana
Grafana available at: http://localhost:3000
```

Monitoring stack is ready!

### Step 3: Verify the Deployment

#### Check Container Status

```bash
docker ps --filter name=my-first-l2
```

You should see ~15 containers running:
- 1 Anvil (L1)
- 10 L2 nodes (2 sequencers × 2 containers + 3 validators × 2 containers)
- 1 op-batcher
- 1 op-proposer
- 1 op-challenger
- 1 op-conductor
- 1 Prometheus
- 1 Grafana

#### Check Grafana

Open http://localhost:3000 in your browser.

**Login**: `admin` / `admin` (you'll be prompted to change this)

Navigate to Dashboards → Browse → Select an OP Stack dashboard.

You should see:
- L1 (Anvil) producing blocks every 12 seconds
- L2 sequencers producing blocks
- Batcher submitting batches to L1
- Proposer submitting state roots

#### Check Container Logs

```bash
# L1 logs
docker logs my-first-l2-anvil

# Sequencer logs
docker logs my-first-l2-op-reth-sequencer-1
docker logs my-first-l2-kona-node-sequencer-1

# Batcher logs
docker logs my-first-l2-op-batcher
```

### Step 4: Interact with Your L2

#### Connect with MetaMask

1. Open MetaMask
2. Add a custom network:
   - **Network Name**: My First L2
   - **RPC URL**: `http://localhost:9545` (first sequencer RPC port)
   - **Chain ID**: `42069`
   - **Currency Symbol**: ETH

3. Import a funded account:
   - Private key: Check `./data-my-first-l2/anvil/anvil.json` for test accounts
   - These accounts have test ETH on both L1 and L2

#### Send a Transaction

1. In MetaMask, send some ETH to another address
2. Watch the transaction in Grafana
3. Check that it appears on L1 (batched by op-batcher)

### Step 5: Explore the Data Directory

```bash
tree -L 2 ./data-my-first-l2
```

```
./data-my-first-l2/
├── Kupcake.toml              # Saved configuration
├── anvil/
│   ├── anvil.json            # L1 test accounts
│   └── state.json            # L1 state snapshots
├── l2-stack/
│   ├── genesis.json          # L2 genesis configuration
│   ├── rollup.json           # Rollup config for consensus
│   ├── intent.toml           # op-deployer intent
│   ├── state.json            # Deployed contract addresses
│   ├── jwt-*.hex             # JWT secrets for each node
│   └── reth-data-*/          # op-reth data directories
└── monitoring/
    ├── prometheus.yml        # Prometheus config
    └── grafana/              # Grafana data
```

### Step 6: Stop the Deployment

Press `Ctrl+C` in the terminal where Kupcake is running.

Kupcake will:
1. Stop all containers
2. Remove all containers
3. Remove the Docker network
4. **Keep** the data directory intact

## What You Just Deployed

### L1 Layer
- **Anvil**: Local Ethereum fork with 12-second block time
- **Smart Contracts**: Full OP Stack contract suite

### L2 Layer
- **2 Sequencers**: Produce and sequence L2 blocks
- **3 Validators**: Validate L2 blocks (read-only nodes)
- **op-conductor**: Coordinates sequencers for high availability

### Infrastructure
- **op-batcher**: Posts L2 transaction data to L1
- **op-proposer**: Posts L2 state roots to L1 (every ~10 minutes)
- **op-challenger**: Monitors for invalid state roots
- **Prometheus + Grafana**: Metrics and dashboards

## Common Next Steps

### Run Again with Same Configuration

```bash
./target/release/kupcake --config ./data-my-first-l2/Kupcake.toml
```

### Fork Sepolia Instead

```bash
./target/release/kupcake --l1 sepolia --network sepolia-test
```

### Change Block Time

```bash
./target/release/kupcake --block-time 2 --network fast-l2
```

Produces L1 blocks every 2 seconds (faster testing).

### Single Sequencer Mode

```bash
./target/release/kupcake --sequencer-count 1 --l2-nodes 3
```

Deploys 1 sequencer and 2 validators (op-conductor not needed).

## Troubleshooting

### Port Conflicts

If ports are already in use:

```
Error: Bind for 0.0.0.0:8545 failed: port is already allocated
```

**Solution**: Use a different network name or stop the conflicting service:
```bash
./target/release/kupcake --network my-unique-name
```

### Container Startup Failures

If a container fails to start, check its logs:

```bash
docker logs my-first-l2-<container-name>
```

Common issues:
- **JWT mismatch**: Deleted data directory but didn't redeploy contracts
- **Port in use**: Another service using the same port
- **Image pull failure**: Check Docker Hub connectivity

**Solution**: Clean up and redeploy:
```bash
./target/release/kupcake cleanup my-first-l2
rm -rf ./data-my-first-l2
./target/release/kupcake --network my-first-l2
```

### Batcher Not Submitting

If the batcher isn't submitting batches:

```bash
docker logs my-first-l2-op-batcher
```

Common issues:
- Insufficient gas: Anvil accounts may need more ETH
- L1 not producing blocks: Check Anvil logs

### Grafana Shows No Data

Wait 30-60 seconds for Prometheus to scrape initial metrics.

If still no data:
```bash
# Check Prometheus targets
curl http://localhost:9090/api/v1/targets | jq
```

All targets should show state: `up`.

## Next Steps

- [Understanding Output](understanding-output.md) - Deep dive into what was deployed
- [CLI Reference](../user-guide/cli-reference.md) - All available options
- [Multi-Sequencer Setup](../user-guide/multi-sequencer.md) - Advanced sequencer configuration
- [Examples](../examples/README.md) - More deployment scenarios

## Summary

You've successfully deployed a complete OP Stack L2 network with:
- ✅ Local L1 blockchain
- ✅ OP Stack smart contracts
- ✅ Multiple L2 nodes (sequencers and validators)
- ✅ Transaction batching and state proposals
- ✅ Monitoring infrastructure

Your network is fully functional and ready for testing!
