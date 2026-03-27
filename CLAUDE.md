# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 📚 Comprehensive Documentation Available

**For detailed documentation, see the `docs/` directory:**
- [Getting Started Guide](docs/getting-started/quickstart.md) - Quickstart for new users
- [User Guide](docs/user-guide/cli-reference.md) - Complete CLI reference
- [Network Interaction & Health Checks](docs/user-guide/network-interaction.md) - Querying nodes, sending transactions, verifying network health
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
5. **Node Lifecycle** (`crates/deploy/src/node_lifecycle.rs`) - Add/remove/pause/unpause/restart L2 nodes on a live network
6. **Inspect** (`crates/deploy/src/inspect.rs`) - Network inspection (container states, block heights, sync status, URLs)

### Service Architecture

Each service implements the `KupcakeService` trait (`crates/deploy/src/service.rs`) which provides a unified deploy interface:
- `deploy()` - Deploy the service container(s) and return a handler
- Associated types: `Input` (deploy-time parameters using owned data), `Output` (handler)

**Pattern**: Each service has:
- **Builder** - Configuration before deployment (implements `KupcakeService`)
- **Input** - Deploy-time parameters using owned data (Strings, Urls), decoupled from handler types
- **Handler** - Runtime handle to running container(s), returned by `deploy()`
- **cmd.rs** - Command builder for container arguments
- **`build_cmd()`** - Inherent method on each Builder that produces Docker command args

A `deploy_container()` helper function (`crates/deploy/src/service.rs`) provides the common single-container deploy pipeline used by leaf services.

Services are organized as L2 node pairs (op-reth + kona-node):
- **L2NodeBuilder<EL, CL, Cond>** - Generic composite that combines execution + consensus + optional conductor. Implements `KupcakeService` by delegating to its children's `deploy()` methods.
- **L2NodeHandler** - Runtime handler managing both execution and consensus containers
- **L2StackBuilder<Node, B, P, C>** - Generic collection of all L2 nodes plus batcher, proposer, challenger. Calls `node.deploy()` via the trait.
- **L2StackHandler** - Runtime handlers for all L2 stack components
- **Deployer<L1, Node, B, P, C>** - Generic top-level deployer. All type params default to concrete types for backward compatibility.

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
│   │         (op-rbuilder replaces op-reth when --flashblocks enabled)
│   ├── Validators (Vec<L2NodeBuilder>)
│   │   └── Each: OpRethBuilder + KonaNodeBuilder
│   ├── OpBatcherBuilder/Handler
│   ├── OpProposerBuilder/Handler
│   ├── OpChallengerBuilder/Handler
│   └── OpConductorBuilder/Handler (optional, if sequencer_count > 1)
├── NodeLifecycle (add/remove/pause/unpause/restart nodes on live network)
├── DevnetRegistry (~/.kupcake/devnets.toml — global tracking of deployed devnets)
└── MonitoringConfig
    ├── PrometheusConfig
    └── GrafanaConfig
```

### Key Types

- **KupcakeService** trait (`crates/deploy/src/service.rs`) - Unified deploy interface with associated types `Input` and `Output`
- **Builder** types (e.g., `OpRethBuilder`, `OpBatcherBuilder`) - Configuration before deployment, implement `KupcakeService`
- **Input** types (e.g., `OpRethInput`, `L2NodeInput`) - Deploy-time parameters using owned data (`String`, `Url`, `Vec<String>`), decoupled from handler types
- **Handler** types - Runtime handles to running containers, returned by `deploy()`
- **DockerImage** - Image name and tag tuple for each service

### Docker Networking

All containers run on a custom Docker network (`{network-name}-network`). Services communicate using container names as hostnames. Port mappings expose services to the host:
- `PortMapping` - Maps container port to host port
- `ExposedPort` - Exposes port within Docker network only

### Configuration Persistence

Deployment configuration is saved to `{outdata}/Kupcake.toml` and can be reloaded using `--config` flag. This enables:
- Resuming deployments
- Modifying and redeploying
- Sharing configurations

### Deployment Targets

Kupcake supports two deployment targets (`--deployment-target`):

- **Live** (default): Anvil starts first, then op-deployer deploys contracts to the running L1 via transactions. Supports forking remote L1 chains (`--l1`).
- **Genesis**: op-deployer deploys contracts into an in-memory L1 state dump, then Anvil boots from the resulting genesis. ~3-4x faster but only works with local Anvil (no fork).

Genesis mode implementation:
- `crates/deploy/src/l1_genesis.rs` - Extracts L1 genesis from op-deployer state dump, patches rollup.json with Anvil's actual genesis block hash (workaround for [foundry-rs/foundry#7366](https://github.com/foundry-rs/foundry/issues/7366))
- `crates/deploy/src/accounts.rs` - Derives Anvil accounts from mnemonic for genesis mode (Anvil isn't running yet)

Both modes use a unified Anvil state persistence approach: `--load-state` for restoring state and `anvil_dumpState` RPC for persisting state before cleanup. In genesis mode, if `anvil/state.json` exists and contracts weren't redeployed, Anvil uses `--load-state` to restore from the persisted state; otherwise, Anvil boots fresh from `l1-genesis.json` via `--init`. If contracts are redeployed, stale `state.json` is deleted so Anvil boots fresh from the new genesis. In live mode, if a persisted `state.json` exists from a previous run, it is restored via `--load-state`. The `--override-state <PATH>` flag allows loading an external Anvil state file in live mode (errors out in genesis mode). The `--dump-state` bool flag controls whether state is persisted via RPC on shutdown (default: true).

### Deployment Versioning

Kupcake implements a configuration hash-based versioning system to avoid unnecessary contract redeployments in both live and genesis modes (`crates/deploy/src/deployment_hash.rs`):

**How it works:**
1. Before deploying contracts, compute a SHA-256 hash of deployment-relevant parameters
2. Save hash to `{outdata}/l2-stack/.deployment-version.json` after successful deployment
3. On subsequent runs, compare current config hash with saved hash
4. Skip contract deployment if hashes match (saves 30-60s in live mode, ~15s in genesis mode)

**Parameters included in hash (affect contract deployment):**
- `l1_chain_id` - Determines which OPCM contracts are used
- `l2_chain_id` - Embedded in deployed contracts and genesis
- `deployment_target` - Live vs Genesis changes how contracts are deployed
- `fork_url` - Changes which L1 state is forked
- `fork_block_number` - Changes L1 fork point
- `timestamp` - Affects genesis timestamp alignment
- EIP-1559 parameters (denominator, elasticity)

**Parameters excluded from hash (runtime-only):**
- `block_time` - Only affects Anvil mining rate
- Docker images/tags, port mappings, container names
- Sequencer/validator counts
- Monitoring settings

**Behavior:**
- `--redeploy` flag bypasses all hash checks and always redeploys
- Missing or corrupted version file triggers redeployment (safe fallback)
- Configuration changes log both previous and current hashes

### File System Structure

```
~/.kupcake/
├── devnets.toml              # Global devnet registry (name, state, datadir, timestamps)
└── devnets.lock              # Lock file for concurrent access

