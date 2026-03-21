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
dump_state = true
publish_all_ports = false
# override_state = "/path/to/state.json"  # Optional: load external Anvil state (live mode only)

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

### Log Management

The `[docker]` section in the generated `Kupcake.toml` supports log rotation and streaming:

```toml
[docker]
net_name = "kup-my-network-network"
no_cleanup = false
publish_all_ports = false
log_max_size = "10m"       # Docker log file max size
log_max_file = "3"         # Max rotated log files
stream_logs = false        # Stream container logs to tracing output
```

Per-service log levels are stored in their respective sections:

```toml
[anvil]
quiet = true               # Suppress non-essential Anvil output

[[l2_stack.sequencers]]
[l2_stack.sequencers.op_reth]
log_filter = "info"        # op-reth stdout log filter

[l2_stack.sequencers.kona_node]
verbosity = "-vvv"         # kona-node verbosity (-vvv = info, -vvvv = debug)

[l2_stack.op_batcher]
log_level = "INFO"         # op-batcher log level
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

## File Includes

For large networks, you can split `Kupcake.toml` into multiple files using the `include` directive. Any TOML table can reference an external file instead of defining its contents inline:

```toml
# Kupcake.toml — main config references external files
[l2_stack.op_batcher]
include = "./configs/batcher.toml"

[[l2_stack.sequencers]]
include = "./configs/seq-0.toml"

[[l2_stack.sequencers]]
include = "./configs/seq-1.toml"

# Validators can still be inline
[[l2_stack.validators]]
[l2_stack.validators.op_reth]
container_name = "my-net-op-reth-validator-1"
```

The referenced file contains the full TOML content for that section:

```toml
# configs/batcher.toml
container_name = "my-net-op-batcher"
docker_image = { image = "custom-batcher", tag = "v1.0" }
log_level = "INFO"
```

**Rules:**
- Paths are resolved relative to the config file's directory (not the working directory)
- Inline and `include` references can be mixed freely within arrays
- Includes can be nested (an included file can itself use `include`)
- Circular includes are detected and produce an error
- When saving, Kupcake always writes the fully-resolved config (no `include` keys in output)

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
