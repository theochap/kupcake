# Architecture Overview

**Target Audience**: Developers | Advanced Users

High-level overview of Kupcake's architecture and design principles.

## System Architecture

Kupcake is a Rust CLI tool that orchestrates Docker containers to deploy a complete OP Stack L2 network.

```
┌─────────────────────────────────────────────────────────────────┐
│                        Kupcake CLI                              │
│  (Rust application - bin/kupcake/src/main.rs)                  │
└────────────────────┬────────────────────────────────────────────┘
                     │
                     ↓
┌─────────────────────────────────────────────────────────────────┐
│                     DeployerBuilder                             │
│  Constructs deployment configuration from CLI args/env/config   │
│  (crates/deploy/src/builder.rs)                                │
└────────────────────┬────────────────────────────────────────────┘
                     │
                     ↓
┌─────────────────────────────────────────────────────────────────┐
│                        Deployer                                 │
│  Orchestrates deployment sequence and lifecycle                 │
│  (crates/deploy/src/deployer.rs)                               │
└────────────────────┬────────────────────────────────────────────┘
                     │
        ┌────────────┴────────────┬────────────────┐
        │                         │                │
        ↓                         ↓                ↓
┌──────────────┐         ┌────────────────┐  ┌──────────────┐
│ KupDocker    │         │ Service        │  │ Monitoring   │
│ (Docker API) │←───────→│ Builders       │  │ Stack        │
│ bollard crate│         │ & Handlers     │  └──────────────┘
└──────────────┘         └────────────────┘
        ↓
┌─────────────────────────────────────────────────────────────────┐
│                      Docker Engine                              │
│  Container orchestration, networking, volume management         │
└─────────────────────────────────────────────────────────────────┘
        ↓
┌─────────────────────────────────────────────────────────────────┐
│                   Docker Containers                             │
│  Anvil, op-reth, kona-node, op-batcher, op-proposer, etc.     │
└─────────────────────────────────────────────────────────────────┘
```

## Core Components

### 1. CLI Layer (`bin/kupcake`)

**Purpose**: Parse command-line arguments and delegate to deployment logic.

**Files**:
- `bin/kupcake/src/main.rs` - Entry point
- `bin/kupcake/src/cli.rs` - Argument parsing (clap)

**Responsibilities**:
- Parse CLI arguments
- Load environment variables
- Invoke DeployerBuilder

### 2. Deployment Layer (`crates/deploy`)

**Purpose**: Orchestrate the deployment sequence.

**Key Types**:
- **DeployerBuilder** - Constructs deployment configuration
- **Deployer** - Executes deployment steps
- **KupDocker** - Docker API wrapper (bollard)

**Files**:
- `crates/deploy/src/builder.rs` - DeployerBuilder
- `crates/deploy/src/deployer.rs` - Deployer
- `crates/deploy/src/docker.rs` - KupDocker

### 3. Service Layer (`crates/deploy/src/services`)

**Purpose**: Define and manage individual services.

**Core Trait**: `KupcakeService` (`crates/deploy/src/service.rs`) provides a unified deploy interface:
- `deploy()` - Deploy the service and return a handler
- Associated types: `Input`, `Output`

**Pattern**: Each service has:
- **Builder** - Configuration before deployment (implements `KupcakeService`)
- **Input** - Deploy-time parameters using owned data (Strings, Urls), decoupled from handler types
- **Handler** - Runtime handle to running container(s), returned by `deploy()`
- **cmd.rs** - Command builder for container arguments
- **`build_cmd()`** - Inherent method on each Builder that produces Docker command args

**Services**:
- `anvil/` - L1 fork
- `op_deployer/` - Contract deployment
- `op_reth/` - L2 execution client
- `kona_node/` - L2 consensus client
- `op_batcher/` - Transaction batching
- `op_proposer/` - State root proposals
- `op_challenger/` - Fault proofs
- `op_conductor/` - Multi-sequencer coordination
- `l2_node.rs` - Composite: combines EL + CL + optional conductor (implements `KupcakeService` by delegating)
- `l2_stack.rs` - Combines all L2 nodes + batcher/proposer/challenger
- `prometheus/` - Metrics collection
- `grafana/` - Metrics visualization

## Design Patterns

### KupcakeService Trait

All services implement the `KupcakeService` trait (`crates/deploy/src/service.rs`):

