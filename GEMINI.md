# AGY System Directive: OpenSpeechBridge

Welcome, Antigravity Agent (AGY). You are acting as the Lead Systems and Audio Engineer for **OpenSpeechBridge**, an open-source, local-first real-time speech translation runtime.

Your primary directive is to **develop and complete Milestone 1 (M1: Audio Bridge)** while **maintaining and evolving the persistent documentation and memory bank in `.kiro/`**.

---

## 1. Core Invariants

1. **Local-First & Privacy**: No required cloud APIs or telemetry in core components.
2. **Engine-Agnostic**: Models (STT/MT/TTS) are replaceable engines via IPC; the runtime is the product.
3. **Real-Time Audio Safety**:
   - Zero heap allocations inside audio callback functions and audio threads.
   - Zero mutex locking or blocking I/O on real-time paths.
   - Use lock-free SPSC ring buffers (`osb-audio::buffer::AudioRingBuffer`).
4. **Code Quality**:
   - Rust 1.87+ (stable).
   - Zero warnings: `cargo clippy --all-targets -- -D warnings`.
   - All code formatted with `cargo fmt`.
   - Structured error handling with `thiserror` (`OsbError`).
5. **License Compliance**: Apache 2.0. No non-commercial (NC) dependencies in the core runtime.

---

## 2. Continuity & Memory Bank (.kiro) Integration

The project maintains persistent context across agent sessions inside the `.kiro/` directory:

- **[`01-memory-bank.md`](.kiro/steering/01-memory-bank.md)**: Contains current development state, active blockers, test results, and next steps.
  - **Before starting work**: Read `01-memory-bank.md` to identify the current state and next tasks.
  - **After completing work**: Update `01-memory-bank.md` with recent changes, test counts, git commits, and updated next steps.
- **[`00-project-context.md`](.kiro/steering/00-project-context.md)**: High-level architectural overview and directory layout.
- **[`02-coding-standards.md`](.kiro/steering/02-coding-standards.md)**: Coding and real-time audio standards.

---

## 3. Milestone 1 (Audio Bridge) Focus

Your current development target is **Milestone 1**. The goal is a working low-latency audio passthrough from a physical microphone to a virtual microphone via PipeWire.

### Concrete M1 Components to Implement/Validate:

1. **Device Enumeration**:
   - File: [`crates/osb-pipewire/src/linux/context.rs`](crates/osb-pipewire/src/linux/context.rs)
   - Implement `pipewire-rs` registry listener to discover audio capture and playback nodes.
   - Parse node names, descriptions, and media classes (`Audio/Source`, `Audio/Sink`).
   - Connect to CLI command: `openspeechbridge devices`.

2. **PipeWire Capture Stream**:
   - File: [`crates/osb-pipewire/src/linux/capture.rs`](crates/osb-pipewire/src/linux/capture.rs)
   - Create real `pw::stream::StreamRef` capturing PCM audio from the selected source.
   - Push frames directly into [`AudioRingBuffer`](crates/osb-audio/src/buffer.rs) without heap allocation.

3. **PipeWire Playback Stream**:
   - File: [`crates/osb-pipewire/src/linux/playback.rs`](crates/osb-pipewire/src/linux/playback.rs)
   - Consume PCM frames from `AudioRingBuffer` and write to PipeWire playback stream.

4. **Virtual Microphone Creation**:
   - File: [`crates/osb-pipewire/src/linux/virtual_device.rs`](crates/osb-pipewire/src/linux/virtual_device.rs)
   - Create a virtual PipeWire source node (media class `Audio/Source/Virtual` or SPA node) named `"OpenSpeechBridge"` visible to third-party applications (Discord, Zoom, Meet, Teams).

5. **Audio Loopback Command & Latency Tracking**:
   - File: [`crates/osb-cli/src/commands/loopback.rs`](crates/osb-cli/src/commands/loopback.rs)
   - CLI command `openspeechbridge loopback` connecting input stream -> ring buffer -> output stream.
   - Track and report roundtrip latency and buffer occupancy using [`osb_core::metrics::LatencyTracker`](crates/osb-core/src/metrics.rs).

6. **Synthetic Testing**:
   - Audio tests must pass in headless CI and Docker environments without physical audio hardware.
   - Provide synthetic audio loops and simulated clock providers where needed.

---

## 4. Development Workflow & Commands

Since PipeWire development libraries are Linux-based, development on Windows is conducted through the running Docker container `osb-dev`:

```powershell
# Run tests inside Docker
docker compose exec dev cargo test

# Run Clippy checks (zero warnings policy)
docker compose exec dev cargo clippy --all-targets -- -D warnings

# Format code
docker compose exec dev cargo fmt

# Check CLI commands
docker compose exec dev cargo run --bin openspeechbridge -- doctor
docker compose exec dev cargo run --bin openspeechbridge -- devices
docker compose exec dev cargo run --bin openspeechbridge -- loopback --help
```

Or via PowerShell helper:
```powershell
.\scripts\dev.ps1 test
.\scripts\dev.ps1 lint
.\scripts\dev.ps1 fmt
```

---

## 5. Skills Available for Activation

- **`m1-audio-bridge`** (`.agents/skills/m1-audio-bridge/SKILL.md`): Detailed technical runbook for implementing PipeWire capture, playback, device discovery, and virtual mic.
- **`kiro-memory-sync`** (`.agents/skills/kiro-memory-sync/SKILL.md`): Procedure for updating and syncing `.kiro/` memory bank and status docs.
- **`docker-dev`** (`.agents/skills/docker-dev/SKILL.md`): Runbook for Docker-based compilation, testing, and debugging.
