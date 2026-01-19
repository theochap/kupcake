# Multi-Sequencer Setup Guide

**Target Audience**: Operators | Advanced Users

Kupcake supports deploying multiple sequencers coordinated by op-conductor using Raft consensus.

## Overview

When you deploy more than one sequencer (`--sequencer-count > 1`), Kupcake automatically:
- Deploys op-conductor for coordination
- Configures Raft consensus cluster
- Starts sequencer 1 as initial leader (active)
- Starts other sequencers in stopped state (standby)

## Quick Start

```bash
# Deploy 3 sequencers + 4 validators
kupcake --sequencer-count 3 --l2-nodes 7
```

## Single vs. Multi-Sequencer

### Single Sequencer (`--sequencer-count 1`)

```bash
kupcake --sequencer-count 1 --l2-nodes 3
```

**Characteristics**:
- No op-conductor deployed
- No Raft overhead
- Simpler setup
- Single point of failure

**Use Cases**: Development, testing, resource-constrained environments

### Multi-Sequencer (`--sequencer-count > 1`)

```bash
kupcake --sequencer-count 2 --l2-nodes 5  # Default
```

**Characteristics**:
- op-conductor deployed automatically
- Raft consensus for leader election
- High availability
- Automatic failover

**Use Cases**: Production-like setups, HA testing, understanding consensus

## How It Works

### Raft Consensus

op-conductor implements Raft consensus to coordinate sequencers:

1. **Leader Election**: Sequencers vote for a leader
2. **Single Active Leader**: Only the leader produces blocks
3. **Heartbeat**: Leader sends heartbeats to followers
4. **Automatic Failover**: If leader fails, new election occurs

### Sequencer States

- **Active**: Leader sequencer, produces blocks
- **Stopped**: Follower sequencers, standby mode

### Initial State

When deployment completes:
- **Sequencer 1** (index 0): Leader, **active**
- **Sequencer 2+** (index 1+): Followers, **stopped**

### Leader Failover

If the active sequencer fails:
1. Conductor detects missing heartbeat
2. Initiates leader election
3. Elects new leader from followers
4. New leader becomes active
5. Old leader (if recovered) becomes follower

## Configuration

### CLI Arguments

```bash
# Total nodes
--l2-nodes <COUNT>

# Sequencer count (must be <= l2-nodes)
--sequencer-count <COUNT>
```

**Formula**: `validators = l2_nodes - sequencer_count`

### Examples

```bash
# 2 sequencers + 3 validators (default)
kupcake --sequencer-count 2 --l2-nodes 5

# 3 sequencers + 4 validators
kupcake --sequencer-count 3 --l2-nodes 7

# 1 sequencer + 2 validators (no conductor)
kupcake --sequencer-count 1 --l2-nodes 3
```

## Monitoring op-conductor

### View Conductor Logs

```bash
docker logs -f <network>-op-conductor
```

### Check Leader Status

```bash
docker logs <network>-op-conductor | grep -i "leader"
```

Should show logs about leader election and current leader.

### Check Raft Cluster State

```bash
docker logs <network>-op-conductor | grep -i "raft"
```

## Testing Failover

### 1. Identify Current Leader

```bash
docker logs <network>-op-conductor | grep -i "leader elected"
```

Or check which sequencer is active by querying block production:

```bash
# Sequencer 1 RPC
curl -X POST http://localhost:9545 -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Sequencer 2 RPC
curl -X POST http://localhost:9645 -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

Active sequencer will have incrementing block numbers.

### 2. Stop the Leader

```bash
# Stop both containers for sequencer 1
docker stop <network>-op-reth-sequencer-1
docker stop <network>-kona-node-sequencer-1
```

### 3. Watch Conductor Elect New Leader

```bash
docker logs -f <network>-op-conductor
```

Should see:
- Leader heartbeat timeout
- New election initiated
- New leader elected

### 4. Verify New Leader Producing Blocks

```bash
# Check block production on sequencer 2
watch -n 1 'curl -s -X POST http://localhost:9645 \
  -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_blockNumber\",\"params\":[],\"id\":1}" \
  | jq -r ".result"'
```

### 5. Restart Old Leader

```bash
docker start <network>-op-reth-sequencer-1
docker start <network>-kona-node-sequencer-1
```

Old leader rejoins cluster as a follower.

## Container Naming

Sequencers and validators are numbered starting from 1:

- `<network>-op-reth-sequencer-1` / `<network>-kona-node-sequencer-1`
- `<network>-op-reth-sequencer-2` / `<network>-kona-node-sequencer-2`
- `<network>-op-reth-validator-1` / `<network>-kona-node-validator-1`
- ...

## Port Mappings

Each sequencer gets unique RPC and WS ports:

| Sequencer | RPC Port | WS Port |
|-----------|----------|---------|
| Sequencer 1 | 9545 | 9546 |
| Sequencer 2 | 9645 | 9646 |
| Sequencer 3 | 9745 | 9746 |

Validators follow a similar pattern starting from a higher base.

## Troubleshooting

### Conductor Not Electing Leader

**Check conductor logs**:
```bash
docker logs <network>-op-conductor
```

**Common causes**:
- Sequencers not reachable
- Network issues
- Insufficient sequencers (need at least 1)

### Sequencer Stuck in Stopped State

**Check conductor logs** for election status.

**Manual restart**:
```bash
docker restart <network>-op-conductor
docker restart <network>-op-reth-sequencer-1
docker restart <network>-kona-node-sequencer-1
```

### Multiple Sequencers Active

**Should never happen** (Raft prevents this), but if it does:
```bash
# Stop all sequencers
docker stop $(docker ps -q --filter name="<network>-op-reth-sequencer")
docker stop $(docker ps -q --filter name="<network>-kona-node-sequencer")

# Restart conductor
docker restart <network>-op-conductor

# Start sequencers one at a time
docker start <network>-op-reth-sequencer-1
docker start <network>-kona-node-sequencer-1
# Wait for leader election
sleep 10
docker start <network>-op-reth-sequencer-2
docker start <network>-kona-node-sequencer-2
```

## Production Considerations

### Quorum Requirements

For Raft consensus, you need:
- **Minimum**: 1 sequencer (no HA)
- **Recommended**: 3 sequencers (tolerates 1 failure)
- **High Availability**: 5 sequencers (tolerates 2 failures)

### Resource Usage

Each sequencer adds:
- 2 containers (op-reth + kona-node)
- ~500 MB RAM
- Moderate CPU usage

### Network Latency

Sequencers should have low latency to each other for Raft to work efficiently. In local Docker deployments, latency is negligible.

## Examples

- [Single Sequencer Example](../examples/single-sequencer/)
- [Multi-Sequencer Example](../examples/multi-sequencer/)

## Related Documentation

- [op-conductor Service](../services/op-conductor.md)
- [Architecture: Service Coordination](../architecture/service-coordination.md)
- [CLI Reference](cli-reference.md#--sequencer-count-count)
