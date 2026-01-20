# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 📚 Comprehensive Documentation Available

**For detailed documentation, see the `docs/` directory:**
- [Getting Started Guide](docs/getting-started/quickstart.md) - Quickstart for new users
- [User Guide](docs/user-guide/cli-reference.md) - Complete CLI reference
- [Architecture Documentation](docs/architecture/overview.md) - System design and patterns
- [Service Documentation](docs/services/README.md) - Individual service details
- [Developer Guide](docs/developer-guide/README.md) - Contributing and development
- [Examples](docs/examples/README.md) - Runnable example scenarios

This file (CLAUDE.md) is for AI assistant guidance when modifying code. For comprehensive user and developer documentation, refer to the `docs/` directory.

## ⚠️ IMPORTANT: Documentation Requirements

**ALWAYS update documentation BEFORE committing code changes!**

When making ANY code changes, you MUST:
1. **Identify affected documentation** - Determine which docs need updates based on your changes:
   - CLI flag changes → Update `docs/user-guide/cli-reference.md` and `docs/user-guide/environment-variables.md`
   - New features → Update relevant user guide sections, add examples if applicable
   - Architecture changes → Update `docs/architecture/overview.md` and `CLAUDE.md`
   - Service modifications → Update `docs/services/README.md` and service-specific docs
   - Configuration changes → Update `docs/user-guide/configuration-file.md`
   - API/interface changes → Update developer guide and relevant examples

2. **Update documentation** - Make the necessary documentation changes:
   - Keep examples accurate and runnable
   - Update command outputs and screenshots if they change
   - Ensure consistency across all affected documents
   - Update CLAUDE.md if development patterns change

3. **Test examples** - If you modified example scripts or configs, verify they work:
   ```bash
   cd docs/examples/<example-name>
   ./run.sh
   ```

4. **Commit together** - Include documentation updates in the same commit as code changes:
   ```bash
   git add <code-files> <doc-files>
   git commit -m "feat: description

   - Code changes...
   - Updated documentation...
   "
   ```

**Documentation is not optional** - Outdated docs are worse than no docs. Every feature commit MUST include documentation updates.

## Project Overview

Kupcake is a CLI tool that bootstraps a complete Rust-based OP Stack (Optimism) L2 chain locally. It orchestrates Docker containers to run a full stack including L1 (Anvil fork), contract deployment, L2 execution/consensus clients, and monitoring infrastructure.

## Build Commands

```bash
# Build release binary
cargo build --release

# Build debug binary
cargo build

# Run development build with arguments
just run-dev [args]

# Run tests
cargo test

# Run specific test
cargo test test_name

# Lint code
cargo clippy

# Lint and auto-fix
cargo clippy --fix
```

## Running the Application

```bash
# Default run (forks Sepolia, generates random L2 chain ID)
./target/release/kupcake

# Common options
./target/release/kupcake --l1-chain mainnet
./target/release/kupcake --l2-chain 42069
./target/release/kupcake --network my-testnet --outdata ./my-testnet-data
./target/release/kupcake --no-cleanup  # Keep containers running on exit
./target/release/kupcake -v debug      # Debug logging

# Load from saved configuration
./target/release/kupcake --config ./data-my-network/Kupcake.toml
```

## Architecture

### High-Level Flow

