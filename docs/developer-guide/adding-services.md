# Adding New Services

**Target Audience**: Developers Extending Kupcake

This guide shows how to add a new service to Kupcake using the `KupcakeService` trait architecture.

## Overview

All services in Kupcake implement the `KupcakeService` trait, which provides:
- **Type-safe deployment** - Invalid stage ordering won't compile
- **Unified interface** - Consistent deployment pattern across all services
- **Serialization** - Automatic config file support
- **Context injection** - Stage-specific dependencies passed automatically

## Step-by-Step Guide

### 1. Create Service Module

Create a directory for your service in `crates/deploy/src/services/`:

```bash
mkdir -p crates/deploy/src/services/my_service
touch crates/deploy/src/services/my_service/mod.rs
touch crates/deploy/src/services/my_service/cmd.rs
```

### 2. Define Config/Builder Type

In `mod.rs`, define your service configuration (must be serializable):

```rust
use serde::{Deserialize, Serialize};
use crate::docker::DockerImage;

/// Default Docker image for my-service.
pub const DEFAULT_DOCKER_IMAGE: &str = "myorg/my-service";
/// Default Docker tag for my-service.
pub const DEFAULT_DOCKER_TAG: &str = "latest";

/// Configuration for MyService.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MyServiceConfig {
    /// Docker image configuration.
    pub docker_image: DockerImage,
    /// Container name.
    pub container_name: String,
    /// Service-specific port.
    pub port: u16,
    // ... additional config fields
}

impl Default for MyServiceConfig {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-my-service".to_string(),
            port: 8080,
        }
    }
}
```

### 3. Define Handler Type

Define the runtime handler returned after deployment:

```rust
/// Handler for a running MyService instance.
pub struct MyServiceHandler {
    /// Docker container ID.
    pub container_id: String,
    /// Docker container name.
    pub container_name: String,
    /// Service port.
    pub port: u16,
}
```

### 4. Implement Deployment Logic

Add the core deployment logic:

```rust
use std::path::PathBuf;
use anyhow::{Context, Result};
use crate::{
    docker::{CreateAndStartContainerOptions, KupDocker, ServiceConfig},
    fs::FsHandler,
};

impl MyServiceConfig {
    /// Start the service container.
    pub async fn start(
        self,
        docker: &mut KupDocker,
        host_config_path: PathBuf,
        // ... additional parameters from context
    ) -> Result<MyServiceHandler> {
        // Create config directory
        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path)?;
        }

        // Build container command (see cmd.rs)
        let cmd = MyCmdBuilder::new()
            .port(self.port)
            // ... additional args
            .build();

        // Configure container
        let service_config = ServiceConfig::new(self.docker_image.clone())
            .cmd(cmd)
            .expose(ExposedPort::tcp(self.port));

        // Start container
        let handler = docker
            .start_service(
                &self.container_name,
                service_config,
                CreateAndStartContainerOptions::default(),
            )
            .await
            .context("Failed to start MyService container")?;

        tracing::info!(
            container_id = %handler.container_id,
            container_name = %handler.container_name,
            port = %self.port,
            "MyService container started"
        );

        Ok(MyServiceHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            port: self.port,
        })
    }
}
```

### 5. Implement KupcakeService Trait

This is the **critical step** that integrates your service into the deployment pipeline:

```rust
impl crate::traits::KupcakeService for MyServiceConfig {
    // Choose the appropriate stage for your service:
    // - L1Stage: For L1 infrastructure (like Anvil)
    // - ContractsStage: For contract deployment
    // - L2Stage: For L2 nodes and services
    // - MonitoringStage: For monitoring infrastructure
    type Stage = crate::traits::L2Stage;  // Example: L2 service

    type Handler = MyServiceHandler;

    // Choose the context type matching your stage:
    // - L1Context: Provides docker, outdata, chain IDs
    // - ContractsContext: Adds anvil handler
    // - L2Context: Includes anvil handler for L2 config
    // - MonitoringContext: Adds l2_stack handler
    type Context<'a> = crate::traits::L2Context<'a>;

    const SERVICE_NAME: &'static str = "my-service";

    async fn deploy<'a>(self, ctx: Self::Context<'a>) -> anyhow::Result<Self::Handler>
    where
        Self: 'a,
    {
        let host_config_path = ctx.outdata.join("my-service");

        // Call your start method with context dependencies
        self.start(
            ctx.docker,
            host_config_path,
            // Extract other needed values from ctx (e.g., ctx.anvil, ctx.l1_chain_id)
        )
        .await
    }
}
```

**Stage Selection Guide**:
- **L1Stage**: L1 infrastructure (Anvil) - has access to basic deployment info
- **ContractsStage**: Contract deployment - needs L1 access (AnvilHandler)
- **L2Stage**: L2 nodes, batcher, proposer - needs L1 and contract info
- **MonitoringStage**: Metrics/observability - needs L2 stack info for scraping

### 6. Create Command Builder

In `cmd.rs`, create a builder for container arguments:

```rust
/// Command builder for MyService container.
pub struct MyCmdBuilder {
    port: u16,
    // ... other args
}

impl MyCmdBuilder {
    pub fn new() -> Self {
        Self {
            port: 8080,
        }
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn build(self) -> Vec<String> {
        vec![
            "my-service".to_string(),
            "--port".to_string(),
            self.port.to_string(),
            // ... additional flags
        ]
    }
}
```

### 7. Export Types

In `crates/deploy/src/services/mod.rs`, export your types:

```rust
pub mod my_service;
pub use my_service::{MyServiceConfig, MyServiceHandler};
```

### 8. Integration Options

#### Option A: Add to L2StackBuilder

