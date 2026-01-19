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

**Pattern**: Each service has:
- **Config/Builder** - Configuration before deployment
- **Handler** - Runtime handle to running container(s)
- **cmd.rs** - Command builder for container arguments

**Services**:
- `anvil/` - L1 fork
- `op_deployer/` - Contract deployment
- `op_reth/` - L2 execution client
- `kona_node/` - L2 consensus client
- `op_batcher/` - Transaction batching
- `op_proposer/` - State root proposals
- `op_challenger/` - Fault proofs
- `op_conductor/` - Multi-sequencer coordination
- `l2_stack/` - Combines all L2 services
- `prometheus/` - Metrics collection
- `grafana/` - Metrics visualization

## Design Patterns

### Builder Pattern

All services follow the Builder pattern:

```rust
pub struct OpRethBuilder {
    pub image: DockerImage,
    pub network_name: String,
    pub role: L2NodeRole,
    // ... configuration fields
}

impl OpRethBuilder {
    pub async fn build(self, docker: &KupDocker) -> Result<OpRethHandler> {
        // Create and start container
        // Return runtime handler
    }
}
```

**Benefits**:
- Immutable configuration before deployment
- Validation at build time
- Clear separation of config and runtime

### Handler Pattern

Services return Handler types that represent running containers:

```rust
pub struct OpRethHandler {
    pub container_id: String,
    pub container_name: String,
    pub image: DockerImage,
    pub rpc_port: u16,
    // ... runtime info
}
```

**Benefits**:
- Type-safe container management
- Easy cleanup on shutdown
- Runtime introspection

### Composition Over Inheritance

L2StackBuilder composes multiple service builders:

```rust
pub struct L2StackBuilder {
    pub sequencers: Vec<L2NodeBuilder>,
    pub validators: Vec<L2NodeBuilder>,
    pub op_batcher: OpBatcherBuilder,
    pub op_proposer: OpProposerBuilder,
    pub op_challenger: OpChallengerBuilder,
    pub op_conductor: Option<OpConductorBuilder>,
}
```

**Benefits**:
- Flexible service combinations
- Independent service lifecycle
- Clear service dependencies

## Deployment Sequence

Kupcake deploys services in this order (see [Deployment Flow](deployment-flow.md)):

1. **Create Docker network**
2. **Start Anvil** (L1 fork)
3. **Deploy contracts** (op-deployer init + apply)
4. **Generate genesis/rollup configs**
5. **Start all op-reth instances** (execution layer)
6. **Start all kona-node instances** (consensus layer)
7. **Start op-batcher, op-proposer, op-challenger**
8. **Start op-conductor** (if multi-sequencer)
9. **Start Prometheus and Grafana**

Each step waits for the previous step to complete.

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

### File System Structure

```
./data-<network-name>/
├── Kupcake.toml              # Saved configuration
├── anvil/
│   ├── anvil.json            # Account information
│   └── state.json            # State snapshots
├── l2-stack/
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