1. **DeployerBuilder** (`crates/deploy/src/builder.rs`) - Constructs deployment configuration
2. **Deployer** (`crates/deploy/src/deployer.rs`) - Orchestrates the deployment sequence
3. **KupDocker** (`crates/deploy/src/docker.rs`) - Docker client wrapper using [bollard](https://crates.io/crates/bollard) for container management
4. **Services** (`crates/deploy/src/services/`) - Individual service handlers for each component

### Service Architecture

Each service follows a consistent pattern with two files:
- `mod.rs` - Contains Config/Builder structs, Handler struct, and deployment logic
- `cmd.rs` - Command builder that generates container arguments (passed to bollard's `Config::cmd`)

Services are organized as L2 node pairs (op-reth + kona-node):
- **L2NodeBuilder** - Combines OpRethBuilder + KonaNodeBuilder with a role (Sequencer/Validator)
- **L2NodeHandler** - Runtime handler managing both execution and consensus containers
- **L2StackBuilder** - Collection of all L2 nodes plus op-batcher, op-proposer, op-challenger
- **L2StackHandler** - Runtime handlers for all L2 stack components

### Trait-Based Architecture

Kupcake uses a trait-based architecture for flexible and type-safe service deployment:

#### KupcakeService Trait

All services implement the `KupcakeService` trait (`crates/deploy/src/traits/service.rs`):

```rust
pub trait KupcakeService: Clone + Serialize + DeserializeOwned + Send + Sync + 'static {
    type Stage: DeploymentStage;        // Deployment stage (L1, Contracts, L2, Monitoring)
    type Handler: Send + 'static;        // Runtime handler returned after deployment
    type Context<'a>;                    // Stage-specific deployment context

    const SERVICE_NAME: &'static str;    // Service identifier for logging

    fn deploy<'a>(self, ctx: Self::Context<'a>) -> impl Future<Output = Result<Self::Handler>>;
}
```

#### Deployment Stages

Services must be deployed in a fixed order, enforced at compile-time via stage markers:

1. **L1Stage** - Anvil (Ethereum L1 fork)
2. **ContractsStage** - op-deployer (OP Stack contract deployment)
3. **L2Stage** - L2StackBuilder (L2 nodes, batcher, proposer, challenger)
4. **MonitoringStage** - MonitoringConfig (Prometheus + Grafana)

The `NextStage` trait ensures only valid transitions are possible:
- `L1Stage -> ContractsStage`
- `ContractsStage -> L2Stage`
- `L2Stage -> MonitoringStage`

#### Deployment Contexts

Each stage receives appropriate context with dependencies from previous stages:

- **L1Context** - Docker client, output path, chain IDs
- **ContractsContext** - Adds `AnvilHandler` (L1 RPC, accounts)
- **L2Context** - Includes `AnvilHandler` for L2 node configuration
- **MonitoringContext** - Adds `L2StackHandler` for metrics scraping

#### Deployer Chain

The `Deployer<S, Next>` type represents a recursive chain of services:

```rust
// Type-safe deployment pipeline
type StandardDeployer = Deployer<
    AnvilConfig,
    Deployer<
        OpDeployerConfig,
        Deployer<
            L2StackBuilder,
            Deployer<MonitoringConfig, End>
        >
    >
>;

// Fluent API for building chains
let deployer = Deployer::new(AnvilConfig::default())
    .then(OpDeployerConfig::default())
    .then(L2StackBuilder::default())
    .then(MonitoringConfig::default());
```

The `then()` method enforces stage ordering at compile-time - invalid chains won't compile.

#### Standard Deployers

Pre-configured type aliases for common scenarios:

- **StandardDeployer** - Full stack with monitoring (L1 + Contracts + L2 + Monitoring)
- **NoMonitoringDeployer** - Without monitoring (L1 + Contracts + L2)

```rust
// Using the standard deployer
let deployer = StandardDeployer::default_stack();
let result = deployer.deploy_chain(&mut docker, outdata, l1_id, l2_id, dashboards).await?;

// Access handlers with named fields
println!("L1 RPC: {}", result.anvil.l1_rpc_url);
println!("L2 nodes: {}", result.l2_stack.node_count());
if let Some(mon) = result.monitoring {
    println!("Grafana: {}", mon.grafana.host_url);
}
```

### Multi-Sequencer Support

When `sequencer_count > 1`:
- Op-conductor is automatically deployed to coordinate sequencers using Raft consensus
- Each sequencer gets a unique container name suffix (e.g., `-sequencer-1`, `-sequencer-2`)
- Validators are numbered separately (e.g., `-validator-1`, `-validator-2`)
- The first sequencer (index 0) is the initial Raft leader and starts active
- Subsequent sequencers start in stopped state, waiting for conductor to activate them

### Component Hierarchy

```
Deployer
├── AnvilConfig/Handler (L1 fork via Foundry's Anvil)
├── OpDeployerConfig (deploys OP Stack contracts)
├── L2StackBuilder/Handler
│   ├── Sequencers (Vec<L2NodeBuilder>)
│   │   └── Each: OpRethBuilder + KonaNodeBuilder
│   ├── Validators (Vec<L2NodeBuilder>)
│   │   └── Each: OpRethBuilder + KonaNodeBuilder
│   ├── OpBatcherBuilder/Handler
│   ├── OpProposerBuilder/Handler
│   ├── OpChallengerBuilder/Handler
│   └── OpConductorBuilder/Handler (optional, if sequencer_count > 1)
└── MonitoringConfig
    ├── PrometheusConfig
    └── GrafanaConfig
```

### Key Types

- **Builder** types (e.g., `DeployerBuilder`, `OpRethBuilder`) - Configuration before deployment
- **Config** types - Serializable configuration (used in Kupcake.toml)
- **Handler** types - Runtime handles to running containers
- **DockerImage** - Image name and tag tuple for each service
- **Deployer<S, Next>** - Recursive chain type for type-safe service deployment
- **Stage markers** (L1Stage, ContractsStage, L2Stage, MonitoringStage) - Zero-sized types enforcing deployment order
- **Context types** (L1Context, ContractsContext, L2Context, MonitoringContext) - Stage-specific deployment dependencies

### Docker Networking

All containers run on a custom Docker network (`{network-name}-network`). Services communicate using container names as hostnames. Port mappings expose services to the host:
- `PortMapping` - Maps container port to host port
- `ExposedPort` - Exposes port within Docker network only

### Configuration Persistence

Deployment configuration is saved to `{outdata}/Kupcake.toml` and can be reloaded using `--config` flag. This enables:
- Resuming deployments
- Modifying and redeploying
- Sharing configurations

### File System Structure

```
{outdata}/
├── Kupcake.toml              # Saved deployment configuration
├── anvil/
│   ├── anvil.json            # Anvil account information
│   └── state.json            # Anvil state snapshots
├── l2-stack/
│   ├── genesis.json          # L2 genesis config
│   ├── rollup.json           # Rollup config for consensus
│   ├── intent.toml           # op-deployer intent file
│   ├── state.json            # Deployment state (contract addresses)
│   ├── jwt-*.hex             # JWT secrets for each node
│   └── reth-data-*/          # op-reth data directories
└── monitoring/
    ├── prometheus.yml        # Prometheus configuration
    └── grafana/              # Grafana data
```

## Important Implementation Details

### Docker Image Defaults

Default images and tags are defined in each service module (e.g., `ANVIL_DEFAULT_IMAGE`, `ANVIL_DEFAULT_TAG`). These can be overridden via CLI args or configuration.

### Account Management

Anvil generates test accounts on startup. The deployer extracts account info from Anvil's JSON output and uses specific accounts for different roles:
- Account 0: Admin/deployer
- Account 1: Batcher
- Account 2: Proposer
- Account 3: Challenger

### Genesis Timestamp Calculation

When forking L1, the genesis timestamp is calculated as: `latest_block_timestamp - (block_time * block_number)`. This ensures the L2 genesis aligns with L1 block 0 time.

### JWT Secret Management

Each op-reth instance requires a JWT secret for authenticated communication with kona-node. JWT files are generated and stored in `l2-stack/jwt-{container-name}.hex`.

### Block Time Configuration

The `block_time` parameter affects both:
- Anvil L1 block production rate
- kona-node `l1_slot_duration` parameter for L1 derivation

### Service Startup Order

Critical startup sequence managed by `Deployer::deploy()`:
1. Create Docker network
2. Start Anvil (L1)
3. Deploy contracts via op-deployer (init + apply)
4. Generate genesis.json and rollup.json
5. Start all op-reth instances (execution layer)
6. Start all kona-node instances (consensus layer)
7. Start op-batcher, op-proposer, op-challenger
8. Start op-conductor (if multi-sequencer)
9. Start Prometheus and Grafana

### Cleanup Behavior

On shutdown (Ctrl+C or error), unless `--no-cleanup` is set:
- All containers are stopped and removed
- Docker network is removed
- Data directories are preserved in `{outdata}/`

## Testing

Tests are primarily unit tests for builders and configuration. Integration testing requires Docker and is done manually via the CLI. Test count is intentionally minimal (~10 tests) as the project focuses on integration orchestration.

## CLI Structure

The CLI (`bin/kupcake/src/cli.rs`) uses clap with:
- Environment variable support (`KUP_*` prefix)
- Nested argument groups for Docker images
- Automatic help generation

## Monitoring

When enabled (default), Prometheus scrapes metrics from all services:
- Each service exposes metrics on a dedicated port
- `MetricsTarget` defines scrape configs
- Grafana dashboards in `grafana/dashboards/` visualize the data
- Default credentials: admin/admin

## Development Tips

When adding new services:
1. Create module in `crates/deploy/src/services/{service_name}/`
2. Define `{Service}Config` or `{Service}Builder` (serializable)
3. Define `{Service}Handler` (runtime handle)
4. Create `cmd.rs` with container argument builder
5. Add default image/tag constants
6. **Implement `KupcakeService` trait**:
   ```rust
   impl KupcakeService for YourServiceConfig {
       type Stage = YourDeploymentStage;  // L1Stage, ContractsStage, L2Stage, or MonitoringStage
       type Handler = YourServiceHandler;
       type Context<'a> = YourContext<'a>;  // Stage-specific context type

       const SERVICE_NAME: &'static str = "your-service";

       async fn deploy<'a>(self, ctx: Self::Context<'a>) -> Result<Self::Handler> {
           // Deploy your service using the context
           let host_config_path = ctx.outdata.join("your-service");
           self.start(ctx.docker, host_config_path, /* other args */).await
       }
   }
   ```
7. Export types in `services/mod.rs`
8. Integrate into `Deployer` or `L2StackBuilder`

**Important trait implementation notes:**
- The `Stage` associated type determines where in the deployment pipeline your service runs
- Use the appropriate `Context` type for your stage (L1Context, ContractsContext, L2Context, MonitoringContext)
- The `deploy()` method should call your existing `start()` or similar method
- `SERVICE_NAME` is used for logging and identification

When modifying container configuration:
- Container arguments are built in `cmd.rs` files using the builder pattern
- Containers are managed via bollard (not CLI) - see `KupDocker` in `docker.rs`
- Test locally with `--no-cleanup` to inspect containers
- Use `docker logs {container-name}` to debug

When changing configuration schema:
- Update both Builder and Config types
- Ensure serde attributes maintain backward compatibility
- Test with existing `Kupcake.toml` files

## Code Style

### General Principles
- Avoid nested loops/ifs/match statements as much as possible
- Avoid `if/else` statements - use early returns with `if` when possible
- Prefer functional programming style over imperative
- Keep functions small and focused on a single responsibility

### Error Handling
- **NEVER use `.unwrap()`, `.expect()`, or anything that may panic** unless absolutely necessary (e.g., compile-time guarantees)
- Use `?` operator for error propagation
- Add context to errors with `.context("descriptive message")` or `.with_context(|| format!(...))`
- Return `Result<T, anyhow::Error>` for fallible functions
- Use `anyhow::bail!()` for early error returns

```rust
// Good
let value = get_value().context("Failed to get value")?;

// Bad
let value = get_value().unwrap();
let value = get_value().expect("should work");
```

### Iterators and Collections
- Prefer iterator combinators over explicit loops
- Use `.collect()` to gather results
- Chain operations fluently

```rust
// Good
let results: Vec<_> = items
    .iter()
    .filter(|x| x.is_valid())
    .map(|x| x.transform())
    .collect();

// Bad
let mut results = Vec::new();
for item in items {
    if item.is_valid() {
        results.push(item.transform());
    }
}
```

### Option and Result Handling
- Use combinators: `.map()`, `.and_then()`, `.ok_or()`, `.unwrap_or_default()`
- Prefer `if let` over `match` for single-variant checks
- Use `?` with `.ok_or_else()` to convert Options to Results

```rust
// Good
let port = config.port.unwrap_or(8080);
let value = opt.ok_or_else(|| anyhow::anyhow!("Value not found"))?;

// Good - early return
if let Some(cached) = cache.get(&key) {
    return Ok(cached.clone());
}

// Bad
match opt {
    Some(v) => v,
    None => panic!("missing value"),
}
```

### Async Code
- Use `tokio` for async runtime
- Propagate errors with `?` in async functions
- Use `.await` on the same line when chaining

```rust
// Good
let response = client.get(url).send().await?.json().await?;

// Also acceptable for readability
let response = client
    .get(url)
    .send()
    .await
    .context("Failed to send request")?;
```

### Logging
- Use `tracing` macros (`tracing::info!`, `tracing::debug!`, etc.) instead of `println!`
- Include structured fields in log messages

```rust
tracing::info!(container_name = %name, port = %port, "Container started");
```

### Naming Conventions
- Use descriptive names that reflect purpose
- Builder pattern: `{Type}Builder` with `.build()` method
- Handler pattern: `{Type}Handler` for runtime handles
- Use `_` prefix for intentionally unused variables

### Imports and Organization
- Group imports: std, external crates, internal modules
- Use `use crate::` for internal imports
- Re-export public types in `mod.rs`