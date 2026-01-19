# Multi-Sequencer Example

**Demonstrates**: High-availability setup with multiple sequencers

## What This Example Does

This example deploys a high-availability network with multiple sequencers:
- 3 sequencer nodes
- 4 validator nodes
- op-conductor for Raft-based coordination
- Leader election and automatic failover

## Running the Example

```bash
./run.sh
```

## What Gets Deployed

| Component | Count | Notes |
|-----------|-------|-------|
| Anvil (L1) | 1 | Local L1 |
| op-reth | 7 | 3 sequencers + 4 validators |
| kona-node | 7 | Paired with op-reth |
| op-batcher | 1 | Batches transactions |
| op-proposer | 1 | Proposes state roots |
| op-challenger | 1 | Challenges proposals |
| **op-conductor** | 1 | **Coordinates sequencers via Raft** |
| Prometheus | 1 | Metrics |
| Grafana | 1 | Dashboards |

**Total**: ~18 containers

## How Multi-Sequencer Works

### Raft Consensus

op-conductor uses Raft consensus to coordinate sequencers:

1. **Leader Election**: Sequencer 1 starts as the leader
2. **Active Sequencer**: Only the leader produces blocks
3. **Standby Sequencers**: Others stay stopped, ready for failover
4. **Automatic Failover**: If leader fails, conductor elects a new leader

### Sequencer States

- **Sequencer 1** (index 0): Initial leader, starts **active**
- **Sequencer 2** (index 1): Starts **stopped**, standby
- **Sequencer 3** (index 2): Starts **stopped**, standby

### Viewing Conductor Logs

```bash
docker logs kup-example-multi-sequencer-op-conductor
```

You should see Raft messages about leader election and cluster status.

## Use Cases

- High-availability testing
- Understanding Raft consensus
- Production-like setups
- Leader failover scenarios

## Testing Failover

### 1. Check Current Leader

```bash
docker logs kup-example-multi-sequencer-op-conductor | grep -i "leader"
```

### 2. Stop the Leader

```bash
docker stop kup-example-multi-sequencer-op-reth-sequencer-1
docker stop kup-example-multi-sequencer-kona-node-sequencer-1
```

### 3. Watch Conductor Elect New Leader

```bash
docker logs -f kup-example-multi-sequencer-op-conductor
```

Should show leader election and a new sequencer becoming active.

### 4. Verify New Leader is Producing Blocks

```bash
curl -X POST http://localhost:9645 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

## Configuration File

This example includes a `config.toml` showing multi-sequencer configuration:

```toml
[deployer]
sequencer_count = 3
l2_nodes = 7
# ... other settings
```

Load it with:
```bash
kupcake --config ./config.toml
```

## Cleanup

```bash
kupcake cleanup kup-example-multi-sequencer
```

## Related Documentation

- [Single Sequencer Example](../single-sequencer/)
- [Multi-Sequencer Guide](../../user-guide/multi-sequencer.md)
- [op-conductor Service](../../services/op-conductor.md)
