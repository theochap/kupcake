# Port Management Guide

**Target Audience**: Operators

Understanding and managing port mappings in Kupcake deployments.

## Default Port Mappings

Kupcake exposes these ports by default:

| Service | Container Port | Host Port | Purpose |
|---------|----------------|-----------|---------|
| Anvil (L1) | 8545 | 8545 | L1 RPC |
| Sequencer 1 RPC | 8545 | 9545 | L2 RPC |
| Sequencer 1 WS | 8546 | 9546 | L2 WebSocket |
| Sequencer 2 RPC | 8545 | 9645 | L2 RPC |
| Sequencer 2 WS | 8546 | 9646 | L2 WebSocket |
| Validator 1 RPC | 8545 | 9745 | L2 RPC (read-only) |
| Prometheus | 9090 | 9090 | Metrics API |
| Grafana | 3000 | 3000 | Dashboards |

## Port Allocation Pattern

### Sequencers

Sequencers increment by 100:
- Sequencer 1: 9545 (RPC), 9546 (WS)
- Sequencer 2: 9645 (RPC), 9646 (WS)
- Sequencer 3: 9745 (RPC), 9746 (WS)

### Validators

Validators continue the pattern:
- Validator 1: 9845 (RPC), 9846 (WS)
- Validator 2: 9945 (RPC), 9946 (WS)
- ...

## Random Port Publishing

Use `--publish-all-ports` to let Docker assign random host ports:

```bash
kupcake --publish-all-ports
```

**Check assigned ports**:
```bash
docker ps --filter name=<network>
# or
docker port <container-name>
```

**Use case**: Avoid port conflicts when running multiple deployments.

## Internal vs. External Ports

### Internal (Docker Network)

All containers communicate using Docker's internal network:
- Container names as hostnames
- Container ports (e.g., 8545)
- No host mapping needed

**Example**: kona-node connects to Anvil at `<network>-anvil:8545`

### External (Host Access)

Services expose ports to the host for external access:
- RPC endpoints for wallets
- Grafana dashboards
- Prometheus API

## Accessing Services

### L1 RPC (Anvil)

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

### L2 RPC (Sequencer 1)

```bash
curl -X POST http://localhost:9545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'
```

### L2 WebSocket (Sequencer 1)

```bash
wscat -c ws://localhost:9546
```

### Grafana

```
http://localhost:3000
```

### Prometheus

```
http://localhost:9090
```

## Port Conflicts

### Symptoms

```
Error: Bind for 0.0.0.0:8545 failed: port is already allocated
```

### Solutions

1. **Use a different network name**:
   ```bash
   kupcake --network unique-name
   ```

2. **Clean up existing deployment**:
   ```bash
   kupcake cleanup <old-network>
   ```

3. **Use random ports**:
   ```bash
   kupcake --publish-all-ports
   ```

4. **Find and stop conflicting process**:
   ```bash
   # Linux/macOS
   lsof -i :8545
   kill <PID>

   # Windows
   netstat -ano | findstr :8545
   taskkill /PID <PID> /F
   ```

## Firewall Configuration

If accessing from other machines, open these ports in your firewall:

```bash
# Ubuntu/Debian (ufw)
sudo ufw allow 8545/tcp  # L1 RPC
sudo ufw allow 9545/tcp  # L2 RPC
sudo ufw allow 3000/tcp  # Grafana

# CentOS/RHEL (firewalld)
sudo firewall-cmd --add-port=8545/tcp --permanent
sudo firewall-cmd --add-port=9545/tcp --permanent
sudo firewall-cmd --add-port=3000/tcp --permanent
sudo firewall-cmd --reload
```

## MetaMask Configuration

Add custom L2 network to MetaMask:

- **Network Name**: Kupcake L2
- **RPC URL**: `http://localhost:9545` (or your host IP)
- **Chain ID**: Your L2 chain ID
- **Currency Symbol**: ETH

## Docker Port Inspection

### List All Port Mappings

```bash
docker ps --format "table {{.Names}}\t{{.Ports}}"
```

### Inspect Specific Container

```bash
docker port <container-name>
```

**Example output**:
```
8545/tcp -> 0.0.0.0:9545
8546/tcp -> 0.0.0.0:9546
```

## Security Considerations

### Local Development

Default port mappings bind to `0.0.0.0` (all interfaces), making services accessible from other machines.

### Production

Bind only to localhost:
- Modify container configuration to use `127.0.0.1:8545` instead of `0.0.0.0:8545`
- Use reverse proxy (nginx, Caddy) for external access
- Enable authentication on Grafana

## Custom Port Mappings

Currently, Kupcake uses fixed port mappings. To customize:

1. **Use `--publish-all-ports`** and note assigned ports
2. **Modify deployment code** to use custom ports (see Developer Guide)

## Related Documentation

- [Understanding Output](../getting-started/understanding-output.md#port-mappings)
- [Troubleshooting](troubleshooting.md#port-already-allocated)
- [Docker Networking](../architecture/docker-networking.md)
