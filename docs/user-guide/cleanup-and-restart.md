# Cleanup and Restart Guide

**Target Audience**: All Users

Managing the lifecycle of Kupcake deployments.

## Normal Shutdown

### Foreground Mode (Default)

Press `Ctrl+C` to stop Kupcake.

**What happens**:
1. ✅ All containers stopped
2. ✅ All containers removed
3. ✅ Docker network removed
4. ✅ Data directory preserved

**Example**:
```bash
kupcake --network my-network
# Press Ctrl+C
# Cleanup happens automatically
```

### Detached Mode

If you ran with `--detach`, use the cleanup command:

```bash
kupcake cleanup <network-name>
```

**Example**:
```bash
kupcake --network my-network --detach
# Later:
kupcake cleanup my-network
```

## Keep Containers Running

### Using `--no-cleanup` Flag

```bash
kupcake --no-cleanup --network my-network
# Press Ctrl+C
# Containers keep running
```

**What happens**:
- ❌ Containers keep running
- ❌ Network remains active
- ✅ Data directory intact

**Use cases**: Debugging, manual inspection, keeping network alive

### Manual Cleanup

```bash
# Stop all containers
docker stop $(docker ps -q --filter name=my-network)

# Remove all containers
docker rm $(docker ps -aq --filter name=my-network)

# Remove network
docker network rm my-network-network
```

## Cleanup Command

### Basic Usage

```bash
kupcake cleanup <network-name>
```

**What it does**:
1. Stops all containers with names starting with `<network-name>`
2. Removes all stopped containers
3. Removes the Docker network `<network-name>-network`
4. **Does NOT** delete the data directory

### Examples

```bash
# Cleanup specific network
kupcake cleanup my-testnet

# Cleanup default network
kupcake cleanup kup-sepolia-42069
```

## Data Directory Management

### Location

Data is stored in:
```
./data-<network-name>/
```

### Contents

```
./data-<network-name>/
├── Kupcake.toml              # Saved configuration
├── anvil/                    # L1 data
├── l2-stack/                 # L2 and contract data
└── monitoring/               # Prometheus and Grafana data
```

### Preserving Data

Data directory is **always preserved** during cleanup.

To resume with the same data:
```bash
kupcake --config ./data-my-network/Kupcake.toml
```

### Deleting Data

To completely remove a deployment:

```bash
# 1. Cleanup containers
kupcake cleanup my-network

# 2. Delete data directory
rm -rf ./data-my-network
```

**Warning**: This is irreversible!

## Restarting a Deployment

### Resume with Same Configuration

```bash
kupcake --config ./data-my-network/Kupcake.toml
```

Resumes using saved configuration. Contracts are NOT redeployed (unless `--redeploy`).

### Resume and Redeploy Contracts

```bash
kupcake --config ./data-my-network/Kupcake.toml --redeploy
```

Redeploys all contracts, resets L2 state.

### Resume with Modified Settings

```bash
kupcake --config ./data-my-network/Kupcake.toml --block-time 1
```

Overrides specific settings from config file.

### Fresh Start

```bash
# Cleanup old deployment
kupcake cleanup my-network
rm -rf ./data-my-network

# Start fresh
kupcake --network my-network
```

## Partial Cleanup

### Stop Specific Containers

```bash
# Stop only Grafana
docker stop my-network-grafana

# Stop all sequencers
docker stop $(docker ps -q --filter name=my-network-op-reth-sequencer)
```

### Remove Specific Containers

```bash
docker rm my-network-grafana
```

### Restart Specific Containers

```bash
docker restart my-network-op-batcher
```

## Troubleshooting

### Cleanup Fails

```
Error: Cannot remove network: network has active endpoints
```

**Solution**: Manually stop all containers first:

```bash
docker stop $(docker ps -q --filter name=my-network)
docker rm $(docker ps -aq --filter name=my-network)
docker network rm my-network-network
```

### Data Directory Locked

```
Error: Permission denied
```

**Solution**: Fix permissions:

```bash
sudo chown -R $USER:$USER ./data-my-network
```

### Containers Won't Stop

```bash
# Force stop
docker kill $(docker ps -q --filter name=my-network)

# Force remove
docker rm -f $(docker ps -aq --filter name=my-network)
```

## Best Practices

### Development

```bash
# Quick iterations with cleanup
kupcake --network dev
# Press Ctrl+C when done
```

### Testing

```bash
# Detached mode for automated testing
kupcake --network ci-test --detach
# Run tests...
kupcake cleanup ci-test
rm -rf ./data-ci-test
```

### Long-Running

```bash
# Keep containers running
kupcake --no-cleanup --network prod-test
# Press Ctrl+C
# Manually stop when needed
docker stop $(docker ps -q --filter name=prod-test)
```

### Debugging

```bash
# No cleanup for inspection
kupcake --no-cleanup --network debug -v debug
# Press Ctrl+C
# Inspect logs
docker logs debug-anvil
docker logs debug-op-reth-sequencer-1
# Cleanup when done
kupcake cleanup debug
```

## CI/CD Integration

### Example GitHub Actions

```yaml
- name: Deploy Kupcake
  run: kupcake --network ci-${GITHUB_RUN_ID} --detach

- name: Run Tests
  run: pytest tests/

- name: Cleanup
  if: always()
  run: |
    kupcake cleanup ci-${GITHUB_RUN_ID}
    rm -rf ./data-ci-${GITHUB_RUN_ID}
```

### Example GitLab CI

```yaml
deploy:
  script:
    - kupcake --network ci-${CI_PIPELINE_ID} --detach
    - pytest tests/
  after_script:
    - kupcake cleanup ci-${CI_PIPELINE_ID}
    - rm -rf ./data-ci-${CI_PIPELINE_ID}
```

## Related Documentation

- [Quickstart](../getting-started/quickstart.md#stopping-and-cleaning-up)
- [CLI Reference](cli-reference.md#--no-cleanup)
- [Troubleshooting](troubleshooting.md)
