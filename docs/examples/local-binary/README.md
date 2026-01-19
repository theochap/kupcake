# Local Binary Deployment Example

Deploy OP Stack services from locally-built binaries instead of Docker images.

## Use Case

This example is useful when you need to:
- Test local builds during development
- Use custom-compiled binaries with specific optimizations
- Work with unreleased versions
- Debug with locally-built debug binaries

## Prerequisites

- Docker running
- Rust toolchain installed
- Source code for the services you want to build locally

## Example: Deploy with Local Kona Binary

### Step 1: Build Kona Locally

```bash
# Clone kona repository
git clone https://github.com/anton-rs/kona
cd kona

# Build kona-node binary
cargo build --release --bin kona-node

# Verify binary exists
ls -lh target/release/kona-node
```

### Step 2: Deploy with Local Binary

```bash
# Deploy using local kona-node binary
kupcake \
  --network local-kona-test \
  --kona-node-binary ./kona/target/release/kona-node \
  --l2-nodes 2 \
  --sequencer-count 1 \
  --publish-all-ports \
  --detach
```

### Step 3: Verify Deployment

```bash
# Check containers
docker ps

# Check local binary images
docker images --filter "reference=kupcake-*-local*"

# Output example:
# REPOSITORY                                       TAG              SIZE
# kupcake-local-kona-test-kona-node-local          5f5278820378    450MB
# kupcake-local-kona-test-kona-node-validator-1-local  5f5278820378    450MB
```

### Step 4: Test the Node

```bash
# Get the kona-node port (should be visible in docker ps output)
KONA_PORT=$(docker port local-kona-test-kona-node 7545 | cut -d: -f2)

# Query sync status
curl -X POST http://localhost:$KONA_PORT \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "optimism_syncStatus",
    "params": [],
    "id": 1
  }' | jq
```

### Step 5: Clean Up

```bash
kupcake cleanup local-kona-test
```

## Example: Multiple Local Binaries

Deploy multiple services from local binaries:

```bash
# Build op-reth
cd ~/op-reth
cargo build --release

# Build kona-node
cd ~/kona
cargo build --release --bin kona-node

# Deploy with both local binaries
kupcake \
  --network multi-local \
  --op-reth-binary ~/op-reth/target/release/op-reth \
  --kona-node-binary ~/kona/target/release/kona-node \
  --l2-nodes 2 \
  --block-time 2 \
  --detach
```

## Example: Debug Build

Deploy with debug build for troubleshooting:

```bash
# Build debug version (includes debug symbols)
cd ~/kona
cargo build --bin kona-node

# Deploy debug build
kupcake \
  --network debug-test \
  --kona-node-binary ./target/debug/kona-node \
  --verbosity debug \
  --detach

# Check logs for detailed output
docker logs debug-test-kona-node
```

**Note**: Debug builds are much larger and slower than release builds.

## Cross-Compilation for Linux

If building on macOS or Windows, you need to compile for Linux:

```bash
# Add Linux target
rustup target add x86_64-unknown-linux-gnu

# Build for Linux
cargo build --release --target x86_64-unknown-linux-gnu --bin kona-node

# Deploy Linux binary
kupcake \
  --kona-node-binary ./target/x86_64-unknown-linux-gnu/release/kona-node
```

## How It Works

When you provide a binary path, Kupcake:

1. **Computes SHA256 hash** of the binary
2. **Checks for cached image** with that hash
3. **Builds Docker image** (if not cached):
   - Base: `debian:trixie-slim` (GLIBC 2.38+)
   - Copies binary into image
   - Sets binary as entrypoint
4. **Deploys container** using the generated image

### Image Naming

Generated images follow this pattern:

```
kupcake-<network-name>-<service>-local:<hash>
```

Example: `kupcake-local-kona-test-kona-node-local:5f5278820378`

The hash (first 12 chars of SHA256) ensures cache reuse when the binary hasn't changed.

## Environment Variables

Use environment variables for cleaner scripts:

```bash
#!/bin/bash
# deploy-local.sh

export KUP_NETWORK_NAME=my-local-test
export KUP_KONA_NODE_BINARY=./kona/target/release/kona-node
export KUP_OP_RETH_BINARY=./op-reth/target/release/op-reth
export KUP_BLOCK_TIME=2
export KUP_PUBLISH_ALL_PORTS=true
export KUP_DETACH=true

kupcake
```

## Troubleshooting

### GLIBC Version Mismatch

**Error**:
```
/binary: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_X.XX' not found
```

**Solutions**:
1. Build with an older toolchain
2. Use a statically-linked binary:
   ```bash
   RUSTFLAGS='-C target-feature=+crt-static' cargo build --release --target x86_64-unknown-linux-gnu
   ```
3. Build inside a Docker container with the same base image:
   ```bash
   docker run --rm -v $(pwd):/workspace -w /workspace debian:trixie-slim \
     bash -c "apt-get update && apt-get install -y cargo && cargo build --release"
   ```

### Binary Not Executable

**Error**:
```
exec format error
```

**Solutions**:
- Ensure binary is for Linux, not macOS/Windows
- Check architecture (amd64 vs arm64):
  ```bash
  file target/release/kona-node
  # Should output: ELF 64-bit LSB executable, x86-64
  ```
- Make executable:
  ```bash
  chmod +x target/release/kona-node
  ```

### Large Binary Size

If binary is very large, building may be slow.

**Solutions**:
- Strip debug symbols:
  ```bash
  strip target/release/kona-node
  ```
- Enable LTO in Cargo.toml:
  ```toml
  [profile.release]
  lto = true
  codegen-units = 1
  ```

## Mixed Deployment

Mix local binaries with Docker images:

```bash
# Use local kona-node but Docker images for everything else
kupcake \
  --kona-node-binary ./kona/target/release/kona-node \
  --op-reth-tag v1.0.0 \
  --op-batcher-tag latest
```

This is useful when you only need to test changes to one component.

## Development Workflow

Typical development workflow with local binaries:

```bash
#!/bin/bash
# dev-loop.sh - Development iteration script

# 1. Make code changes
vim kona/crates/node/src/main.rs

# 2. Rebuild
cd kona && cargo build --release --bin kona-node

# 3. Clean up old deployment
kupcake cleanup dev-test

# 4. Deploy with new binary
kupcake \
  --network dev-test \
  --kona-node-binary ./target/release/kona-node \
  --block-time 2 \
  --publish-all-ports \
  --detach

# 5. Run tests
./run-tests.sh

# 6. Check logs if needed
docker logs dev-test-kona-node
```

## Related Documentation

- [Docker Images Guide - Local Binary Deployment](../../user-guide/docker-images.md#local-binary-deployment)
- [CLI Reference - Local Binary Deployment](../../user-guide/cli-reference.md#local-binary-deployment)
- [Environment Variables - Local Binary Paths](../../user-guide/environment-variables.md#local-binary-paths)
- [Architecture Overview - Local Binary Deployment](../../architecture/overview.md#local-binary-deployment)
