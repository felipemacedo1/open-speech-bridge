# Contributing to OpenSpeechBridge

Thank you for your interest in contributing! This document provides guidelines for contributing to the project.

## Getting Started

### Prerequisites

- **Linux** with PipeWire (Ubuntu 22.04+ recommended)
- **Rust 1.75+** (we use latest stable features)
- **Docker** (optional, for development container)

### Development Setup

```bash
# Clone the repository
git clone https://github.com/open-speech-bridge/open-speech-bridge.git
cd open-speech-bridge

# Install system dependencies (Ubuntu/Debian)
sudo apt install pipewire libpipewire-0.3-dev libspa-0.2-dev libclang-dev

# Build the project
cargo build

# Run tests
cargo test --workspace

# Run clippy
cargo clippy --workspace --all-targets

# Check formatting
cargo fmt --check
```

### Using Docker (Windows/macOS)

If you're not on Linux, use our development container:

```bash
docker compose up -d dev
docker compose exec dev cargo build
docker compose exec dev cargo test
```

## How to Contribute

### Reporting Bugs

1. Check if the issue already exists in [Issues](https://github.com/felipemacedo1/open-speech-bridge/issues)
2. Create a new issue with:
   - Clear, descriptive title
   - Steps to reproduce
   - Expected vs actual behavior
   - System information (OS, PipeWire version, Rust version)

### Suggesting Features

1. Open a [Discussion](https://github.com/felipemacedo1/open-speech-bridge/discussions) first
2. Describe the use case and motivation
3. If there's consensus, create an issue for tracking

### Submitting Pull Requests

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Make your changes
4. Ensure all checks pass:
   ```bash
   cargo fmt
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo doc --workspace --no-deps
   ```
5. Commit with a descriptive message (see commit guidelines below)
6. Push and open a PR

## Code Guidelines

### Rust Style

- Follow [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `rustfmt` defaults (no custom configuration)
- No `clippy::allow` without justification comment
- Prefer explicit types in public APIs
- Use `#[must_use]` for functions with important return values

### Documentation

- All public items must have doc comments
- Include examples for non-trivial APIs
- Use `# Errors` and `# Panics` sections where applicable
- Keep doc comments concise but complete

### Error Handling

- Use `thiserror` for error types
- Provide context in error messages
- Avoid `.unwrap()` outside of tests
- Use `anyhow` only in binaries, not libraries

### Testing

- Write unit tests for new functionality
- Place tests in the same file using `#[cfg(test)]` module
- Use descriptive test names: `test_buffer_overflow_drops_oldest`
- Test edge cases and error conditions

### Performance

- No allocations in real-time audio paths
- Use `#[inline]` judiciously (profile first)
- Prefer stack allocation for small, fixed-size buffers
- Document any `unsafe` code thoroughly

## Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

### Types

- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `style`: Formatting, no code change
- `refactor`: Code change that neither fixes a bug nor adds a feature
- `perf`: Performance improvement
- `test`: Adding or correcting tests
- `chore`: Maintenance tasks
- `ci`: CI/CD changes

### Examples

```
feat(audio): add stereo-to-mono conversion

fix(pipewire): handle device disconnection gracefully

docs(readme): update installation instructions

refactor(protocol): extract capability validation
```

## Project Structure

```
open-speech-bridge/
├── crates/
│   ├── osb-core/      # Core types, errors, config
│   ├── osb-audio/     # Audio buffers and processing
│   ├── osb-protocol/  # Engine protocol definitions
│   ├── osb-pipewire/  # PipeWire integration
│   ├── osb-daemon/    # Background runtime
│   └── osb-cli/       # Command-line interface
├── docs/              # Documentation
└── examples/          # Usage examples
```

### Crate Dependencies

```
osb-cli ──▶ osb-daemon ──▶ osb-pipewire ──▶ osb-audio ──▶ osb-core
                │                                │
                └──────▶ osb-protocol ◀──────────┘
```

## Review Process

1. All PRs require at least one review
2. CI must pass (build, test, clippy, fmt, docs)
3. Keep PRs focused — one feature/fix per PR
4. Respond to feedback promptly
5. Squash commits if requested

## License

By contributing, you agree that your contributions will be licensed under the Apache License 2.0.

## Questions?

- Open a [Discussion](https://github.com/felipemacedo1/open-speech-bridge/discussions)
- Check existing [Issues](https://github.com/felipemacedo1/open-speech-bridge/issues)

Thank you for contributing! 🎉
