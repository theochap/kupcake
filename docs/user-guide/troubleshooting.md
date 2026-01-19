# Troubleshooting Guide

**Target Audience**: All Users

Common issues and solutions when using Kupcake.

## Docker Issues

### Docker Not Running

```
Error: Cannot connect to the Docker daemon. Is the docker daemon running?
```

**Solution**: Start Docker:

```bash
# Linux (systemd)
sudo systemctl start docker
sudo systemctl enable docker  # Start on boot

# macOS
open -a Docker

# Windows
# Start Docker Desktop from Start Menu
```

### Permission Denied

```
Error: Permission denied while trying to connect to the Docker daemon socket
```

**Solution**: Add your user to the docker group:

```bash
sudo usermod -aG docker $USER
newgrp docker

# Verify
docker ps
```

Or use Docker Desktop which handles permissions automatically.

### Port Already Allocated

```
Error: Bind for 0.0.0.0:8545 failed: port is already allocated
```

**Causes**:
- Another Kupcake deployment is running
- Another service is using the port
- Previous deployment wasn't cleaned up

**Solutions**:

1. Use a different network name:
```bash
kupcake --network my-unique-name
```

2. Find and stop the conflicting process:
```bash
# Linux/macOS
lsof -i :8545
# Kill the process
kill <PID>
```

3. Clean up existing deployment:
```bash
kupcake cleanup <network-name>
```

4. Use `--publish-all-ports` to let Docker choose ports:
```bash
kupcake --publish-all-ports
docker ps  # See assigned ports
```

### Container Fails to Start

**Check logs**:
```bash
docker logs <container-name>
```

**Common causes**:

1. **JWT Mismatch**: Deleted data directory but didn't redeploy
   ```bash
   kupcake cleanup <network-name>
   rm -rf ./data-<network-name>
   kupcake --network <network-name>
   ```

2. **Image Pull Failure**: Check Docker Hub connectivity
   ```bash
   docker pull ghcr.io/op-rs/op-reth:latest
   ```

3. **Insufficient Resources**: Docker resource limits reached
   ```bash
   docker system df  # Check disk usage
   docker system prune  # Clean up unused resources
   ```

## Build Issues

### Rust Compiler Errors

```
error: linker `cc` not found
```

**Solution**: Install build tools:

```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install build-essential pkg-config libssl-dev

# macOS
xcode-select --install

# Arch Linux
sudo pacman -S base-devel openssl
```

### OpenSSL Not Found

```
error: failed to run custom build command for `openssl-sys`
```

**Solution**:

```bash
# Ubuntu/Debian
sudo apt-get install pkg-config libssl-dev

# macOS
brew install openssl
export OPENSSL_DIR=$(brew --prefix openssl)

# Arch Linux
sudo pacman -S openssl
```

### Out of Memory During Build

```
error: could not compile `kupcake` due to previous error
killed
```

**Solution**: Increase available RAM or use fewer parallel jobs:

```bash
cargo build --release -j 2  # Use only 2 CPU cores
```

## Deployment Issues

### Contracts Fail to Deploy

**Check op-deployer logs**:
```bash
docker logs <network>-op-deployer-init
docker logs <network>-op-deployer-apply
```

**Common causes**:

1. **L1 RPC Unreachable**: Check network connectivity
   ```bash
   curl https://ethereum-sepolia-rpc.publicnode.com
   ```