```rust
pub trait KupcakeService: Send + Sync + 'static {
    type Input: Send;
    type Output: Send;

    fn container_name(&self) -> &str;
    fn docker_image(&self) -> &DockerImage;
    fn deploy<'a>(&'a self, docker: &'a mut KupDocker, host_config_path: &'a Path, input: Self::Input)
        -> impl Future<Output = Result<Self::Output>> + Send + 'a;
}
```

**Decoupled Inputs**: Input types use owned data (`String`, `Url`, `Vec<String>`) instead of handler references. This keeps the trait simple (no GATs or lifetime parameters).

**Command Building**: Each Builder has an inherent `build_cmd()` method (not on the trait) that produces Docker command args. This is called internally by `deploy()`.

**Benefits**:
- Unified deploy interface for all services
- Swappable implementations via generics
- Composable: L2NodeBuilder implements the trait by delegating to children

### Builder + Input + Handler Pattern

Each service has three types:
- **Builder** (e.g., `OpRethBuilder`) - Configuration, implements `KupcakeService`
- **Input** (e.g., `OpRethInput`) - Deploy-time parameters using owned data (Strings, Urls), decoupled from handler types
- **Handler** (e.g., `OpRethHandler`) - Runtime handle to running container(s)

### Generic Composition

`L2StackBuilder`, `L2NodeBuilder`, and `Deployer` are all generic over their service types with default type params for backward compatibility:

```rust
pub struct L2NodeBuilder<EL = OpRethBuilder, CL = KonaNodeBuilder, Cond = OpConductorBuilder> { ... }
pub struct L2StackBuilder<Node = L2NodeBuilder, B = OpBatcherBuilder, P = OpProposerBuilder, C = OpChallengerBuilder> { ... }
pub struct Deployer<L1 = AnvilConfig, Node = L2NodeBuilder, B = OpBatcherBuilder, P = OpProposerBuilder, C = OpChallengerBuilder> { ... }
```

**Benefits**:
- Services are swappable at the type level
- Default type params maintain backward compatibility
- L2Node is treated as one opaque service by L2Stack and Deployer

## Deployment Sequence

Kupcake supports two deployment targets that determine how OP Stack contracts are deployed:

### Live Mode (default)

In live mode, Anvil starts first and contracts are deployed to the running L1 via transactions. This supports forking remote L1 chains.

1. **Compute deployment configuration hash** (SHA-256 of deployment-relevant parameters)
2. **Create Docker network**
3. **Start Anvil** (L1, optionally forking a remote chain; if `--override-state` is specified, loads the external state via `--load-state`; otherwise if a persisted `state.json` exists from a previous run, restores from it via `--load-state`)
4. **Check deployment version** - Compare current hash with saved hash
   - If unchanged, skip contract deployment (saves 30-60s)
   - If changed, missing, or corrupted, redeploy contracts
5. **Deploy contracts** (op-deployer init + apply) - Only if needed
6. **Save deployment version** - Store hash, timestamp, and Kupcake version
7. **Generate genesis/rollup configs**

### Genesis Mode

In genesis mode, contracts are deployed into an in-memory L1 state, then Anvil boots from the resulting genesis. This is ~3-4x faster but only works with local Anvil (no fork).

L1 state restoration is supported: if `anvil/state.json` exists and contracts haven't been redeployed, Anvil restores from the persisted state via `--load-state` instead of booting fresh from genesis. State is persisted via `anvil_dumpState` RPC before cleanup (Anvil's `--init` flag is incompatible with `--dump-state`). Note: `--override-state` is not supported in genesis mode and will produce an error if specified.

1. **Compute deployment configuration hash**
2. **Create Docker network**
3. **Check deployment version** - Compare current hash with saved hash
4. **Deploy contracts in-memory** (op-deployer init + apply with `--l1-state-dump`) - Only if needed
5. **Extract L1 genesis** from state dump
6. **Start Anvil** - If persisted `state.json` exists (from a previous run), use `--load-state` to restore; otherwise use `--init` with L1 genesis
7. **Patch rollup.json** with Anvil's actual genesis block hash (workaround for [foundry-rs/foundry#7366](https://github.com/foundry-rs/foundry/issues/7366))
8. **Save deployment version**

### Common Steps (both modes)

After L1 and contracts are ready:

