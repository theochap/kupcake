# Mainnet Fork Example

**Demonstrates**: Forking Ethereum mainnet for realistic testing

## What This Example Does

This example forks Ethereum mainnet to create a realistic testing environment:
- Forks Ethereum mainnet (chain ID 1)
- Uses public RPC endpoint
- Custom L2 chain ID (42069)
- All mainnet contract state available on L1

## Running the Example

```bash
./run.sh
```

## What Gets Deployed

Same as basic deployment, but with mainnet state:
- L1 fork of Ethereum mainnet at latest block
- Access to all mainnet contracts and state
- Realistic gas prices and behavior

## Use Cases

- Testing interactions with mainnet contracts
- Realistic gas price simulation
- Integration testing with production contract state
- Validating L1-L2 bridge behavior

## Verifying the Deployment

### Check L1 Chain ID

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'
```

Should return `0x1` (mainnet chain ID).

### Check Mainnet State

Query a known mainnet contract:

```bash
# Check USDC contract (mainnet)
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getCode",
    "params":["0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", "latest"],
    "id":1
  }'
```

Should return contract bytecode.

## Cleanup

```bash
kupcake cleanup kup-example-mainnet
```

## Related Documentation

- [L1 Sources Guide](../../user-guide/l1-sources.md)
- [Understanding Output](../../getting-started/understanding-output.md)
