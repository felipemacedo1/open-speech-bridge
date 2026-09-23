# Rules: Milestone 1 (Audio Bridge) Implementation

These rules govern the implementation of Milestone 1 in OpenSpeechBridge.

## Core Objective
Deliver a functioning low-latency audio passthrough using PipeWire on Linux:
Physical Microphone ➔ PipeWire Capture Stream ➔ AudioRingBuffer ➔ Virtual Microphone Source Node (PipeWire) ➔ Applications.

## Implementation Guidelines

### 1. Device Discovery (`crates/osb-pipewire/src/linux/context.rs`)
- Use the PipeWire core and registry (`pw::registry::RegistryRef`) to listen for global objects.
- Filter globals where `type_ == "PipeWire:Interface:Node"`.
- Read node properties:
  - `media.class`: check for `"Audio/Source"`, `"Audio/Source/Virtual"`, `"Audio/Sink"`.
  - `node.name` and `node.description`.
  - `device.id` and `object.serial`.
- Expose typed devices conforming to `osb_pipewire::device::AudioDevice`.
- Keep device lists thread-safe (e.g. using `Arc<RwLock<Vec<AudioDevice>>>`).

### 2. Real-Time Capture Stream (`crates/osb-pipewire/src/linux/capture.rs`)
- Build a `pw::stream::Stream` configured for capture:
  - `Direction::Input`.
  - Format: 48,000 Hz (or requested rate), interleaved or planar `f32` or `i16` (defaulting to 48kHz stereo or mono).
- Inside the process callback (`pw::stream::StreamListener::process`):
  - Dequeue the PipeWire buffer (`stream.dequeue_buffer()`).
  - Read raw audio frames from the SPA buffer.
  - Push samples directly into the lock-free [`AudioRingBuffer`](crates/osb-audio/src/buffer.rs).
  - Handle buffer overrun: drop oldest or record metric if buffer is full; never block or panic!
  - Requeue the buffer (`stream.queue_buffer()`).
- Invariant: Zero heap allocation in the process callback.

### 3. Real-Time Playback Stream (`crates/osb-pipewire/src/linux/playback.rs`)
- Build a `pw::stream::Stream` configured for playback:
  - `Direction::Output`.
- Inside the process callback:
  - Dequeue the buffer.
  - Pop frames from [`AudioRingBuffer`](crates/osb-audio/src/buffer.rs).
  - If buffer is empty (underrun), write digital silence (zeros) and increment the underrun counter in `osb_core::metrics::AudioMetrics`.
  - Requeue the buffer.

### 4. Virtual Microphone (`crates/osb-pipewire/src/linux/virtual_device.rs`)
- The virtual microphone must appear in the system as an `Audio/Source/Virtual` node so external software (Discord, Chrome, Meet, Zoom) lists it as an audio input device.
- Use PipeWire stream properties:
  - `media.class = "Audio/Source/Virtual"`
  - `node.name = "OpenSpeechBridge"`
  - `node.description = "OpenSpeechBridge Virtual Microphone"`
- Feed this virtual source with the output of the translation pipeline (or the loopback buffer in M1).

### 5. Loopback Passthrough CLI (`crates/osb-cli/src/commands/loopback.rs`)
- Connect capture stream directly to playback / virtual mic stream via `AudioRingBuffer`.
- Track metrics:
  - Buffer occupancy.
  - Latency (Time between capture dequeue and playback queue).
  - Underruns and overruns.
- Print live or periodic performance statistics (e.g., avg latency, min/max latency).

### 6. Synthetic & Headless Testing
- In headless CI and Docker environments without an active PipeWire daemon, fallback gracefully or use mocked loopback tests.
- Real PipeWire tests must be marked with `#[ignore]` or gated behind integration test flags so `cargo test` always passes cleanly.
