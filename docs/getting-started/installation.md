# Installation Guide

**Target Audience**: New Users
**Prerequisites**: Basic command line knowledge

## System Requirements

### Required
- **Docker**: Version 20.10 or higher (for container orchestration)
- **Rust**: Version 1.75 or higher (for building Kupcake)
- **Git**: For cloning the repository

### Recommended
- **8 GB RAM**: Minimum for running all services
- **20 GB Disk Space**: For Docker images and blockchain data
- **Linux or macOS**: Primary development platforms (Windows via WSL2 also works)

## Installing Prerequisites

### Docker

#### Linux (Ubuntu/Debian)
```bash
# Install Docker
curl -fsSL https://get.docker.com -o get-docker.sh
sudo sh get-docker.sh

# Add your user to the docker group (avoid using sudo)
sudo usermod -aG docker $USER
newgrp docker

# Verify installation
docker --version
docker ps
```

#### macOS
```bash
# Install Docker Desktop
brew install --cask docker

# Open Docker Desktop
open -a Docker

# Verify installation
docker --version
docker ps
```

#### Windows (WSL2)
1. Install [Docker Desktop for Windows](https://www.docker.com/products/docker-desktop)
2. Enable WSL2 integration in Docker Desktop settings
3. Open WSL2 terminal and verify:
```bash
docker --version
docker ps
```

### Rust

The easiest way to install Rust is via `rustup`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Follow the prompts, then reload your shell
source $HOME/.cargo/env

# Verify installation
rustc --version
cargo --version
```

Should show Rust 1.75 or higher.

### Git

#### Linux (Ubuntu/Debian)
```bash
sudo apt-get update
sudo apt-get install git
```

#### macOS
```bash
# Git is included with Xcode Command Line Tools
xcode-select --install

# Or install via Homebrew
brew install git
```

#### Verify Installation
```bash
git --version
```

## Building Kupcake

### 1. Clone the Repository

```bash
git clone https://github.com/op-rs/kupcake.git
cd kupcake
```

### 2. Build Release Binary

```bash
cargo build --release
```

This will take several minutes on the first build. The compiled binary will be at:
```
./target/release/kupcake
```

### 3. Verify the Build

```bash
./target/release/kupcake --version
```

Should output the version number (e.g., `kup 0.1.0`).

### 4. (Optional) Install Globally

To use `kupcake` from anywhere:

```bash
cargo install --path .
```

Then you can run:
```bash
kupcake --version
```

## Development Build (Optional)

For faster compilation during development:

```bash
# Build debug binary (faster compilation, slower runtime)
cargo build

# Run directly
./target/debug/kupcake --version

# Or use cargo run
cargo run -- --version
```

## Using Just (Optional Build Tool)

Kupcake includes a `justfile` for convenient build commands:

### Install Just

```bash
cargo install just
```

### Available Commands

```bash
# Build release binary
just build

# Run development build with args
just run-dev --network testnet

# Run tests
just test

# Run linter
just lint

# Run linter with auto-fix
just fix
```

## Verifying the Installation

After building, verify Docker access:

```bash
./target/release/kupcake --help
```

Should display the help message with all available commands and options.

Try a dry run to check Docker connectivity:

```bash
docker ps
```

Should list running containers (may be empty).

## Troubleshooting

### Rust Compilation Errors

#### Missing Linker
```
error: linker `cc` not found
```

**Solution**: Install build tools:
```bash
# Ubuntu/Debian
sudo apt-get install build-essential

# macOS
xcode-select --install
```

#### OpenSSL Not Found
```
error: failed to run custom build command for `openssl-sys`
```

**Solution**: Install OpenSSL development packages:
```bash
# Ubuntu/Debian
sudo apt-get install pkg-config libssl-dev

# macOS
brew install openssl
```

### Docker Permission Denied

```
Error: Permission denied while trying to connect to the Docker daemon
```

**Solution**: Add your user to the docker group:
```bash
sudo usermod -aG docker $USER
newgrp docker
```

Or use Docker Desktop which handles permissions automatically.

### Docker Not Running

```
Error: Cannot connect to the Docker daemon. Is the docker daemon running?
```

**Solution**: Start Docker:
```bash
# Linux (systemd)
sudo systemctl start docker

# macOS
open -a Docker

# Windows
# Start Docker Desktop from Start Menu
```

### Cargo Build is Slow

First builds are slow because Rust compiles all dependencies. Subsequent builds are much faster.

To speed up builds:
```bash
# Use more CPU cores (default is # of CPUs)
cargo build --release -j$(nproc)

# Or use a faster linker (optional)
cargo install -f cargo-binutils
```

## Next Steps

- [**Quickstart Guide**](quickstart.md) - Deploy your first L2 in 5 minutes
- [First Deployment](first-deployment.md) - Detailed walkthrough
- [CLI Reference](../user-guide/cli-reference.md) - All available commands and options

## Additional Resources

- [Rust Installation Guide](https://www.rust-lang.org/tools/install)
- [Docker Installation Guide](https://docs.docker.com/get-docker/)
- [WSL2 Setup for Windows](https://docs.microsoft.com/en-us/windows/wsl/install)
