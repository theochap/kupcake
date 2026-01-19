# Docker Images Guide

**Target Audience**: Operators | Advanced Users

Customizing Docker images for all Kupcake services.

## Default Images

Kupcake uses these default images:

| Service | Image | Tag |
|---------|-------|-----|
| Anvil | `ghcr.io/foundry-rs/foundry` | `latest` |
| op-reth | `ghcr.io/op-rs/op-reth` | `latest` |
| kona-node | `ghcr.io/op-rs/kona` | `latest` |
| op-batcher | `ghcr.io/ethereum-optimism/op-batcher` | `latest` |
| op-proposer | `ghcr.io/ethereum-optimism/op-proposer` | `latest` |
| op-challenger | `ghcr.io/ethereum-optimism/op-challenger` | `latest` |
| op-conductor | `ghcr.io/ethereum-optimism/op-conductor` | `latest` |
| op-deployer | `ghcr.io/ethereum-optimism/op-deployer` | `latest` |
| Prometheus | `prom/prometheus` | `latest` |
| Grafana | `grafana/grafana` | `latest` |

## Overriding Images

### Method 1: CLI Arguments

```bash
kupcake \
  --op-reth-image ghcr.io/op-rs/op-reth \
  --op-reth-tag v1.0.0 \
  --kona-node-image ghcr.io/op-rs/kona \
  --kona-node-tag v0.5.0
```

### Method 2: Environment Variables

```bash
export KUP_OP_RETH_IMAGE=ghcr.io/op-rs/op-reth
export KUP_OP_RETH_TAG=v1.0.0
export KUP_KONA_NODE_IMAGE=ghcr.io/op-rs/kona
export KUP_KONA_NODE_TAG=v0.5.0
kupcake
```

### Method 3: .env File

```bash
# .env
KUP_OP_RETH_IMAGE=ghcr.io/op-rs/op-reth
KUP_OP_RETH_TAG=v1.0.0
KUP_KONA_NODE_IMAGE=ghcr.io/op-rs/kona
KUP_KONA_NODE_TAG=v0.5.0
```

```bash
source .env && kupcake
```

See the [Custom Images Example](../examples/custom-images/) for a complete `.env.example`.

## Use Cases

### Pin Specific Versions

Ensure reproducible deployments:

```bash
kupcake \
  --op-reth-tag v1.0.0 \
  --kona-node-tag v0.5.0 \
  --op-batcher-tag v1.0.0
```

### Use Development Builds

Test custom builds:

```bash
kupcake \
  --op-reth-image localhost:5000/op-reth \
  --op-reth-tag dev
```

### Private Registry

Use images from private registry:

```bash
# Login to registry
docker login myregistry.io

# Deploy with custom images
kupcake \
  --op-reth-image myregistry.io/op-reth \
  --op-reth-tag internal-v1 \
  --kona-node-image myregistry.io/kona \
  --kona-node-tag internal-v2
```

### Mix and Match

Override only specific images:

```bash
kupcake \
  --op-reth-tag nightly \
  # All other images use default
```

## Building Custom Images

### Example: Build Custom op-reth

```bash
# Clone op-reth
git clone https://github.com/op-rs/op-reth
cd op-reth

# Build Docker image
docker build -t localhost:5000/op-reth:custom .

# Push to local registry (optional)
docker push localhost:5000/op-reth:custom

# Use in Kupcake
kupcake \
  --op-reth-image localhost:5000/op-reth \
  --op-reth-tag custom
```

## Verifying Images

### Check Running Images

```bash
docker ps --format "table {{.Names}}\t{{.Image}}"
```

### Inspect Image Details

```bash
docker inspect <container-name> | jq '.[0].Config.Image'
```

### Check Image Layers

```bash
docker history <image-name>:<tag>
```

## Image Pull Errors

### Authentication Required

```
Error: unauthorized: authentication required
```

**Solution**: Login to registry:

```bash
docker login ghcr.io
# or
docker login myregistry.io
```

### Image Not Found

```
Error: manifest unknown: manifest unknown
```

**Solution**: Verify image name and tag:

```bash
# List available tags (GitHub Container Registry)
gh api /orgs/op-rs/packages/container/op-reth/versions
```

### Rate Limiting

```
Error: toomanyrequests: You have reached your pull rate limit
```

**Solution**:
- Login to Docker Hub
- Use a different registry
- Wait for rate limit to reset

## Image Size Considerations

| Image | Approximate Size |
|-------|------------------|
| op-reth | ~500 MB |
| kona-node | ~300 MB |
| Anvil (foundry) | ~200 MB |
| op-batcher/proposer/challenger | ~100 MB each |
| Prometheus | ~200 MB |
| Grafana | ~300 MB |

**Total**: ~3-4 GB for all images

## Caching and Performance

### Pre-pull Images

```bash
docker pull ghcr.io/op-rs/op-reth:latest
docker pull ghcr.io/op-rs/kona:latest
# ...

# Then deploy
kupcake
```

Deployment will be faster as images are already local.

### Clean Up Old Images

```bash
# Remove unused images
docker image prune -a

# Check disk usage
docker system df
```

## Multi-Architecture Support

Most images support both amd64 and arm64:

```bash
# Docker automatically pulls correct architecture
docker pull ghcr.io/op-rs/op-reth:latest
```

For specific architecture:

```bash
docker pull --platform linux/amd64 ghcr.io/op-rs/op-reth:latest
docker pull --platform linux/arm64 ghcr.io/op-rs/op-reth:latest
```

## Configuration File

In `Kupcake.toml`:

```toml
[deployer.docker_images]
op_reth_image = "ghcr.io/op-rs/op-reth"
op_reth_tag = "v1.0.0"
kona_node_image = "ghcr.io/op-rs/kona"
kona_node_tag = "v0.5.0"
# ... other images
```

## Related Documentation

- [Custom Images Example](../examples/custom-images/)
- [Environment Variables](environment-variables.md#docker-image-overrides)
- [CLI Reference](cli-reference.md#docker-image-overrides)
