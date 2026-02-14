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

## Local Binary Deployment

Deploy services from local binaries or source directories instead of Docker images.

This is useful for:
- Testing local builds during development
- Using custom-compiled binaries with specific optimizations
- Working with unreleased versions
- Debugging with locally-built debug binaries

### How It Works

The `--<service>-binary` flag accepts either a **file path** or a **directory path**:

**File path** (pre-built binary):
1. Validates the binary is a Linux ELF executable (rejects macOS Mach-O with a helpful error)
2. Computes a SHA256 hash of the binary
3. Checks if a Docker image with that hash already exists (caching)
4. If not, creates a lightweight Docker image using `debian:trixie-slim` as the base
5. Copies the binary into the image and sets it as the entrypoint

**Directory path** (Rust source — recommended on macOS):
1. Verifies the directory contains a `Cargo.toml`
2. Detects Docker's platform architecture (aarch64 or x86_64)
3. On macOS, cross-compiles with `cargo build --release --target <linux-target> --bin <service>`
4. On Linux, builds natively with `cargo build --release --bin <service>`
5. Creates the Docker image from the resulting binary (same as file path flow)

**Base Image**: `debian:trixie-slim` (provides GLIBC 2.38+ support)

### Supported Services

All OP Stack services support local binary deployment:

- `--op-reth-binary <path>`
- `--kona-node-binary <path>`
- `--op-batcher-binary <path>`
- `--op-proposer-binary <path>`
- `--op-challenger-binary <path>`
- `--op-conductor-binary <path>`

### Basic Usage

Build from source directory (recommended, handles cross-compilation automatically):

```bash
# Just point to the source directory
kupcake --kona-node-binary ./kona
```

Deploy with a pre-built Linux binary:

```bash
kupcake --kona-node-binary ./kona/target/release/kona-node
```

### Multiple Local Binaries

```bash
kupcake \
  --op-reth-binary ./op-reth \
  --kona-node-binary ./kona \
  --op-batcher-binary ./optimism/op-batcher/bin/op-batcher
```

### Environment Variables

```bash
export KUP_KONA_NODE_BINARY=./kona
export KUP_OP_RETH_BINARY=./op-reth
kupcake
```

### Mixed Deployment

Mix local binaries with Docker images:

```bash
kupcake \
  --kona-node-binary ./kona \
  --op-reth-tag v1.0.0 \
  --op-batcher-tag latest
```

### Image Naming and Caching

Generated images follow this naming pattern:

```
kupcake-<network-name>-<service>-local:<hash>
```

Example:
```
kupcake-my-testnet-kona-node-local:5f5278820378
```

The hash is the first 12 characters of the binary's SHA256 hash. If you rebuild with the same binary, the cached image is reused.

### Verifying Local Binary Images

List local binary images:

```bash
docker images --filter "reference=kupcake-*-local*"
```

Check which binary was used:

```bash
docker inspect kupcake-my-testnet-kona-node-local:5f5278820378 \
  | jq '.[0].Config.Labels'
```

### Debug Builds

Use debug builds for troubleshooting:

```bash
# Build with debug symbols
cargo build --bin kona-node

# Deploy debug build
kupcake --kona-node-binary ./target/debug/kona-node
```

**Note**: Debug builds are much larger and slower than release builds.

### Binary Requirements (pre-built binaries)

Your binary must:
- Be a Linux ELF executable (macOS Mach-O binaries are rejected with a helpful error)
- Be statically linked or have all dependencies available in `debian:trixie-slim`
- Require GLIBC 2.38 or earlier
- Be executable (`chmod +x`)

### Source Directory Requirements (build-from-source)

Your directory must:
- Contain a `Cargo.toml`
- On macOS, have the cross-compilation toolchain installed (see below)

### macOS Cross-Compilation Setup

When passing a source directory on macOS, Kupcake auto cross-compiles for Docker's Linux platform. This requires a one-time toolchain setup:

```bash
# 1. Install the Rust cross-compilation target
#    For Apple Silicon Macs running Docker Desktop (arm64/aarch64):
rustup target add aarch64-unknown-linux-gnu

#    For Intel Macs or Docker configured for amd64:
rustup target add x86_64-unknown-linux-gnu

# 2. Install the cross-linker (via Homebrew)
brew tap messense/macos-cross-toolchains
brew install aarch64-unknown-linux-gnu    # for arm64
# or: brew install x86_64-unknown-linux-gnu  # for amd64
```

Kupcake automatically sets `CARGO_TARGET_<TRIPLE>_LINKER` when cross-compiling, so no `.cargo/config.toml` is needed.

### Cross-Compilation

Kupcake handles cross-compilation automatically when you pass a source directory on macOS. It queries Docker for its platform and builds for the correct target.

Manual cross-compilation is also supported:

```bash
# Install cross-compilation target
rustup target add x86_64-unknown-linux-gnu

# Build for Linux
cargo build --release --target x86_64-unknown-linux-gnu --bin kona-node

# Use the Linux binary
kupcake --kona-node-binary ./target/x86_64-unknown-linux-gnu/release/kona-node
```

### Troubleshooting Local Binaries

#### macOS Binary Passed to Docker

```
Error: Binary is a macOS Mach-O executable and cannot run in Docker containers.
```

**Solution**: Pass a source directory instead to auto-build for Linux:
```bash
kupcake --kona-node-binary ./kona
```

#### GLIBC Version Mismatch

```
Error: /binary: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_X.XX' not found
```

**Solution**: Your binary requires a newer GLIBC than provided by `debian:trixie-slim` (2.38). Options:
- Build with an older toolchain
- Use a statically-linked binary
- Build inside a Docker container with the same base image

#### Binary Not Executable

```
Error: exec format error
```

**Solution**:
- Ensure binary is for Linux, not macOS/Windows
- Check architecture matches (amd64 vs arm64)
- Verify binary is executable: `chmod +x <binary>`

#### Build Context Too Large

If binary is very large (>1GB), image building may be slow.

**Solution**:
- Strip debug symbols: `strip <binary>`
- Use release builds instead of debug builds
- Enable LTO in Cargo.toml for smaller binaries

### Example: Testing Local Kona Changes

```bash
# 1. Clone and modify kona
git clone https://github.com/anton-rs/kona
cd kona
# Make your changes...

# 2. Build the binary
cargo build --release --bin kona-node

# 3. Deploy with local binary
cd /path/to/kupcake
kupcake \
  --network my-test \
  --kona-node-binary ../kona/target/release/kona-node \
  --publish-all-ports \
  --detach

# 4. Test your changes
curl http://localhost:<port> -X POST \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"optimism_syncStatus","params":[],"id":1}'

# 5. Clean up
kupcake cleanup my-test
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
