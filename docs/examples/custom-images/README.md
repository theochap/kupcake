# Custom Images Example

**Demonstrates**: Using custom Docker images for all services

## What This Example Does

This example shows how to override default Docker images:
- Use custom image registries
- Pin specific image tags
- Test development builds
- Use private registries

## Running the Example

```bash
./run.sh
```

## Configuration Methods

### Method 1: Command Line Arguments

```bash
kupcake \
  --op-reth-image myregistry.io/op-reth \
  --op-reth-tag v1.0.0 \
  --kona-node-image myregistry.io/kona \
  --kona-node-tag v0.5.0
```

### Method 2: Environment Variables

```bash
export KUP_OP_RETH_IMAGE=myregistry.io/op-reth
export KUP_OP_RETH_TAG=v1.0.0
export KUP_KONA_NODE_IMAGE=myregistry.io/kona
export KUP_KONA_NODE_TAG=v0.5.0
kupcake
```

### Method 3: .env File

This example includes `.env.example`:

```bash
cp .env.example .env
# Edit .env with your custom images
source .env
kupcake
```

## Included .env.example

Shows all available image overrides:
- Anvil (L1)
- op-reth (L2 execution)
- kona-node (L2 consensus)
- op-batcher, op-proposer, op-challenger
- op-conductor
- op-deployer
- Prometheus, Grafana

## Use Cases

- Testing custom builds
- Using specific versions
- Private registry access
- Development workflows

## Verifying Custom Images

```bash
# Check running images
docker ps --filter name=kup-example-custom-images --format "table {{.Names}}\t{{.Image}}"
```

## Cleanup

```bash
kupcake cleanup kup-example-custom-images
```

## Related Documentation

- [Docker Images Guide](../../user-guide/docker-images.md)
- [CLI Reference](../../user-guide/cli-reference.md#docker-image-overrides)
