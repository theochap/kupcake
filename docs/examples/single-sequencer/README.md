# Single Sequencer Example

**Demonstrates**: Deploying with only one sequencer (no op-conductor)

## What This Example Does

This example deploys a simpler network with a single sequencer:
- 1 sequencer node
- 2 validator nodes
- No op-conductor (not needed for single sequencer)
- Lower resource usage

## Running the Example

```bash
./run.sh
```

## What Gets Deployed

| Component | Count | Notes |
|-----------|-------|-------|
| Anvil (L1) | 1 | Local L1 |
| op-reth | 3 | 1 sequencer + 2 validators |
| kona-node | 3 | Paired with op-reth |
| op-batcher | 1 | Batches transactions |
| op-proposer | 1 | Proposes state roots |
| op-challenger | 1 | Challenges proposals |
| op-conductor | 0 | **Not deployed** (single sequencer) |
| Prometheus | 1 | Metrics |
| Grafana | 1 | Dashboards |

**Total**: ~12 containers (vs. ~15 for multi-sequencer)

## Use Cases

- Local development (fewer resources)
- Simple testing scenarios
- CI/CD pipelines (faster startup)
- Learning OP Stack architecture

## Differences from Multi-Sequencer

- No op-conductor container
- No Raft consensus overhead
- Single point of failure (no HA)
- Simpler logs and debugging

## Cleanup

```bash
kupcake cleanup kup-example-single-sequencer
```

## Related Documentation

- [Multi-Sequencer Example](../multi-sequencer/)
- [Multi-Sequencer Guide](../../user-guide/multi-sequencer.md)
