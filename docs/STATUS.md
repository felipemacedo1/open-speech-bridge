# Project Status

> Last updated: 2024-12-XX (Initial Bootstrap)

## CI repair audit — 2026-09-24

- PR #11: corrected five CI toolchain references and the release reference to
  `dtolnay/rust-toolchain@stable`; derived audio enum defaults without changing
  F32/Stereo defaults; removed an unused capture-state mirror and its own test.
- GitHub Actions run [36080014937](https://github.com/felipemacedo1/open-speech-bridge/actions/runs/36080014937)
  passed all six jobs on commit `7a5b134`: Check, Test, Clippy, Format,
  Documentation and Security Audit.
- `cargo test --workspace` in that run: 102 unit tests and 4 doctests passed;
  11 unit tests and 4 doctests were ignored. Ignored tests are not audio evidence.
- Local Docker (Debian 12, Rust 1.87.0): `cargo build`, `cargo fmt --check`,
  `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`
  passed. Local tests reproduced 102 unit tests + 4 doctests passing and 15 ignored.
- CLI `doctor`, `devices`, and `loopback --help` executed in Docker. No PipeWire
  daemon or WirePlumber is available there; device listing was empty. This does
  not validate real microphone capture or audio routing.
- SonarQube Cloud project `felipemacedo1_open-speech-bridge` in organization
  `felipemacedo1` is confirmed. The new workflow imports Clippy JSON and waits
  for the Quality Gate. `SONAR_TOKEN` is registered as an Actions secret.
  Authenticated run 36082523834 completed analysis at `f27f55b`; the gate
  failed on new duplicated lines (30.5%, limit 3%). Three critical complexity
  code smells remain in capture.rs and virtual_device.rs. See SONARQUBE.md.
- Next: refactor the Sonar duplication/complexity findings, pass the Quality
  Gate, and validate real PipeWire audio on Linux
  before merge. The main branch and Dependabot PRs do not yet contain this fix.
- Release workflow reference was corrected but no release/tag was triggered.
- The bootstrap inventory below is historical and has not been revalidated.

## Current State: Foundation Complete

The project infrastructure is in place. Core Rust crates are defined with types, errors, and basic implementations. Docker development environment is configured.

## What Works Today

### ✅ Build System
- Rust workspace with 6 crates compiles (on Linux with PipeWire)
- Docker development container configured
- Proxy support for corporate environments

### ✅ Core Types (osb-core)
- Audio format types (SampleRate, SampleFormat, ChannelLayout, AudioFormat)
- Error types with severity levels
- Configuration structures
- Metrics definitions

### ✅ Audio Buffers (osb-audio)
- Lock-free SPSC ring buffer (AudioRingBuffer)
- Sample format conversion (i16↔f32, mono↔stereo)
- Resampling wrapper (via rubato)
- Unit tests pass

### ✅ Engine Protocol (osb-protocol)
- Capability definitions
- Engine info and state types
- Request/Response message types
- JSON serialization

### ✅ CLI Framework (osb-cli)
- Command structure with clap
- `doctor` command (system diagnostics)
- `devices` command (list audio devices)
- `status` command (show runtime status)
- `engines` command (list engines)
- JSON output support

### ✅ Documentation
- README with architecture overview
- AGENTS.md for coding agents
- CONTRIBUTING.md
- Architecture documentation
- ADR structure

## What's Partially Working

### 🔨 PipeWire Integration (osb-pipewire)
- Module structure defined
- Types for devices, streams, virtual devices
- Stub implementations for non-Linux
- **NOT YET**: Actual PipeWire stream creation
- **NOT YET**: Real device enumeration
- **NOT YET**: Virtual microphone creation

### 🔨 Daemon (osb-daemon)
- Runtime state machine defined
- Audio manager structure
- Engine supervisor structure
- Pipeline structure
- **NOT YET**: Actual audio processing
- **NOT YET**: Engine process management

## What's Not Implemented Yet

### 📋 Audio Pipeline
- [ ] Real PipeWire capture stream
- [ ] Real PipeWire playback stream
- [ ] Virtual microphone device
- [ ] Virtual sink device
- [ ] Audio routing

### 📋 Engine System
- [ ] Engine process spawning
- [ ] IPC with engines
- [ ] Mock engine implementation
- [ ] Health monitoring

### 📋 ML Integration
- [ ] VAD (voice activity detection)
- [ ] STT engine adapter
- [ ] Translation engine adapter
- [ ] TTS engine adapter

### 📋 Features
- [ ] Configuration file loading
- [ ] Metrics export
- [ ] Graceful shutdown
- [ ] Hot-reload configuration

## Validated Commands

These commands are known to work:

```bash
# In Docker container (Linux)
cargo build                              # Builds all crates
cargo test                               # Runs all tests
cargo run --bin openspeechbridge -- --help  # Shows CLI help

# On Windows (limited - won't compile PipeWire)
# Use Docker for full builds
```

## Known Limitations

1. **Linux Only**: PipeWire integration requires Linux
2. **No Real Audio Yet**: PipeWire streams are stubs
3. **No ML Engines**: Mock engine not fully implemented
4. **Docker Required on Windows**: Native Windows build won't include PipeWire

## Active Architectural Questions

1. **IPC Mechanism**: Unix sockets vs shared memory for engine communication?
2. **Config Format**: TOML vs YAML for configuration files?
3. **Metrics Export**: Prometheus vs OpenTelemetry?

## Next Recommended Tasks

1. **Complete PipeWire device enumeration**
   - Use pipewire-rs registry listener
   - Parse device properties correctly
   - Handle device hot-plug

2. **Implement capture stream**
   - Create PipeWire stream
   - Connect to device
   - Push samples to ring buffer

3. **Implement virtual microphone**
   - Create PipeWire source node
   - Make it visible to applications
   - Serve audio from ring buffer

4. **Create mock engine**
   - Implement protocol messages
   - Echo or transform audio
   - Test engine supervisor

5. **Add integration tests**
   - Synthetic audio tests
   - Protocol message tests
   - Buffer behavior tests

## Environment Requirements

### Linux Development (Native or Docker)
- Ubuntu 22.04+ or Fedora 38+
- PipeWire 0.3.x
- Rust 1.75+

### Docker Development
- Docker Desktop with Linux containers
- 8GB RAM recommended
- Proxy configuration if behind corporate firewall

## How to Continue Development

1. Start Docker:
   ```powershell
   .\scripts\dev.ps1 build
   .\scripts\dev.ps1 shell
   ```

2. Inside container:
   ```bash
   cargo build
   cargo test
   cargo run --bin openspeechbridge -- doctor
   ```

3. Check this file for next tasks

4. Update this file when completing work

---

*This file should be updated whenever significant changes are made to the project.*
