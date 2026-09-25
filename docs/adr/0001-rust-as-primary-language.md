# ADR-0001: Rust as Primary Implementation Language

## Status

Accepted

## Context

We need to choose a primary language for implementing the OpenSpeechBridge runtime. The runtime must handle:

- Real-time audio capture and playback
- Low-latency buffer management
- PipeWire integration
- Process management for ML engines
- CLI interface

Key requirements:
- Real-time safety (no GC pauses)
- Memory safety
- Good PipeWire bindings
- Cross-platform potential
- Strong type system

## Decision

**Use Rust as the primary implementation language for the runtime.**

Python will be used only for ML engine implementations where the PyTorch/ML ecosystem provides significant benefits.

## Alternatives Considered

### C++
- ✅ Maximum performance
- ✅ Mature PipeWire bindings
- ❌ Memory safety issues
- ❌ Build system complexity
- ❌ Harder to contribute to

### Go
- ✅ Good concurrency model
- ✅ Easy to learn
- ❌ GC pauses problematic for real-time audio
- ❌ No good PipeWire bindings
- ❌ Less control over memory layout

### Python
- ✅ Excellent ML ecosystem
- ✅ Easy to prototype
- ❌ GC and GIL problematic for real-time
- ❌ Not suitable for audio callbacks
- ❌ Performance overhead

### Rust
- ✅ Memory safety without GC
- ✅ Zero-cost abstractions
- ✅ Good PipeWire bindings (pipewire-rs)
- ✅ Strong type system catches bugs early
- ✅ Modern tooling (cargo, rustfmt, clippy)
- ⚠️ Steeper learning curve
- ⚠️ Smaller talent pool

## Consequences

### Positive
- Real-time audio callbacks are safe to write
- Memory bugs caught at compile time
- Fearless concurrency
- Good ecosystem for CLI, async, serialization
- Attracts contributors who value quality

### Negative
- Contributors need Rust knowledge
- Some ML libraries require FFI or Python subprocess
- Compile times can be slow
- Some platform-specific code needed

### Mitigation
- Provide Docker environment with tooling
- Use Python for ML engines via subprocess
- Document architecture clearly
- Accept PRs that improve build times

## Related Decisions

- ADR-0003: Engine isolation (Python engines in separate processes)
