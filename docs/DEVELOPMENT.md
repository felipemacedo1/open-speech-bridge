# Development Guide

This guide explains how to set up a development environment for OpenSpeechBridge.

## Prerequisites

### Option 1: Docker (Recommended)

Docker provides a consistent Linux environment with all dependencies:

- Docker Desktop (Windows/Mac) or Docker Engine (Linux)
- 8GB RAM minimum
- 20GB disk space

### Option 2: Native Linux

- Ubuntu 22.04+ or Fedora 38+
- PipeWire 0.3.x
- Rust 1.75+
- Build tools (gcc, pkg-config, cmake)

## Quick Start with Docker

### Windows PowerShell

```powershell
# Clone the repository
git clone https://github.com/user/open-speech-bridge.git
cd open-speech-bridge

# Configure proxy (if needed)
# Edit .env file with your proxy settings

# Build the development container
.\scripts\dev.ps1 build

# Enter the container
.\scripts\dev.ps1 shell

# Inside container: build the project
cargo build
cargo test
```

### Linux/Mac

```bash
# Clone and enter
git clone https://github.com/user/open-speech-bridge.git
cd open-speech-bridge

# Start development environment
docker-compose up -d dev
docker-compose exec dev bash

# Build
cargo build
cargo test
```

## Native Linux Setup

```bash
# Ubuntu/Debian
sudo apt update
sudo apt install -y \
    build-essential \
    pkg-config \
    cmake \
    pipewire \
    pipewire-audio-client-libraries \
    libpipewire-0.3-dev \
    libspa-0.2-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Install Rust tools
rustup component add rustfmt clippy rust-analyzer

# Clone and build
git clone https://github.com/user/open-speech-bridge.git
cd open-speech-bridge
cargo build
cargo test
```

## Project Structure

```
open-speech-bridge/
├── Cargo.toml              # Workspace definition
├── crates/
│   ├── osb-core/           # Core types (no I/O)
│   ├── osb-audio/          # Audio buffers, resampling
│   ├── osb-pipewire/       # PipeWire backend
│   ├── osb-protocol/       # Engine protocol
│   ├── osb-daemon/         # Runtime
│   └── osb-cli/            # CLI
├── engines/
│   ├── mock/               # Test engine
│   └── sdk-python/         # Python SDK
├── docs/                   # Documentation
├── benchmarks/             # Performance benchmarks
└── fixtures/               # Test data
```

## Common Tasks

### Build

```bash
# Debug build (faster compile, slower runtime)
cargo build

# Release build (slower compile, faster runtime)
cargo build --release

# Build specific crate
cargo build -p osb-audio
```

### Test

```bash
# Run all tests
cargo test

# Run tests for specific crate
cargo test -p osb-audio

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_ring_buffer
```

### Lint and Format

```bash
# Check formatting
cargo fmt --check

# Fix formatting
cargo fmt

# Run clippy
cargo clippy -- -D warnings

# Run clippy with fixes
cargo clippy --fix
```

### Run CLI

```bash
# Debug build
cargo run --bin openspeechbridge -- doctor
cargo run --bin openspeechbridge -- devices
cargo run --bin openspeechbridge -- status

# Release build
cargo run --release --bin openspeechbridge -- doctor
```

### Documentation

```bash
# Generate and open docs
cargo doc --no-deps --open

# Generate docs for all dependencies
cargo doc --open
```

## IDE Setup

### VS Code

Recommended extensions:
- rust-analyzer
- CodeLLDB (debugging)
- Even Better TOML
- Docker

Settings (`.vscode/settings.json`):
```json
{
    "rust-analyzer.cargo.features": "all",
    "rust-analyzer.checkOnSave.command": "clippy"
}
```

### IntelliJ/CLion

- Install Rust plugin
- Open as Cargo project
- Configure toolchain to point to rustup

## Debugging

### VS Code

Create `.vscode/launch.json`:
```json
{
    "version": "0.2.0",
    "configurations": [
        {
            "type": "lldb",
            "request": "launch",
            "name": "Debug CLI",
            "cargo": {
                "args": ["build", "--bin=openspeechbridge", "--package=osb-cli"],
                "filter": {
                    "name": "openspeechbridge",
                    "kind": "bin"
                }
            },
            "args": ["doctor"],
            "cwd": "${workspaceFolder}"
        }
    ]
}
```

### Command Line

```bash
# With gdb
gdb --args ./target/debug/openspeechbridge doctor

# With lldb
lldb ./target/debug/openspeechbridge -- doctor
```

## Proxy Configuration

For corporate environments with proxy:

1. Copy `.env.example` to `.env`
2. Configure proxy settings:
   ```
   HTTP_PROXY=http://user:pass@proxy:3128
   HTTPS_PROXY=http://user:pass@proxy:3128
   NO_PROXY=localhost,127.0.0.1
   ```
3. Rebuild Docker container: `.\scripts\dev.ps1 build`

## Troubleshooting

### Docker build fails with network error

Check proxy configuration in `.env` file.

### PipeWire not found

Ensure PipeWire is installed and running:
```bash
systemctl --user status pipewire
pipewire --version
```

### Cargo build fails on Windows

OpenSpeechBridge requires Linux for PipeWire. Use Docker:
```powershell
.\scripts\dev.ps1 build
.\scripts\dev.ps1 shell
```

### Tests fail with permission error

Audio tests may need access to PipeWire socket:
```bash
# Check PipeWire is running
pw-cli info 0

# If using Docker, ensure volume mounts are correct
```

## Next Steps

- Read [ARCHITECTURE.md](ARCHITECTURE.md) for design overview
- Check [STATUS.md](STATUS.md) for current state
- See [CONTRIBUTING.md](../CONTRIBUTING.md) for contribution guidelines
