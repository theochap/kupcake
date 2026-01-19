# Local Mode Example

**Demonstrates**: Running without any L1 fork (local Anvil only)

## What This Example Does

This example runs in fully local mode:
- No L1 fork (Anvil with random chain ID)
- Completely isolated from public networks
- Minimal resource usage
- No external RPC dependencies

## Running the Example

```bash
./run.sh
```

## Local Mode vs. Fork Mode

### Local Mode (This Example)
- **No `--l1` flag** (or `--l1` omitted)
- Random L1 chain ID generated
- No external RPC calls
- Fastest startup
- Completely offline

### Fork Mode
- `--l1 sepolia` or `--l1 mainnet`
- Forks public chain state
- Requires internet connection
- Slower startup (downloads state)

## Use Cases

- Air-gapped development
- Offline testing
- CI/CD without external dependencies
- Minimal resource consumption
- Learning OP Stack without needing testnet access

## Verifying Local Mode

### Check L1 Chain ID (Random)

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'
```

Should return a random chain ID (not 1 or 11155111).

### Check No External Connections

```bash
# Check network traffic - should be minimal
docker stats kup-example-local-anvil --no-stream
```

No significant network I/O (only Docker network).

## Benefits

- ✅ Fully isolated
- ✅ No external dependencies
- ✅ Fastest startup
- ✅ Lowest resource usage
- ✅ Reproducible (no external state changes)

## Cleanup

```bash
kupcake cleanup kup-example-local
```

## Related Documentation

- [L1 Sources Guide](../../user-guide/l1-sources.md)
- [CLI Reference](../../user-guide/cli-reference.md#--l1-source)
