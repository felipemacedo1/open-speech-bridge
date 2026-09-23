# ADR-0002: Linux with PipeWire as Primary Platform

## Status

Accepted

## Context

We need to choose an initial target platform for OpenSpeechBridge. Requirements:

- Create virtual audio devices
- Capture from physical microphones
- Play to physical outputs
- Integrate with common applications (Discord, Zoom, browsers)
- Support real-time audio with low latency

Platform options:
- Linux (PipeWire, PulseAudio, ALSA)
- Windows (WASAPI, virtual audio cable)
- macOS (CoreAudio, virtual audio)

## Decision

**Target Linux with PipeWire as the primary platform.**

Other platforms will be supported in future milestones.

## Alternatives Considered

### Windows First
- ✅ Largest desktop market share
- ✅ Good documentation
- ❌ Virtual audio devices require third-party software
- ❌ More complex audio stack
- ❌ Harder to develop on Linux/Mac

### macOS First
- ✅ Good audio stack
- ❌ Requires code signing for virtual devices
- ❌ Smaller market share
- ❌ Development requires Mac hardware

### Cross-Platform from Start
- ✅ Wider reach immediately
- ❌ Significant engineering effort
- ❌ Delays initial release
- ❌ Harder to get anything working

### Linux with PipeWire
- ✅ Modern audio stack
- ✅ Native virtual device support
- ✅ Well-documented API
- ✅ Good Rust bindings
- ✅ Used by Fedora, Ubuntu 22.04+
- ✅ Easier development environment
- ⚠️ Smaller desktop market share
- ⚠️ Requires relatively recent distro

## Consequences

### Positive
- Can leverage PipeWire's virtual device capabilities
- Good Rust bindings available (pipewire-rs)
- Active community and development
- Can test with Docker
- Contributors likely have Linux available

### Negative
- Excludes Windows/Mac users initially
- Some users on older distros may need PipeWire
- Need to abstract audio backend for future platforms

### Mitigation
- Document PipeWire installation
- Provide Docker environment for development
- Design audio abstraction for future backends
- Add Windows/Mac in Milestone 8

## Platform Support Timeline

| Milestone | Platform |
|-----------|----------|
| M1-M7 | Linux (PipeWire) |
| M8 | Windows (WASAPI) |
| M8 | macOS (CoreAudio) |

## Related Decisions

- ADR-0001: Rust as primary language (good PipeWire bindings)
