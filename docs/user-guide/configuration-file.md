# Configuration File (Kupcake.toml)

**Target Audience**: Operators | Advanced Users

Kupcake saves deployment configuration to `Kupcake.toml` in the output data directory. This file can be reloaded to resume or modify deployments.

## Automatic Saving

After every deployment, Kupcake saves your configuration:

```bash
kupcake --network my-network --l1 sepolia --l2-chain 42069
```

Saves to: `./data-my-network/Kupcake.toml`

## Loading Configuration

```bash
kupcake --config ./data-my-network/Kupcake.toml
```

This reloads all settings from the saved configuration.

## Overriding Config Values

CLI arguments override config file values:

```bash
kupcake --config ./saved.toml --block-time 1
# Uses saved.toml but changes block_time to 1
```

## File Structure

```toml
[deployer]
network_name = "my-network"
l1_chain_id = 11155111          # Sepolia
l2_chain_id = 42069
block_time = 12
l2_nodes = 5
sequencer_count = 2
redeploy = false
no_cleanup = false
detach = false
publish_all_ports = false

[deployer.l1_source]
# Sepolia fork
Sepolia = []

# OR Mainnet fork
# Mainnet = []

# OR Custom RPC
# Custom = "https://your-rpc-url"

# OR Local mode (no fork)
# (field omitted entirely)

[deployer.outdata]
# Path to data directory
Path = "./data-my-network"

[deployer.docker_images]
anvil_image = "ghcr.io/foundry-rs/foundry"
anvil_tag = "latest"
op_reth_image = "ghcr.io/op-rs/op-reth"
op_reth_tag = "latest"
kona_node_image = "ghcr.io/op-rs/kona"
kona_node_tag = "latest"
op_batcher_image = "ghcr.io/ethereum-optimism/op-batcher"
op_batcher_tag = "latest"
op_proposer_image = "ghcr.io/ethereum-optimism/op-proposer"
op_proposer_tag = "latest"
op_challenger_image = "ghcr.io/ethereum-optimism/op-challenger"
op_challenger_tag = "latest"
op_conductor_image = "ghcr.io/ethereum-optimism/op-conductor"
op_conductor_tag = "latest"
op_deployer_image = "ghcr.io/ethereum-optimism/op-deployer"
op_deployer_tag = "latest"
prometheus_image = "prom/prometheus"
prometheus_tag = "latest"
grafana_image = "grafana/grafana"
grafana_tag = "latest"
```

## Use Cases

### Resume a Deployment

```bash
kupcake --config ./data-my-network/Kupcake.toml
```

Resumes with identical settings.

### Modify and Redeploy

```bash
kupcake --config ./data-my-network/Kupcake.toml --block-time 1 --redeploy
```

### Share Configurations

```bash
# Save config
cp ./data-my-network/Kupcake.toml team-config.toml

# Share with team
# Others can deploy with identical settings:
kupcake --config team-config.toml --network team-test
```

### Template Configurations

Create reusable templates:

```bash
# fast-dev.toml
[deployer]
network_name = "dev"
block_time = 1
l2_nodes = 2
sequencer_count = 1

# production-like.toml
[deployer]
network_name = "prod-test"
l1_chain_id = 1  # Mainnet
block_time = 12
l2_nodes = 7
sequencer_count = 3
```

## Manual Editing

You can manually edit `Kupcake.toml`:

```bash
vim ./data-my-network/Kupcake.toml
# Edit values
kupcake --config ./data-my-network/Kupcake.toml
```

**Warning**: Invalid TOML will cause errors. Validate your changes.

## L1 Source Configuration

The `l1_source` field uses TOML enum syntax:

```toml
# Sepolia fork
[deployer.l1_source]
Sepolia = []

# Mainnet fork
[deployer.l1_source]
Mainnet = []

# Custom RPC URL
[deployer.l1_source]
Custom = "https://eth-mainnet.g.alchemy.com/v2/YOUR-KEY"
```

For local mode (no fork), **omit the `l1_source` field entirely**.

## Common Modifications

### Change Block Time

```toml
[deployer]
block_time = 2  # Changed from 12
```

### Upgrade Docker Images

```toml
[deployer.docker_images]
op_reth_tag = "v1.1.0"  # Changed from latest
kona_node_tag = "v0.6.0"
```

### Add More Sequencers

```toml
[deployer]
sequencer_count = 3  # Changed from 2
l2_nodes = 7         # Increased to match (3 seq + 4 val)
```

### Switch to Mainnet Fork

```toml
[deployer.l1_source]
Mainnet = []  # Changed from Sepolia
```

## Validation

Kupcake validates the configuration file on load:

- Required fields must be present
- Values must be valid types
- Sequencer count must be ≤ l2_nodes

**Example error**:
```
Error: Invalid configuration: sequencer_count (3) cannot exceed l2_nodes (2)
```

## Related Documentation

- [CLI Reference](cli-reference.md) - All configuration options
- [Environment Variables](environment-variables.md) - Alternative configuration method
- [Multi-Sequencer Example](../examples/multi-sequencer/config.toml) - Example config file
