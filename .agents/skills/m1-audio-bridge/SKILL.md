---
name: m1-audio-bridge
description: >-
  Use this skill when developing, refactoring, or testing Milestone 1 (M1 - Audio Bridge) components for OpenSpeechBridge, including PipeWire device enumeration, capture stream, playback stream, virtual microphone creation, and loopback latency measurement.
---

# Skill: Milestone 1 (Audio Bridge) Development Runbook

This skill guides the agent through implementing and validating the PipeWire Audio Bridge for OpenSpeechBridge.

---

## 1. Overview of Milestone 1 Goals

The goal of M1 is establishing a working, low-latency audio passthrough:
`Physical Microphone ➔ PipeWire Capture Stream ➔ AudioRingBuffer ➔ PipeWire Virtual Microphone ➔ Desktop Apps`

### Success Criteria:
- `openspeechbridge devices` lists actual PipeWire capture and playback devices.
- `openspeechbridge loopback` runs continuous audio passthrough and displays real-time latency stats.
- Applications (Discord, Meet, Zoom) recognize the virtual microphone named `"OpenSpeechBridge"`.

---

## 2. Component Implementation Procedures

### Step 1: Device Enumeration
**File:** [`crates/osb-pipewire/src/linux/context.rs`](crates/osb-pipewire/src/linux/context.rs)

1. Connect to PipeWire context and core:
   ```rust
   let mainloop = pw::main_loop::MainLoop::new(None)?;
   let context = pw::context::Context::new(&mainloop)?;
   let core = context.connect(None)?;
   let registry = core.get_registry()?;
   ```
2. Attach a listener for global objects:
   ```rust
   let _listener = registry.add_listener_local()
       .global(|global| {
           if global.type_ == "PipeWire:Interface:Node" {
               if let Some(props) = global.props {
                   let media_class = props.get("media.class");
                   let node_name = props.get("node.name");
                   let node_desc = props.get("node.description");
                   // Parse and construct AudioDevice (Input or Output)
               }
           }
       })
       .register();
   ```
3. Run one mainloop iteration with `mainloop.run()` or roundtrip sync to collect globals.
4. Expose the list in `PipeWireContext::list_devices()`.
5. Connect to `crates/osb-cli/src/commands/devices.rs` for user CLI output.

---

### Step 2: Microphone Capture Stream
**File:** [`crates/osb-pipewire/src/linux/capture.rs`](crates/osb-pipewire/src/linux/capture.rs)

1. Create a stream with `pw::stream::Stream::new`:
   - Set properties: `media.type = "Audio"`, `media.category = "Capture"`, `media.role = "Communication"`.
2. Connect stream to selected device ID (or default source):
   - Format: `AudioFormat::F32LE` (or requested format), sample rate 48,000 Hz, stereo or mono.
3. Process Callback implementation:
   ```rust
   stream.add_listener_local()
       .process(|stream| {
           if let Some(mut buffer) = stream.dequeue_buffer() {
               let datas = buffer.datas_mut();
               // Extract PCM f32 samples from datas[0]
               // Push directly to bounded AudioRingBuffer producer
               // If buffer full: record drop in metrics, do not panic
               stream.queue_buffer(buffer);
           }
       })
       .register();
   ```

---

### Step 3: Playback Stream
**File:** [`crates/osb-pipewire/src/linux/playback.rs`](crates/osb-pipewire/src/linux/playback.rs)

1. Configure stream with `Direction::Output` and `media.category = "Playback"`.
2. In the process callback:
   - Dequeue buffer.
   - Pop samples from `AudioRingBuffer` consumer.
   - If buffer underruns, fill with silence (`0.0f32`) and increment underrun metrics.
   - Requeue buffer.

---

### Step 4: Virtual Microphone Node
**File:** [`crates/osb-pipewire/src/linux/virtual_device.rs`](crates/osb-pipewire/src/linux/virtual_device.rs)

1. To create a virtual source node in PipeWire:
   - Configure a `Stream` with `Direction::Output` but setting `media.class = "Audio/Source/Virtual"`.
   - Node name: `"OpenSpeechBridge"`.
   - Node description: `"OpenSpeechBridge Virtual Microphone"`.
2. Applications reading from microphone devices will see this node in their audio input selection list.
3. Audio frames passed to this stream appear as microphone input to third-party applications.

---

### Step 5: Loopback CLI Command & Latency Tracking
**File:** [`crates/osb-cli/src/commands/loopback.rs`](crates/osb-cli/src/commands/loopback.rs)

1. Setup:
   - Initialize `AudioRingBuffer` with bounded capacity (e.g., 4096 frames).
   - Initialize `LatencyTracker` from `osb_core::metrics`.
   - Start capture stream pushing into the ring buffer.
   - Start playback stream (or virtual mic stream) popping from the ring buffer.
2. Monitor loop:
   - Print periodic stats: buffer occupancy, underruns, latency (min, max, avg, p95).
   - Handle `Ctrl+C` for graceful shutdown.

---

## 3. Validation & Testing Commands

Always validate in the Docker dev container:

```powershell
# 1. Compile all crates
docker compose exec dev cargo build

# 2. Run all unit and doc tests
docker compose exec dev cargo test

# 3. Check for any clippy warnings
docker compose exec dev cargo clippy --all-targets -- -D warnings

# 4. Verify formatting
docker compose exec dev cargo fmt --check

# 5. Check CLI doctor and devices commands
docker compose exec dev cargo run --bin openspeechbridge -- doctor
docker compose exec dev cargo run --bin openspeechbridge -- devices
docker compose exec dev cargo run --bin openspeechbridge -- loopback --help
```