8/9. **Start all op-reth instances** (execution layer; op-rbuilder for sequencers if `--flashblocks`)
9/10. **Start all kona-node instances** (consensus layer; with flashblocks relay if `--flashblocks`)
10/11. **Start op-batcher, op-proposer, op-challenger**
11/12. **Start op-conductor** (if multi-sequencer)
12/13. **Start Prometheus and Grafana**

Each step waits for the previous step to complete.

### Flashblocks Data Flow

When `--flashblocks` is enabled, the sequencer's execution client is replaced with op-rbuilder (a fork of op-reth). Kona-node's built-in flashblocks relay propagates sub-block data:

```
Sequencer:
  op-rbuilder (flashblocks WS on port 1111)
       ↓ ws://op-rbuilder:1111
  sequencer kona-node (--flashblocks, relay on port 1112)
       ↓ ws://sequencer-kona-node:1112
  validator kona-node (--flashblocks, subscribes to relay)
       ↓
  validator op-reth (unchanged)
```

## Docker Integration

### bollard Crate

Kupcake uses [bollard](https://crates.io/crates/bollard) for Docker API access:

```rust
pub struct KupDocker {
    pub client: Docker,
}

impl KupDocker {
    pub async fn create_container(&self, config: ContainerConfig) -> Result<String> {
        // Create container via Docker API
    }

    pub async fn start_container(&self, id: &str) -> Result<()> {
        // Start container
    }
}
```

**Why bollard?**
- Type-safe Docker API
- Async support (tokio)
- No CLI subprocess overhead
- Better error handling

### Container Configuration

Containers are configured using bollard's `Config` type:

```rust
let config = Config {
    image: Some("ghcr.io/op-rs/op-reth:latest"),
    cmd: Some(vec!["node", "--http", "--http.port=8545"]),
    env: Some(vec!["RUST_LOG=info"]),
    exposed_ports: Some(port_map),
    host_config: Some(HostConfig { /* ... */ }),
    // ... more fields
};
```

### Local Binary Deployment

Kupcake supports deploying services from local binaries instead of Docker images.

**Process**:

1. **Hash Computation**: Calculate SHA256 hash of the binary
2. **Cache Check**: Check if an image with that hash already exists
3. **Image Building**: If not cached, build a Docker image:
   - Base image: `debian:trixie-slim` (GLIBC 2.38+)
   - Copy binary into image
   - Set binary as entrypoint
4. **Deployment**: Deploy container using the generated image

**Implementation** (`crates/deploy/src/docker.rs`):

```rust
pub async fn build_local_image(
    &self,
    binary_path: &Path,
    service_name: &str,
) -> Result<String> {
    // Compute SHA256 hash
    let hash = compute_file_hash(binary_path)?;
    let short_hash = &hash[..12];
    let image_ref = format!("kupcake-{}-local:{}", service_name, short_hash);

    // Check if cached
    if self.docker.inspect_image(&image_ref).await.is_ok() {
        return Ok(image_ref);
    }

    // Pull base image
    self.pull_image("debian", "trixie-slim").await?;

    // Create build context with Dockerfile and binary
    let tar_bytes = create_build_context(binary_path)?;

    // Build image via Docker API
    self.docker.build_image(BuildImageOptions {
        dockerfile: "Dockerfile".to_string(),
        t: image_ref.clone(),
        // ...
    }, None, Some(tar_bytes.into())).await?;

    Ok(image_ref)
}
```

**Generated Dockerfile**:

```dockerfile
FROM debian:trixie-slim
COPY binary /binary
RUN chmod +x /binary
ENTRYPOINT ["/binary"]
```

**Image Naming**:
- Pattern: `kupcake-<network>-<service>-local:<hash>`
- Example: `kupcake-my-testnet-kona-node-local:5f5278820378`

**Caching**: Images are cached by hash, so rebuilding with the same binary reuses the existing image.

**Use Cases**:
- Testing local builds during development
- Using custom-compiled binaries with optimizations
- Working with unreleased versions
- Debugging with debug builds

### Networking

All containers run in an isolated Docker network:

```rust
let network_config = NetworkConfig {
    name: format!("{}-network", network_name),
    driver: Some("bridge"),
    // ...
};
```

**Benefits**:
- Containers communicate via container names
- Isolated from other Docker networks
- No host port conflicts for internal communication

## Data Persistence

### Configuration Persistence

Deployment configuration is saved to `Kupcake.toml`:

```rust
let config = DeployerConfig {
    network_name,
    l1_chain_id,
    l2_chain_id,
    // ... all settings
};

// Serialize to TOML
let toml = toml::to_string_pretty(&config)?;
fs::write("./data/Kupcake.toml", toml)?;
```

### Deployment Versioning

Kupcake implements a hash-based versioning system to skip unnecessary contract redeployments in both live and genesis modes (`crates/deploy/src/deployment_hash.rs`):

**Implementation**:

```rust
// 1. Compute hash of deployment-relevant parameters
let config = DeploymentConfigHash::from_deployer(&deployer);
let hash = config.compute_hash(); // SHA-256 hash

// 2. Check if deployment is needed
let version_file = Path::new(".deployment-version.json");
if let Ok(prev_version) = DeploymentVersion::load_from_file(version_file) {
    if prev_version.config_hash == hash {
        // Skip deployment - configuration unchanged
        return Ok(());
    }
}

// 3. Deploy contracts...

// 4. Save version metadata
let version = DeploymentVersion {
    config_hash: hash,
    deployed_at: current_timestamp(),
    kupcake_version: env!("CARGO_PKG_VERSION"),
};
version.save_to_file(version_file)?;
```

**Hash Scope**:

Included in hash (affects contract deployment):
- `l1_chain_id` - Determines OPCM contracts
- `l2_chain_id` - Embedded in contracts
- `fork_url` - L1 state source
- `fork_block_number` - Fork point
- `timestamp` - Genesis alignment
- EIP-1559 parameters

Excluded from hash (runtime-only):
- `block_time` - Anvil mining rate
- Docker images/tags
- Port mappings
- Container names
- Sequencer/validator counts

**Behavior**:
- `--redeploy` flag bypasses all checks
- Missing/corrupted version file triggers redeployment
- Logs both hashes when configuration changes

### File System Structure

```
./data-<network-name>/
├── Kupcake.toml              # Saved configuration
├── anvil/
│   ├── anvil.json            # Account information
│   └── state.json            # State snapshots
├── l2-stack/
│   ├── .deployment-version.json  # Deployment version metadata
│   ├── genesis.json          # L2 genesis config
│   ├── rollup.json           # Rollup config
│   ├── state.json            # Contract addresses
│   ├── jwt-*.hex             # JWT secrets
│   └── reth-data-*/          # op-reth databases
└── monitoring/
    ├── prometheus.yml        # Prometheus config
    └── grafana/              # Grafana data
```

## Error Handling

Kupcake uses `anyhow` for error handling:

```rust
use anyhow::{Context, Result};

pub async fn deploy(&self) -> Result<()> {
    let container_id = self.start_anvil()
        .context("Failed to start Anvil")?;

    self.deploy_contracts()
        .context("Failed to deploy contracts")?;

    Ok(())
}
```

**Principles**:
- **Never panic** - Use `?` operator for error propagation
- **Add context** - Use `.context()` for descriptive errors
- **Fail fast** - Return errors immediately
- **Clean shutdown** - Stop containers on error

## Async Runtime

Kupcake uses [tokio](https://tokio.rs/) for async execution:

```rust
#[tokio::main]
async fn main() -> Result<()> {
    let deployer = DeployerBuilder::new()
        .network_name("my-network")
        .build()?;

    deployer.deploy().await?;
    Ok(())
}
```

**Why async?**
- Docker API is async (bollard)
- Concurrent container operations
- Better resource utilization

## Logging

Kupcake uses [tracing](https://docs.rs/tracing) for structured logging:

```rust
use tracing::{info, debug, error};

info!(container_name = %name, port = %port, "Container started");
debug!("Generated genesis config");
error!(error = %e, "Failed to start container");
```

**Log Levels**:
- `error` - Fatal errors
- `warn` - Non-fatal warnings
- `info` - High-level progress (default)
- `debug` - Detailed debugging
- `trace` - Very verbose

## Related Documentation

- [Deployment Flow](deployment-flow.md) - Step-by-step deployment sequence
- [Component Hierarchy](component-hierarchy.md) - Builder/Config/Handler patterns
- [Docker Networking](docker-networking.md) - Container networking model
- [Data Persistence](data-persistence.md) - File system structure
- [Service Coordination](service-coordination.md) - How services communicate
