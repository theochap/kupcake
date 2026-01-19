# Kupcake Documentation

Welcome to the comprehensive documentation for **Kupcake** - a CLI tool that bootstraps a complete Rust-based OP Stack (Optimism) L2 chain locally.

## Quick Navigation

### 🚀 Getting Started
Perfect for new users who want to deploy their first L2 chain.

- [**Quickstart**](getting-started/quickstart.md) - Deploy your first L2 in under 5 minutes
- [Installation](getting-started/installation.md) - Prerequisites and build instructions
- [First Deployment](getting-started/first-deployment.md) - Guided walkthrough
- [Understanding Output](getting-started/understanding-output.md) - What gets deployed and where

### 📖 User Guide
Comprehensive guides for operators and advanced users.

- [CLI Reference](user-guide/cli-reference.md) - Complete CLI arguments documentation
- [Environment Variables](user-guide/environment-variables.md) - KUP_* environment variables
- [Configuration File](user-guide/configuration-file.md) - Kupcake.toml structure and usage
- [L1 Sources](user-guide/l1-sources.md) - Forking Sepolia, Mainnet, or custom RPC
- [Multi-Sequencer Setup](user-guide/multi-sequencer.md) - Running multiple sequencers with op-conductor
- [Port Management](user-guide/port-management.md) - Port mappings and networking
- [Docker Images](user-guide/docker-images.md) - Using custom Docker images
- [Monitoring](user-guide/monitoring.md) - Prometheus and Grafana setup
- [MCP Integration](user-guide/mcp-integration.md) - Expose docs to AI assistants via MCP
- [Cleanup and Restart](user-guide/cleanup-and-restart.md) - Lifecycle management
- [Troubleshooting](user-guide/troubleshooting.md) - Common issues and solutions

### 🏗️ Architecture
Deep dives into how Kupcake works internally.

- [Overview](architecture/overview.md) - High-level architecture
- [Deployment Flow](architecture/deployment-flow.md) - Step-by-step deployment sequence
- [Component Hierarchy](architecture/component-hierarchy.md) - Builder/Config/Handler patterns
- [Docker Networking](architecture/docker-networking.md) - Container networking model
- [Data Persistence](architecture/data-persistence.md) - File system structure
- [Service Coordination](architecture/service-coordination.md) - How services communicate

### ⚙️ Service Reference
Detailed documentation for each service component.

- [Services Overview](services/README.md) - Service patterns and common configuration
- [Anvil](services/anvil.md) - L1 fork (Foundry's Anvil)
- [op-deployer](services/op-deployer.md) - OP Stack contract deployment
- [op-reth](services/op-reth.md) - L2 execution client
- [kona-node](services/kona-node.md) - L2 consensus client
- [op-batcher](services/op-batcher.md) - Transaction batching to L1
- [op-proposer](services/op-proposer.md) - State root proposals to L1
- [op-challenger](services/op-challenger.md) - Fault proof challenges
- [op-conductor](services/op-conductor.md) - Multi-sequencer coordination (Raft)
- [Prometheus](services/prometheus.md) - Metrics collection
- [Grafana](services/grafana.md) - Metrics visualization

### 💡 Examples
Runnable examples demonstrating common use cases.

- [Examples Overview](examples/README.md) - How to run and test examples
- [Basic Deployment](examples/basic-deployment/) - Simplest deployment with defaults
- [Mainnet Fork](examples/mainnet-fork/) - Fork Ethereum mainnet
- [Single Sequencer](examples/single-sequencer/) - Deploy with only one sequencer
- [Multi-Sequencer](examples/multi-sequencer/) - Multiple sequencers with op-conductor
- [Custom Images](examples/custom-images/) - Override Docker images
- [Fast Blocks](examples/fast-blocks/) - Configure faster block times
- [Local Mode](examples/local-mode/) - Run without L1 fork

### 👩‍💻 Developer Guide
For contributors and those extending Kupcake.

- [Developer Overview](developer-guide/README.md) - Getting started with development
- [Project Structure](developer-guide/project-structure.md) - Codebase organization
- [Build and Test](developer-guide/build-and-test.md) - Build commands and testing
- [Adding Services](developer-guide/adding-services.md) - How to add new services
- [Builder Pattern](developer-guide/builder-pattern.md) - Design patterns used
- [Docker Integration](developer-guide/docker-integration.md) - Using bollard
- [Configuration Schema](developer-guide/configuration-schema.md) - Serde/TOML patterns
- [Error Handling](developer-guide/error-handling.md) - Error handling patterns
- [Code Style](developer-guide/code-style.md) - Rust style guide
- [Contributing](developer-guide/contributing.md) - How to contribute

### 📚 API Reference
Type and API documentation.

- [Deployer API](api/deployer.md) - Main Deployer interface
- [Builder Types](api/builder-types.md) - All Builder types
- [Config Types](api/config-types.md) - All Config types
- [Handler Types](api/handler-types.md) - All Handler types

## Additional Resources

- [Main README](../README.md) - Project overview and quick examples
- [CLAUDE.md](../CLAUDE.md) - Instructions for AI assistants
- [GitHub Repository](https://github.com/op-rs/kupcake) - Source code and issues

## Documentation Standards

All documentation in this repository follows these standards:

- **Target Audience**: Each document clearly states its intended audience (New Users | Operators | Developers)
- **Runnable Examples**: Code examples are tested and runnable
- **Code References**: Links to specific source files using `path/file.rs:line` format
- **Troubleshooting**: Common issues included where relevant
- **Navigation**: Clear links to related documentation

## Contributing to Documentation

Found an issue or want to improve the docs? Contributions are welcome!

1. All documentation is in Markdown format
2. Examples must be runnable and testable
3. Follow the existing structure and style
4. Test your changes before submitting

See [Contributing Guide](developer-guide/contributing.md) for details.
