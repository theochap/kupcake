# L1 Sources Guide

**Target Audience**: Operators | Advanced Users

Kupcake supports multiple L1 sources for the Anvil fork.

## L1 Source Modes

### Local Mode (Default)

**No `--l1` flag** - Run Anvil without forking any chain.

```bash
kupcake
# or explicitly:
kupcake --network my-network
```

**Characteristics**:
- Random L1 chain ID generated
- No external RPC calls
- Fastest startup
- Completely isolated
- No external dependencies

**Use Cases**: Offline development, air-gapped testing, CI/CD

### Sepolia Fork

**`--l1 sepolia`** - Fork Ethereum Sepolia testnet.

```bash
kupcake --l1 sepolia
```

**Characteristics**:
- Chain ID: 11155111
- Public RPC: `https://ethereum-sepolia-rpc.publicnode.com`
- Testnet state available
- Free testnet ETH

**Use Cases**: Testnet integration, realistic L1 environment

### Mainnet Fork

**`--l1 mainnet`** - Fork Ethereum mainnet.

```bash
kupcake --l1 mainnet
```

**Characteristics**:
- Chain ID: 1
- Public RPC: `https://ethereum-rpc.publicnode.com`
- Full mainnet state available
- Realistic gas prices

**Use Cases**: Production-like testing, mainnet contract interaction

### Custom RPC

**`--l1 <RPC_URL>`** - Fork any chain via custom RPC URL.

```bash
kupcake --l1 https://eth-mainnet.g.alchemy.com/v2/YOUR-API-KEY
```

**Characteristics**:
- Chain ID detected via `eth_chainId` RPC call
- Full control over RPC provider
- Can use private RPC endpoints
- Can fork other EVM chains (e.g., Polygon, BSC)

**Use Cases**: Private RPC, rate limit control, non-Ethereum chains

## How Forking Works

When Anvil forks a chain:

1. Connects to the specified RPC URL
2. Fetches latest block number
3. Clones state at that block
4. Continues producing blocks locally
5. All state from the fork point is available

**Fork Point**: Always the latest block when Anvil starts.

## Genesis Timestamp Calculation

By default, the L2 genesis timestamp is automatically calculated:

**For forked chains**:
```
genesis_timestamp = latest_block_timestamp - (block_time × block_number)
```
This ensures the L2 genesis aligns with L1 block 0 time.

**For local mode**:
```
genesis_timestamp = current_unix_timestamp
```

**Manual Override**:

You can manually specify the genesis timestamp using the `--genesis-timestamp` flag:

```bash
kupcake --l1 sepolia --genesis-timestamp 1768464000
```

This is useful for:
- Testing with specific timestamps
- Deterministic deployments
- Reproducing specific blockchain states

## Comparing L1 Source Modes

| Feature | Local Mode | Sepolia Fork | Mainnet Fork | Custom RPC |
|---------|------------|--------------|--------------|------------|
| **External RPC** | No | Yes | Yes | Yes |
| **Startup Speed** | Fast | Medium | Medium | Medium |
| **Chain State** | Empty | Testnet | Mainnet | Custom |
| **Cost** | Free | Free | Free | Varies |
| **Offline** | ✅ | ❌ | ❌ | ❌ |
| **Realistic** | ❌ | ⚠️ | ✅ | ✅ |

## Environment Variables

```bash
export KUP_L1=sepolia
kupcake
```

See [Environment Variables Guide](environment-variables.md) for details.

## Configuration File

In `Kupcake.toml`:

```toml
# Local mode (omit l1_source field entirely)
[deployer]
network_name = "local"

# Sepolia fork
[deployer.l1_source]
Sepolia = []

# Mainnet fork
[deployer.l1_source]
Mainnet = []

# Custom RPC
[deployer.l1_source]
Custom = "https://your-rpc-url"
```

## Public RPC Endpoints

### Sepolia

Default: `https://ethereum-sepolia-rpc.publicnode.com`

Alternatives:
- `https://rpc.sepolia.org`
- `https://ethereum-sepolia.blockpi.network/v1/rpc/public`
- Infura: `https://sepolia.infura.io/v3/YOUR-API-KEY`
- Alchemy: `https://eth-sepolia.g.alchemy.com/v2/YOUR-API-KEY`

### Mainnet

Default: `https://ethereum-rpc.publicnode.com`

Alternatives:
- `https://eth.llamarpc.com`
- `https://ethereum.blockpi.network/v1/rpc/public`
- Infura: `https://mainnet.infura.io/v3/YOUR-API-KEY`
- Alchemy: `https://eth-mainnet.g.alchemy.com/v2/YOUR-API-KEY`

## Rate Limiting

Public RPC endpoints have rate limits. For heavy usage, use:

1. **API Keys**: Sign up for Infura, Alchemy, etc.
2. **Custom RPC**: Run your own node
3. **Local Mode**: No external RPC calls

## Testing L1 Connectivity

```bash
# Test RPC connectivity
curl -X POST https://ethereum-sepolia-rpc.publicnode.com \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Should return latest block number
```

## Verifying Fork Mode

```bash
# Check L1 chain ID
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'

# Local mode: random chain ID
# Sepolia: 0xaa36a7 (11155111)
# Mainnet: 0x1 (1)
```

## Examples

See these example scenarios:
- [Basic Deployment](../examples/basic-deployment/) - Local mode
- [Mainnet Fork](../examples/mainnet-fork/) - Mainnet fork
- [Local Mode](../examples/local-mode/) - Explicit local mode

## Related Documentation

- [CLI Reference](cli-reference.md#--l1-source)
- [Environment Variables](environment-variables.md#kup_l1)
- [Configuration File](configuration-file.md#l1-source-configuration)
