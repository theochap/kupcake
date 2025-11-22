# Grafana Dashboard Setup

This directory contains Grafana configuration and dashboards for the reth-kona stack.

## Directory Structure

```
grafana/
├── datasources/
│   └── prometheus.yml          # Prometheus datasource configuration
└── dashboards/
    ├── dashboard.yml           # Dashboard provisioning configuration
    ├── reth-overview.json      # Main Reth metrics dashboard
    ├── reth-performance.json   # Reth performance metrics
    ├── consensus-summary.json  # Consensus layer summary
    ├── consensus-network.json  # Network metrics
    └── consensus-beacon-processing.json  # Beacon chain processing
```

## How It Works

When Grafana starts, it automatically:

1. **Loads the datasource** from `datasources/prometheus.yml`
   - Connects to Prometheus at `http://prometheus:9090`
   - Sets it as the default datasource

2. **Provisions dashboards** from `dashboards/*.json`
   - All JSON files in this directory are automatically imported
   - Changes to files require Grafana restart to reload

## Accessing Grafana

- **URL**: http://localhost:3001 (or your configured `GRAFANA_PORT`)
- **Default login**: `admin` / `admin`
- **First login**: You'll be prompted to change the password

## Adding New Dashboards

### Method 1: Add JSON File (Automatic)

1. Export a dashboard as JSON from Grafana UI
2. Save it in `grafana/dashboards/` directory
3. Restart Grafana:
   ```bash
   docker compose restart grafana
   # or
   ./stack.sh restart
   ```

### Method 2: Import via Grafana UI

1. Go to Grafana → Dashboards → Import
2. Upload JSON file or paste JSON content
3. Select "Prometheus" as the datasource
4. Click "Import"

**Note**: Dashboards imported via UI won't persist if you remove volumes

### Method 3: Import from Grafana.com

1. Go to Grafana → Dashboards → Import
2. Enter a dashboard ID from https://grafana.com/grafana/dashboards/
3. Select "Prometheus" as datasource
4. Click "Load" then "Import"

## Available Dashboards

### Reth Dashboards
- **reth-overview.json** - Complete overview of Reth metrics
- **reth-performance.json** - Performance and resource usage

### Consensus Layer Dashboards
- **consensus-summary.json** - High-level consensus metrics
- **consensus-network.json** - P2P network statistics
- **consensus-beacon-processing.json** - Beacon chain processing details

## Useful Dashboard IDs from Grafana.com

You can import these popular Ethereum dashboards:

- **14053** - Ethereum Node Dashboard
- **13457** - Prometheus 2.0 Stats
- **1860** - Node Exporter Full
- **3662** - Prometheus 2.0 Overview

## Customizing Dashboards

### Edit in Grafana UI
1. Open a dashboard
2. Click the gear icon (⚙️) → Settings
3. Make your changes
4. Save

### Export and Save
1. Click the share icon → Export → Save to file
2. Save the JSON to `grafana/dashboards/`
3. Restart Grafana to make it permanent

## Dashboard Variables

Most dashboards support these variables (configurable in UI):

- **instance** - Select specific node (`reth`, `kona`)
- **job** - Prometheus job name
- **interval** - Time interval for aggregation

## Troubleshooting

### Dashboards not appearing
```bash
# Check Grafana logs
docker logs reth-kona-grafana

# Verify dashboard files exist
ls -la grafana/dashboards/

# Restart Grafana
docker compose restart grafana
```

### "No data" in panels
- Wait 1-2 minutes for Prometheus to scrape metrics
- Check Prometheus is scraping: http://localhost:9091/targets
- Verify nodes are running: `docker compose ps`
- Check datasource: Grafana → Configuration → Data Sources → Prometheus → Test

### Wrong datasource
If panels show "default" instead of "Prometheus":
1. Open dashboard settings
2. Go to JSON Model
3. Find and replace: `"datasource": "default"` → `"datasource": "Prometheus"`
4. Save

## Prometheus Queries

Some useful queries to use in Grafana:

```promql
# Current block number (Reth)
reth_sync_block_height{instance="reth"}

# Sync percentage
rate(reth_sync_block_height[5m])

# Memory usage
process_resident_memory_bytes{job="op-reth"}

# Network peers
net_peers{job="kona-node"}

# Transaction pool size
txpool_pending{job="op-reth"}
```

## Best Practices

1. **Version control dashboards** - Save JSON files to git
2. **Use variables** - Make dashboards reusable across instances
3. **Set proper refresh rates** - Don't overload Prometheus (15-30s recommended)
4. **Organize with folders** - Use the dashboard.yml to set folders
5. **Document custom panels** - Add descriptions to panels

## Resources

- [Grafana Provisioning Docs](https://grafana.com/docs/grafana/latest/administration/provisioning/)
- [Prometheus Query Examples](https://prometheus.io/docs/prometheus/latest/querying/examples/)
- [Grafana Dashboard Best Practices](https://grafana.com/docs/grafana/latest/dashboards/build-dashboards/best-practices/)

