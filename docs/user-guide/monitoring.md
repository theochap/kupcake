# Monitoring Guide

**Target Audience**: Operators

Kupcake deploys Prometheus and Grafana for comprehensive metrics collection and visualization.

## Quick Access

### Grafana Dashboards

```
http://localhost:3000
```

**Default credentials**: `admin` / `admin` (you'll be prompted to change on first login)

### Prometheus UI

```
http://localhost:9090
```

## What Gets Monitored

All services expose Prometheus metrics:

| Service | Metrics Port | Metrics Endpoint |
|---------|--------------|------------------|
| Anvil | 9000 | /metrics |
| op-reth (all nodes) | 9001+ | /metrics |
| kona-node (all nodes) | 9002+ | /metrics |
| op-batcher | 7300 | /metrics |
| op-proposer | 7300 | /metrics |
| op-challenger | 7300 | /metrics |
| op-conductor | 8080 | /metrics |

## Viewing Metrics

### Grafana Dashboards

1. Open http://localhost:3000
2. Login with `admin` / `admin`
3. Navigate to **Dashboards** → **Browse**
4. Select an OP Stack dashboard

**Pre-configured dashboards**:
- OP Stack Overview
- L1 (Anvil) Metrics
- L2 Sequencer Metrics
- L2 Validator Metrics
- Batcher & Proposer Metrics
- op-conductor Raft Metrics

### Prometheus Queries

#### Check Scrape Targets

```
http://localhost:9090/targets
```

All targets should show status `UP`.

#### Example Queries

**L1 Block Height**:
```promql
anvil_block_number
```

**L2 Block Height** (sequencer 1):
```promql
op_reth_block_number{instance="<network>-op-reth-sequencer-1:9001"}
```

**Batcher Batch Submissions**:
```promql
rate(op_batcher_batches_submitted_total[5m])
```

**Proposer State Root Submissions**:
```promql
rate(op_proposer_proposals_submitted_total[5m])
```

## Metrics Configuration

### Prometheus Configuration

Prometheus scrape configuration is auto-generated at:
```
./data-<network>/monitoring/prometheus.yml
```

**Default scrape interval**: 15 seconds

### Adding Custom Scrape Targets

Edit `prometheus.yml` and restart Prometheus:

```yaml
scrape_configs:
  - job_name: 'my-custom-service'
    static_configs:
      - targets: ['my-service:9090']
```

```bash
docker restart <network>-prometheus
```

### Grafana Configuration

Grafana data and dashboards are stored in:
```
./data-<network>/monitoring/grafana/
```

Prometheus is pre-configured as a data source.

## Common Metrics

### L1 Metrics (Anvil)

- `anvil_block_number` - Current L1 block height
- `anvil_gas_used` - Gas used per block
- `anvil_transactions` - Transactions per block

### L2 Metrics (op-reth)

- `op_reth_block_number` - Current L2 block height
- `op_reth_gas_used` - Gas used per L2 block
- `op_reth_transaction_count` - Transactions processed
- `op_reth_sync_status` - Sync status (sequencer vs. validator)

### Consensus Metrics (kona-node)

- `kona_l1_block_derived` - Latest L1 block derived
- `kona_l2_block_produced` - L2 blocks produced from L1 data
- `kona_derivation_errors` - Derivation pipeline errors

### Batcher Metrics

- `op_batcher_batches_submitted` - Total batches submitted to L1
- `op_batcher_batch_size` - Size of batches (bytes)
- `op_batcher_gas_used` - Gas used for batch submissions

### Proposer Metrics

- `op_proposer_proposals_submitted` - Total state root proposals
- `op_proposer_proposal_interval` - Time between proposals
- `op_proposer_gas_used` - Gas used for proposals

### Conductor Metrics (Multi-Sequencer)

- `op_conductor_leader_id` - Current Raft leader ID
- `op_conductor_cluster_size` - Number of sequencers in cluster
- `op_conductor_raft_state` - Raft cluster state

## Alerts

Prometheus supports alerting rules. Create `alerts.yml`:

```yaml
groups:
  - name: kupcake_alerts
    rules:
      - alert: L1Stopped
        expr: rate(anvil_block_number[1m]) == 0
        for: 1m
        annotations:
          summary: "L1 (Anvil) has stopped producing blocks"

      - alert: SequencerStopped
        expr: rate(op_reth_block_number{role="sequencer"}[1m]) == 0
        for: 2m
        annotations:
          summary: "Sequencer has stopped producing blocks"
```

Add to `prometheus.yml`:
```yaml
rule_files:
  - "/etc/prometheus/alerts.yml"
```

## Troubleshooting

### No Data in Grafana

1. **Wait 30-60 seconds** for initial scrape
2. Check Prometheus targets: http://localhost:9090/targets
3. Verify all targets show `UP`
4. Check Prometheus logs:
   ```bash
   docker logs <network>-prometheus
   ```

### Metrics Endpoint Unreachable

```bash
# Test direct access
curl http://localhost:9001/metrics
```

If unreachable:
- Check container is running
- Check port mapping with `docker ps`
- Check container logs

### Grafana Login Issues

**Reset admin password**:
```bash
docker exec -it <network>-grafana grafana-cli admin reset-admin-password newpassword
```

## Advanced Usage

### Custom Dashboards

1. Create dashboard in Grafana UI
2. Export as JSON
3. Save to `./grafana/dashboards/`
4. Import on next deployment

### External Prometheus

To use an external Prometheus instance:

1. Copy scrape configs from `prometheus.yml`
2. Add to your external Prometheus config
3. Update Grafana data source to point to external Prometheus

### Long-Term Storage

Prometheus stores metrics in:
```
./data-<network>/monitoring/grafana/prometheus-data/
```

**Default retention**: 15 days

**Increase retention**:
Edit docker run command to add:
```
--storage.tsdb.retention.time=30d
```

## Related Documentation

- [Prometheus Service](../services/prometheus.md)
- [Grafana Service](../services/grafana.md)
- [Understanding Output](../getting-started/understanding-output.md#monitoring-prometheus--grafana)
