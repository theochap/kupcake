# Fast Blocks Example

**Demonstrates**: Configuring faster block times for development

## What This Example Does

This example configures 1-second block times for rapid iteration:
- L1 (Anvil) produces blocks every 1 second
- L2 derives blocks faster from L1
- Rapid feedback for testing
- Faster transaction finality

## Running the Example

```bash
./run.sh
```

## Block Time Configuration

The `--block-time` flag affects:
- **Anvil** L1 block production interval
- **kona-node** `l1_slot_duration` parameter

### Examples

```bash
# 1 second blocks (this example)
kupcake --block-time 1

# 2 second blocks (fast testing)
kupcake --block-time 2

# 12 second blocks (mainnet-like, default)
kupcake --block-time 12
```

## Use Cases

- Fast iteration during development
- Rapid transaction testing
- CI/CD with quick feedback
- Unit and integration testing

## Tradeoffs

### Pros
- ✅ Faster feedback loops
- ✅ Quicker transaction finality
- ✅ More iterations in less time

### Cons
- ❌ Higher CPU usage
- ❌ More log output
- ❌ Not representative of production block times

## Observing Fast Blocks

### Check Block Production Rate

```bash
# Watch block numbers increase rapidly
watch -n 1 'curl -s -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_blockNumber\",\"params\":[],\"id\":1}" \
  | jq -r ".result"'
```

Should increment every 1-2 seconds.

### Check L2 Derivation

```bash
# Watch L2 block numbers
watch -n 1 'curl -s -X POST http://localhost:9545 \
  -H "Content-Type: application/json" \
  -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_blockNumber\",\"params\":[],\"id\":1}" \
  | jq -r ".result"'
```

## Cleanup

```bash
kupcake cleanup kup-example-fast-blocks
```

## Related Documentation

- [CLI Reference - Block Time](../../user-guide/cli-reference.md#--block-time-seconds)
- [Understanding Output](../../getting-started/understanding-output.md)
