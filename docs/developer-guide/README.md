# Developer Guide

**Target Audience**: Contributors | Developers Extending Kupcake

Welcome to the Kupcake developer guide. This section covers everything you need to know to contribute to or extend Kupcake.

## Quick Links

- [Project Structure](project-structure.md) - Codebase organization
- [Build and Test](build-and-test.md) - Build commands and testing
- [Adding Services](adding-services.md) - How to add new services
- [Builder Pattern](builder-pattern.md) - Design patterns used
- [Docker Integration](docker-integration.md) - Using bollard
- [Configuration Schema](configuration-schema.md) - Serde/TOML patterns
- [Error Handling](error-handling.md) - Error handling patterns
- [Code Style](code-style.md) - Rust style guide
- [Contributing](contributing.md) - How to contribute

## Getting Started

### Prerequisites

- **Rust**: 1.75 or higher
- **Docker**: 20.10 or higher
- **Git**: For cloning the repository

### Clone and Build

```bash
git clone https://github.com/op-rs/kupcake.git
cd kupcake
cargo build --release
```

### Run Tests

```bash
cargo test
cargo clippy
cargo clippy --fix
```

### Development Workflow

```bash
# Use justfile for convenience
just build      # Build release binary
just run-dev -- --network test  # Run development build
just test       # Run tests
just lint       # Run clippy
just fix        # Run clippy --fix
```

## Project Structure

```
kupcake/
├── bin/
│   └── kupcake/           # CLI binary
│       ├── src/
│       │   ├── main.rs    # Entry point
│       │   └── cli.rs     # Argument parsing
│       └── Cargo.toml
│
├── crates/
│   └── deploy/            # Deployment logic
│       ├── src/
│       │   ├── lib.rs     # Library root
│       │   ├── builder.rs # DeployerBuilder
│       │   ├── deployer.rs # Deployer
│       │   ├── docker.rs  # KupDocker (bollard wrapper)
│       │   └── services/  # Service definitions
│       │       ├── anvil/
│       │       │   ├── mod.rs  # Config, Builder, Handler
│       │       │   └── cmd.rs  # Container command builder
│       │       ├── op_reth/
│       │       ├── kona_node/
│       │       ├── op_batcher/
│       │       ├── op_proposer/
│       │       ├── op_challenger/
│       │       ├── op_conductor/
│       │       ├── op_deployer/
│       │       ├── l2_stack/     # Combines all L2 services
│       │       ├── prometheus/
│       │       └── grafana/
│       └── Cargo.toml
│
├── docs/                  # Documentation
├── grafana/               # Grafana dashboards
├── justfile               # Build commands
├── Cargo.toml             # Workspace config
├── CLAUDE.md              # AI assistant guidance
└── README.md              # Project readme
```

## Key Concepts

### Trait-Based Service Architecture

All services implement the `KupcakeService` trait for type-safe, composable deployment:

```rust
pub trait KupcakeService: Clone + Serialize + DeserializeOwned + Send + Sync + 'static {
    type Stage: DeploymentStage;        // Deployment stage (L1, Contracts, L2, Monitoring)
    type Handler: Send + 'static;        // Runtime handler returned after deployment
    type Context<'a>;                    // Stage-specific deployment context

    const SERVICE_NAME: &'static str;    // Service identifier for logging

    fn deploy<'a>(self, ctx: Self::Context<'a>) -> impl Future<Output = Result<Self::Handler>>;
}
```

**Key benefits**:
- **Type safety**: Invalid deployment chains won't compile
- **Stage ordering**: Services must be deployed in correct sequence
- **Context injection**: Dependencies passed automatically based on stage
- **Serialization**: All configs can be saved to Kupcake.toml

**Deployment stages** (enforced at compile-time):
1. **L1Stage** - Anvil (Ethereum L1 fork)
2. **ContractsStage** - op-deployer (contract deployment)
3. **L2Stage** - L2 nodes, batcher, proposer, challenger
4. **MonitoringStage** - Prometheus, Grafana

