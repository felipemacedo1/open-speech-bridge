# Contributing to OpenSpeechBridge

Thank you for your interest in contributing to OpenSpeechBridge! This document provides guidelines for contributing to the project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Making Changes](#making-changes)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)
- [Testing](#testing)
- [Documentation](#documentation)

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating, you are expected to uphold this code.

## Getting Started

1. **Read the documentation**
   - [README.md](README.md) - Project overview
   - [AGENTS.md](AGENTS.md) - Development guidelines
   - [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - System design
   - [docs/STATUS.md](docs/STATUS.md) - Current state

2. **Find something to work on**
   - Check [Issues](https://github.com/user/open-speech-bridge/issues) for open tasks
   - Look for `good first issue` or `help wanted` labels
   - Comment on an issue before starting work

3. **Fork and clone**
   ```bash
   git clone https://github.com/YOUR_USERNAME/open-speech-bridge.git
   cd open-speech-bridge
   ```

## Development Setup

### Option 1: Docker (Recommended)

Docker provides a consistent development environment with all dependencies:

```powershell
# Windows PowerShell
.\scripts\dev.ps1 build   # Build container
.\scripts\dev.ps1 shell   # Enter container

# Inside container
cargo build
cargo test
```

### Option 2: Native Linux

```bash
# Ubuntu/Debian
sudo apt install pipewire pipewire-audio-client-libraries libpipewire-0.3-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup component add rustfmt clippy

# Build
cargo build
cargo test
```

## Making Changes

### Branch Naming

Use descriptive branch names:

```
feat/add-vad-support
fix/buffer-underrun
docs/update-architecture
refactor/engine-protocol
```

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(audio): add lock-free ring buffer implementation

Add a SPSC ring buffer using the ringbuf crate for real-time
audio transfer between capture and processing threads.

- Bounded capacity to prevent memory issues
- Metrics for overflow/underrun detection
- Tests for concurrent access patterns
```

Types: `feat`, `fix`, `docs`, `refactor`, `perf`, `test`, `chore`

Scopes: `core`, `audio`, `pipewire`, `protocol`, `daemon`, `cli`, `engine`, `ci`, `docs`

### Before Committing

```bash
# Format code
cargo fmt

# Run linter
cargo clippy -- -D warnings

# Run tests
cargo test

# Check for issues
cargo build --all-targets
```

## Pull Request Process

1. **Create a focused PR**
   - One logical change per PR
   - Keep changes small and reviewable
   - Link to related issues

2. **Fill out the PR template**
   - Describe what changed and why
   - List testing performed
   - Note any breaking changes

3. **Ensure CI passes**
   - All tests must pass
   - No new clippy warnings
   - Code is formatted

4. **Update documentation**
   - Update relevant docs
   - Update `docs/STATUS.md` if needed
   - Add ADR for architectural changes

5. **Respond to reviews**
   - Address all feedback
   - Keep discussion constructive

## Coding Standards

### Rust Style

- Follow `rustfmt` defaults
- Use `clippy` suggestions
- Prefer explicit error handling over `.unwrap()`
- Document public APIs with `///` comments
- Keep functions focused and small

### Architecture Rules

- **Real-time safety**: Never block audio callbacks
- **Bounded buffers**: All queues have fixed capacity
- **Engine isolation**: Runtime doesn't depend on specific models
- **License compliance**: Verify dependencies are Apache-2.0 compatible

### Error Handling

```rust
// Good: Explicit error handling
pub fn process(&mut self, data: &[u8]) -> Result<Output, Error> {
    let parsed = parse_data(data)?;
    Ok(transform(parsed))
}

// Bad: Panicking on errors
pub fn process(&mut self, data: &[u8]) -> Output {
    let parsed = parse_data(data).unwrap(); // Don't do this
    transform(parsed)
}
```

### Real-Time Audio Code

```rust
// Good: Non-blocking, bounded
pub fn on_audio_callback(&mut self, samples: &[f32]) {
    // Try to push, drop if buffer full
    let written = self.buffer.push(samples);
    if written < samples.len() {
        self.metrics.record_dropped((samples.len() - written) as u64);
    }
}

// Bad: Blocking, unbounded
pub fn on_audio_callback(&mut self, samples: &[f32]) {
    // NEVER do this in audio callbacks
    self.channel.send(samples.to_vec()).await; // Blocks!
    self.file.write_all(samples);              // I/O!
    self.engine.process(samples);              // ML inference!
}
```

## Testing

### Test Organization

```
crates/osb-core/src/
├── audio.rs           # Implementation
└── audio/tests.rs     # Unit tests (or inline #[cfg(test)])

tests/
└── integration/       # Integration tests
```

### Writing Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_push_pop() {
        let (mut producer, mut consumer) = AudioRingBuffer::new(1024, 2);
        
        let input = vec![0.5f32; 256];
        producer.push(&input);
        
        let mut output = vec![0.0f32; 256];
        consumer.pop(&mut output);
        
        assert_eq!(input, output);
    }

    #[test]
    fn test_buffer_overflow_metrics() {
        let (mut producer, consumer) = AudioRingBuffer::new(64, 1);
        
        // Overflow the buffer
        producer.push(&vec![1.0f32; 128]);
        
        let metrics = consumer.metrics().snapshot();
        assert!(metrics.overruns > 0);
    }
}
```

### Running Tests

```bash
# All tests
cargo test

# Specific crate
cargo test -p osb-audio

# With output
cargo test -- --nocapture

# Single test
cargo test test_buffer_push_pop
```

## Documentation

### Code Documentation

```rust
/// A lock-free ring buffer for real-time audio.
///
/// This buffer uses a SPSC (single-producer, single-consumer) design
/// suitable for passing audio between threads without blocking.
///
/// # Example
///
/// ```
/// let (mut producer, mut consumer) = AudioRingBuffer::new(1024, 2);
/// producer.push(&samples);
/// consumer.pop(&mut output);
/// ```
///
/// # Panics
///
/// Does not panic under normal operation.
pub struct AudioRingBuffer { ... }
```

### Architecture Decision Records

For significant decisions, create an ADR:

```markdown
# ADR-XXXX: Title

## Status
Proposed | Accepted | Deprecated | Superseded

## Context
What is the issue we're addressing?

## Decision
What is the change we're proposing?

## Alternatives Considered
What other options were evaluated?

## Consequences
What are the tradeoffs?
```

## Questions?

- Open a [Discussion](https://github.com/user/open-speech-bridge/discussions)
- Check existing [Issues](https://github.com/user/open-speech-bridge/issues)
- Review [ADRs](docs/adr/) for past decisions

Thank you for contributing! 🎉