2. **Insufficient Gas**: Anvil accounts may need more ETH (shouldn't happen with Anvil)

3. **Contract Deployment Timeout**: Retry deployment
   ```bash
   kupcake --config ./data-<network>/Kupcake.toml --redeploy
   ```

### Batcher Not Submitting

**Check batcher logs**:
```bash
docker logs <network>-op-batcher
```

**Common causes**:

1. **Sequencer Not Producing Blocks**: Check sequencer logs
   ```bash
   docker logs <network>-op-reth-sequencer-1
   docker logs <network>-kona-node-sequencer-1
   ```

2. **L1 Not Reachable**: Check Anvil is running
   ```bash
   docker logs <network>-anvil
   curl -X POST http://localhost:8545 -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
   ```

### Sequencer Not Producing Blocks

**Check both op-reth and kona-node logs**:
```bash
docker logs <network>-op-reth-sequencer-1
docker logs <network>-kona-node-sequencer-1
```

**Common causes**:

1. **JWT Mismatch**: op-reth and kona-node can't authenticate
   ```bash
   # Check JWT files exist
   ls -la ./data-<network>/l2-stack/jwt-*.hex
   ```

2. **kona-node Can't Reach L1**: Check network connectivity
   ```bash
   docker exec <network>-kona-node-sequencer-1 ping <network>-anvil
   ```

3. **Conductor Hasn't Elected Leader** (multi-sequencer only):
   ```bash
   docker logs <network>-op-conductor | grep -i leader
   ```

## Monitoring Issues

### Grafana Shows No Data

**Wait 30-60 seconds** for initial metrics scrape.

Still no data?

1. Check Prometheus is scraping:
   ```bash
   curl http://localhost:9090/api/v1/targets | jq
   ```

   All targets should show `state: "up"`.

2. Check Prometheus logs:
   ```bash
   docker logs <network>-prometheus
   ```

3. Check service metrics endpoints:
   ```bash
   # op-reth metrics
   curl http://localhost:9001/metrics

   # kona-node metrics
   curl http://localhost:9002/metrics
   ```

### Can't Access Grafana

```
Error: Connection refused at http://localhost:3000
```

**Solutions**:

1. Check Grafana is running:
   ```bash
   docker ps | grep grafana
   ```

2. Check Grafana logs:
   ```bash
   docker logs <network>-grafana
   ```

3. Try a different port if 3000 is taken:
   ```bash
   # Grafana port is fixed, but you can access via Docker IP
   docker inspect <network>-grafana | grep IPAddress
   # Access at http://<ip>:3000
   ```

## Network Issues

### Containers Can't Communicate

**Check Docker network exists**:
```bash
docker network ls | grep <network>-network
```

**Inspect network**:
```bash
docker network inspect <network>-network
```

All containers should be listed under "Containers".

**Test connectivity**:
```bash
docker exec <network>-op-reth-sequencer-1 ping <network>-anvil
```

### RPC Calls Failing

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

**If timeout/connection refused**:

1. Check container is running:
   ```bash
   docker ps | grep <network>-anvil
   ```

2. Check port mapping:
   ```bash
   docker port <network>-anvil
   ```

3. Check container logs:
   ```bash
   docker logs <network>-anvil
   ```

## Data Directory Issues

### Data Directory Locked

```
Error: Cannot access data directory: permission denied
```

**Solution**: Check permissions:

```bash
ls -la ./data-<network>
sudo chown -R $USER:$USER ./data-<network>
```

### Corrupted Data Directory

**Symptoms**: Containers crash on startup, strange errors

**Solution**: Clean up and redeploy:

```bash
kupcake cleanup <network>
rm -rf ./data-<network>
kupcake --network <network>
```

## Performance Issues

### High CPU Usage

**Check resource usage**:
```bash
docker stats
```

**Causes**:
- Fast block times (`--block-time 1`)
- Too many nodes for your system

**Solutions**:
1. Increase block time:
   ```bash
   kupcake --block-time 12
   ```

2. Reduce node count:
   ```bash
   kupcake --sequencer-count 1 --l2-nodes 2
   ```

### High Memory Usage

**Check memory**:
```bash
docker stats --no-stream
```

**Solutions**:
- Increase Docker memory limit (Docker Desktop settings)
- Reduce node count
- Prune old data:
  ```bash
  docker system prune -a
  ```

### Slow Performance

**Disk I/O** is often the bottleneck.

**Solutions**:
1. Use SSD for data directory
2. Use temp directory:
   ```bash
   kupcake --outdata /tmp/kupcake-data
   ```

## Getting Help

If you're still stuck:

1. **Check GitHub Issues**: https://github.com/op-rs/kupcake/issues
2. **Create a New Issue**: Include:
   - Kupcake version (`kupcake --version`)
   - Docker version (`docker --version`)
   - OS/Platform
   - Full error message
   - Steps to reproduce
   - Relevant container logs

3. **Include Logs**:
   ```bash
   # Collect all logs
   docker logs <network>-anvil > anvil.log 2>&1
   docker logs <network>-op-reth-sequencer-1 > op-reth.log 2>&1
   docker logs <network>-kona-node-sequencer-1 > kona-node.log 2>&1
   ```

## Related Documentation

- [Installation Guide](../getting-started/installation.md)
- [CLI Reference](cli-reference.md)
- [Understanding Output](../getting-started/understanding-output.md)