If your service is part of the L2 stack, add it to `L2StackBuilder` in `crates/deploy/src/l2_stack.rs`:

```rust
pub struct L2StackBuilder {
    // ... existing fields
    pub my_service: Option<MyServiceConfig>,
}
```

Then deploy it in the `start()` method.

#### Option B: Create Custom Deployer Chain

For standalone services or custom deployment sequences:

```rust
use kupcake_deploy::traits::{Deployer, DeployChain};
use kupcake_deploy::{AnvilConfig, OpDeployerConfig, L2StackBuilder, MyServiceConfig};

// Create custom chain
let deployer = Deployer::new(AnvilConfig::default())
    .then(OpDeployerConfig::default())
    .then(L2StackBuilder::default())
    .then(MyServiceConfig::default());

// Deploy
let result = deployer.deploy_chain(
    &mut docker,
    outdata,
    l1_chain_id,
    l2_chain_id,
    None,
).await?;
```

**Note**: The `.then()` method enforces compile-time stage ordering. You can only chain services whose stages follow the `NextStage` transitions.

## Complete Example

Here's a complete minimal example for a new monitoring service:

```rust
// crates/deploy/src/services/jaeger/mod.rs
use serde::{Deserialize, Serialize};
use crate::docker::DockerImage;

pub const DEFAULT_DOCKER_IMAGE: &str = "jaegertracing/all-in-one";
pub const DEFAULT_DOCKER_TAG: &str = "latest";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JaegerConfig {
    pub docker_image: DockerImage,
    pub container_name: String,
    pub ui_port: u16,
}

impl Default for JaegerConfig {
    fn default() -> Self {
        Self {
            docker_image: DockerImage::new(DEFAULT_DOCKER_IMAGE, DEFAULT_DOCKER_TAG),
            container_name: "kupcake-jaeger".to_string(),
            ui_port: 16686,
        }
    }
}

pub struct JaegerHandler {
    pub container_id: String,
    pub container_name: String,
    pub ui_port: u16,
}

impl JaegerConfig {
    pub async fn start(
        self,
        docker: &mut crate::KupDocker,
        host_config_path: std::path::PathBuf,
    ) -> anyhow::Result<JaegerHandler> {
        use crate::docker::{CreateAndStartContainerOptions, ExposedPort, ServiceConfig};
        use crate::fs::FsHandler;

        if !host_config_path.exists() {
            FsHandler::create_host_config_directory(&host_config_path)?;
        }

        let service_config = ServiceConfig::new(self.docker_image.clone())
            .expose(ExposedPort::tcp(self.ui_port));

        let handler = docker
            .start_service(
                &self.container_name,
                service_config,
                CreateAndStartContainerOptions::default(),
            )
            .await?;

        Ok(JaegerHandler {
            container_id: handler.container_id,
            container_name: handler.container_name,
            ui_port: self.ui_port,
        })
    }
}

// Implement the trait - Jaeger is a monitoring service
impl crate::traits::KupcakeService for JaegerConfig {
    type Stage = crate::traits::MonitoringStage;
    type Handler = JaegerHandler;
    type Context<'a> = crate::traits::MonitoringContext<'a>;

    const SERVICE_NAME: &'static str = "jaeger";

    async fn deploy<'a>(self, ctx: Self::Context<'a>) -> anyhow::Result<Self::Handler>
    where
        Self: 'a,
    {
        let host_config_path = ctx.outdata.join("jaeger");
        self.start(ctx.docker, host_config_path).await
    }
}
```

## Best Practices

### Error Handling
- **Never use `.unwrap()` or `.expect()`** - use `?` and `.context()`
- Add descriptive context to all errors
- Use `anyhow::Result` for return types

```rust
// Good
let value = read_config(&path)
    .context("Failed to read service configuration")?;

// Bad
let value = read_config(&path).unwrap();
```

### Logging
- Use structured logging with `tracing`
- Include relevant context (container name, ports, etc.)

```rust
tracing::info!(
    container_id = %handler.container_id,
    container_name = %handler.container_name,
    "Service started successfully"
);
```

### Configuration
- Make all config fields public for serialization
- Provide sensible defaults
- Use `DockerImage` type for image configuration
- Add `#[serde(default, skip_serializing_if = "...")]` for optional fields

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyConfig {
    pub docker_image: DockerImage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional_field: Option<String>,
}
```

### Container Naming
- Use consistent naming: `kupcake-{service-name}`
- For multiple instances, add suffix: `kupcake-{service}-{index}`

### Port Management
- Define default ports as constants
- Use `ExposedPort` for Docker network exposure
- Use `PortMapping` only for host-accessible ports

## Testing

Test your service integration:

```bash
# Build
cargo build --release

# Test with custom config
./target/release/kupcake \
    --network test-my-service \
    --no-cleanup

# Verify container is running
docker ps | grep kupcake-my-service

# Check logs
docker logs kupcake-my-service

# Cleanup
docker stop kupcake-my-service
docker rm kupcake-my-service
```

## Common Pitfalls

1. **Forgetting to implement KupcakeService** - Service won't be usable in chains
2. **Wrong stage type** - Results in compile errors or incorrect dependency access
3. **Missing serialization** - Config won't save to Kupcake.toml correctly
4. **Not adding context** - Errors will be hard to debug
5. **Using `.unwrap()`** - Violates project standards and causes panics

## Related Documentation

- [Architecture Overview](../architecture/overview.md) - Understanding the trait architecture
- [Code Style Guide](code-style.md) - Rust coding standards
- [Docker Integration](docker-integration.md) - Using bollard
- [Service Coordination](../architecture/service-coordination.md) - How services communicate