{outdata}/
├── Kupcake.toml              # Saved deployment configuration
├── anvil/
│   ├── anvil.json            # Anvil account information
│   ├── l1-genesis.json       # L1 genesis state (genesis mode only)
│   └── state.json            # Anvil state snapshots
├── l2-stack/
│   ├── .deployment-version.json  # Deployment version metadata (hash, timestamp, version)
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

By default, the genesis timestamp is automatically calculated:
- When forking L1: `latest_block_timestamp - (block_time * block_number)` - ensures L2 genesis aligns with L1 block 0 time
- In local mode: Current Unix timestamp

This can be manually overridden using the `--genesis-timestamp` CLI argument or the `genesis_timestamp()` method on `DeployerBuilder`. When a manual timestamp is provided, it is used instead of the automatic calculation.

### JWT Secret Management

Each op-reth instance requires a JWT secret for authenticated communication with kona-node. JWT files are generated and stored in `l2-stack/jwt-{container-name}.hex`.

### Block Time Configuration

The `block_time` parameter affects both:
- Anvil L1 block production rate
- kona-node `l1_slot_duration` parameter for L1 derivation

### Service Startup Order

Critical startup sequence managed by `Deployer::deploy()`:

**Live mode:**
1. Create Docker network
2. Start Anvil (L1; restores from `--override-state`, persisted `state.json`, or starts fresh)
3. Deploy contracts via op-deployer (init + apply) — skipped if config hash matches
4. Generate genesis.json and rollup.json

**Genesis mode:**
1. Create Docker network
2. Deploy contracts in-memory via op-deployer — skipped if config hash matches
3. Extract L1 genesis from state dump
4. Start Anvil (restores from persisted `state.json` via `--load-state`, or boots fresh from genesis via `--init`)
5. Patch rollup.json with actual genesis block hash

**Both modes (continued):**
- Start all op-reth instances (execution layer)
- Start all kona-node instances (consensus layer)
- Start op-batcher, op-proposer, op-challenger
- Start op-conductor (if multi-sequencer)
- Start Prometheus and Grafana

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
2. Define `{Service}Builder` (serializable) and `{Service}Input` struct
3. Define `{Service}Handler` (runtime handle)
4. Create `cmd.rs` with container argument builder
5. Implement `KupcakeService` trait for `{Service}Builder` (see `service.rs` for the trait and `deploy_container` helper)
6. Add default image/tag constants
7. Export types in `services/mod.rs`
8. Integrate into `Deployer` or `L2StackBuilder`

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

### Retry and Polling
- **NEVER write manual polling loops** (`loop` + `sleep` + deadline/`Instant` checks)
- Use the `backon` crate (`Retryable` with `ConstantBuilder` or `ExponentialBuilder`) for all retry/polling patterns
- This applies to port readiness checks, RPC availability, file existence, or any condition that needs repeated checking

```rust
// Good
use backon::{ConstantBuilder, Retryable};

let backoff = ConstantBuilder::default()
    .with_delay(Duration::from_millis(500))
    .with_max_times(30);

let result = (|| async {
    let value = check_something().await?;
    if !value.is_ready() {
        anyhow::bail!("not ready yet");
    }
    Ok(value)
})
.retry(backoff)
.await
.context("Timed out waiting for readiness")?;

// Bad
loop {
    if start.elapsed() > timeout {
        anyhow::bail!("timed out");
    }
    if let Ok(value) = check_something().await {
        break value;
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
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
- **NEVER use `print!`, `println!`, `eprint!`, or `eprintln!`** — always use `tracing` macros (`tracing::info!`, `tracing::debug!`, etc.)
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