See [Architecture Overview](../architecture/overview.md#trait-based-service-architecture) for details.

### Builder Pattern

Services follow the Builder pattern in conjunction with the trait:

```rust
pub struct OpRethBuilder {
    pub image: DockerImage,
    pub network_name: String,
    pub role: L2NodeRole,
    // ... config fields
}

impl OpRethBuilder {
    pub async fn build(self, docker: &KupDocker) -> Result<OpRethHandler> {
        // 1. Generate container command
        let cmd = self.generate_command()?;

        // 2. Create Docker container config
        let config = ContainerConfig { /* ... */ };

        // 3. Create and start container
        let container_id = docker.create_container(config).await?;
        docker.start_container(&container_id).await?;

        // 4. Return handler
        Ok(OpRethHandler {
            container_id,
            container_name: self.container_name(),
            // ...
        })
    }
}

// Implement KupcakeService trait
impl crate::traits::KupcakeService for OpRethBuilder {
    type Stage = crate::traits::L2Stage;
    type Handler = OpRethHandler;
    type Context<'a> = crate::traits::L2Context<'a>;

    const SERVICE_NAME: &'static str = "op-reth";

    async fn deploy<'a>(self, ctx: Self::Context<'a>) -> Result<Self::Handler> {
        self.build(ctx.docker).await
    }
}
```

### Handler Pattern

Handlers represent running containers:

```rust
pub struct OpRethHandler {
    pub container_id: String,
    pub container_name: String,
    pub image: DockerImage,
    pub rpc_port: u16,
}

impl OpRethHandler {
    pub async fn stop(&self, docker: &KupDocker) -> Result<()> {
        docker.stop_container(&self.container_id).await
    }

    pub async fn logs(&self, docker: &KupDocker) -> Result<String> {
        docker.container_logs(&self.container_id).await
    }
}
```

### Docker Integration

Kupcake uses [bollard](https://crates.io/crates/bollard) for Docker API access:

```rust
use bollard::Docker;
use bollard::container::{Config, CreateContainerOptions};

pub struct KupDocker {
    pub client: Docker,
}

impl KupDocker {
    pub async fn create_container(&self, config: ContainerConfig) -> Result<String> {
        let options = CreateContainerOptions {
            name: config.name,
        };

        let response = self.client
            .create_container(Some(options), config.into())
            .await?;

        Ok(response.id)
    }
}
```

## Code Style

Kupcake follows strict Rust coding guidelines:

### Error Handling

```rust
// Good
let value = get_value().context("Failed to get value")?;

// Bad
let value = get_value().unwrap();
let value = get_value().expect("should work");
```

### Iterators

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

### Logging

```rust
use tracing::{info, debug, error};

info!(container_name = %name, port = %port, "Container started");
debug!("Generated genesis config");
error!(error = %e, "Failed to start container");
```

See [Code Style Guide](code-style.md) for complete guidelines.

## Adding a New Service

See [Adding Services Guide](adding-services.md) for comprehensive step-by-step instructions.

Basic steps:

1. Create `crates/deploy/src/services/<service>/mod.rs`
2. Define `<Service>Config` or `<Service>Builder` (must be serializable)
3. Define `<Service>Handler` for runtime
4. Create `cmd.rs` with container command builder
5. Add default image/tag constants
6. **Implement `KupcakeService` trait** with appropriate Stage, Handler, and Context types
7. Export types in `services/mod.rs`
8. Integrate into `Deployer` chain or `L2StackBuilder`

**Critical**: The `KupcakeService` trait implementation determines when and how your service is deployed. Choose the correct stage:
- **L1Stage** - For L1 infrastructure (like Anvil)
- **ContractsStage** - For contract deployment
- **L2Stage** - For L2 nodes and services
- **MonitoringStage** - For monitoring infrastructure

## Testing

### Unit Tests

```bash
cargo test
```

### Integration Tests

Integration tests require Docker:

```bash
# Run integration tests (manual for now)
cargo build --release
./target/release/kupcake --detach --network test
# Verify deployment
kupcake cleanup test
```

### Linting

```bash
cargo clippy
cargo clippy --fix
```

## Contributing

We welcome contributions! See [Contributing Guide](contributing.md) for:
- Code of conduct
- Pull request process
- Coding standards
- Documentation requirements

### Quick Start

1. Fork the repository
2. Create a feature branch: `git checkout -b feature/my-feature`
3. Make your changes
4. Run tests: `cargo test && cargo clippy`
5. Commit: `git commit -am 'Add my feature'`
6. Push: `git push origin feature/my-feature`
7. Open a pull request

## Resources

### Documentation

- [Architecture Overview](../architecture/overview.md)
- [Service Documentation](../services/README.md)
- [User Guide](../user-guide/cli-reference.md)

### External Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [OP Stack Documentation](https://docs.optimism.io/)
- [bollard Documentation](https://docs.rs/bollard/)
- [tokio Documentation](https://docs.rs/tokio/)
- [clap Documentation](https://docs.rs/clap/)

## Getting Help

- **Issues**: https://github.com/op-rs/kupcake/issues
- **Discussions**: https://github.com/op-rs/kupcake/discussions
- **OP Stack Discord**: https://discord.optimism.io/

## License

Kupcake is released under the MIT License. See LICENSE for details.
