# Architecture

This document describes the high-level architecture of OpenSpeechBridge.

## Overview

OpenSpeechBridge is a **local-first, real-time speech translation runtime**. It captures audio, processes it through pluggable engines, and outputs translated audio.

```
┌─────────────────────────────────────────────────────────────────────┐
│                        OpenSpeechBridge                              │
│                                                                      │
│  ┌────────────┐   ┌────────────┐   ┌────────────┐   ┌────────────┐  │
│  │  Physical  │   │   Ring     │   │  Engine    │   │  Virtual   │  │
│  │    Mic     │──▶│  Buffer    │──▶│  Pipeline  │──▶│    Mic     │  │
│  └────────────┘   └────────────┘   └────────────┘   └────────────┘  │
│        │                                                    │        │
│        │         PipeWire                                   │        │
│        └────────────────────────────────────────────────────┘        │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │                      Engine Supervisor                          │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐       │ │
│  │  │   STT    │  │    MT    │  │   TTS    │  │   S2S    │       │ │
│  │  │ Engine   │  │  Engine  │  │  Engine  │  │  Engine  │       │ │
│  │  └──────────┘  └──────────┘  └──────────┘  └──────────┘       │ │
│  └────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

## Core Principles

### 1. Real-Time Audio Safety

The audio callback path must **never block**. This means:

- No network requests
- No ML inference in the callback
- No unbounded queues
- No disk I/O
- No slow locks

Audio data flows through bounded ring buffers. If buffers overflow, we drop audio rather than block.

### 2. Engine Independence

The runtime doesn't depend on any specific ML model. Engines:

- Implement a versioned protocol
- Advertise their capabilities
- Can be swapped without code changes
- Run in isolated processes (for Python/ML engines)

### 3. Local-First

- Works without internet
- No mandatory cloud APIs
- Audio stays on-device by default

## Crate Structure

```
open-speech-bridge/
├── crates/
│   ├── osb-core/       # Types, errors, config (no I/O)
│   ├── osb-audio/      # Ring buffers, resampling, conversion
│   ├── osb-pipewire/   # PipeWire integration (Linux)
│   ├── osb-protocol/   # Engine protocol definitions
│   ├── osb-daemon/     # Runtime orchestration
│   └── osb-cli/        # Command-line interface
└── engines/
    ├── mock/           # Testing engine
    └── sdk-python/     # Python engine SDK
```

### osb-core

Pure data types with no I/O:
- `AudioFormat`, `SampleRate`, `ChannelLayout`
- `Error` types with severity levels
- `Config` structures
- `Metrics` definitions

### osb-audio

Audio processing utilities:
- `AudioRingBuffer` - Lock-free SPSC ring buffer
- `Resampler` - Sample rate conversion via rubato
- `SampleConverter` - Format conversion (i16/f32, mono/stereo)

### osb-pipewire

Linux audio backend:
- `PipeWireContext` - Initialize and manage PipeWire
- `CaptureStream` - Record from microphone
- `PlaybackStream` - Play to speakers
- `VirtualMicrophone` - Create virtual input device

### osb-protocol

Engine communication:
- `Capability` - What engines can do
- `EngineInfo` - Engine metadata
- `EngineRequest/Response` - Protocol messages

### osb-daemon

Runtime coordination:
- `Runtime` - Main orchestrator
- `AudioManager` - Device management
- `EngineSupervisor` - Engine lifecycle
- `Pipeline` - Processing flow

### osb-cli

User interface:
- `doctor` - System diagnostics
- `devices` - List audio devices
- `loopback` - Audio test
- `run` - Start daemon

## Data Flow

### Outgoing Translation (Mic → App)

```
Physical Mic
    │
    ▼
┌─────────────────┐
│ PipeWire Capture│ (48kHz stereo)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Ring Buffer    │ (bounded, lock-free)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Resample      │ (48kHz → 16kHz, stereo → mono)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│      VAD        │ (detect speech)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│      STT        │ (speech → text)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Translation    │ (text → text)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│      TTS        │ (text → speech)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│   Resample      │ (engine rate → 48kHz)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Ring Buffer    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Virtual Mic     │
└────────┬────────┘
         │
         ▼
    Application
```

### Incoming Translation (App → Headphones)

Similar flow but capturing from application audio output.

## Engine Protocol

Engines communicate via JSON messages:

```json
// Request
{
  "type": "process_stt",
  "request_id": 1,
  "audio": [...],
  "sample_rate": 16000,
  "is_final": true
}

// Response
{
  "type": "stt_result",
  "request_id": 1,
  "result": {
    "status": "success",
    "data": {
      "text": "Hello world",
      "confidence": 0.95
    },
    "processing_ms": 150
  }
}
```

See [ENGINE_PROTOCOL.md](ENGINE_PROTOCOL.md) for details.

## Buffer Management

All buffers are bounded to prevent memory issues:

```rust
// Ring buffer between capture and processing
let (producer, consumer) = AudioRingBuffer::new(
    4096,  // frames
    2      // channels
);

// Producer (capture callback) - non-blocking
let written = producer.push(samples);
if written < samples.len() {
    metrics.record_dropped(...);
}

// Consumer (processing thread) - can wait
let read = consumer.pop_or_silence(&mut buffer);
```

## Metrics

Key metrics tracked:

| Metric | Description |
|--------|-------------|
| `frames_received` | Total capture frames |
| `frames_emitted` | Total playback frames |
| `frames_dropped` | Frames lost to overflow |
| `underruns` | Buffer underrun events |
| `overruns` | Buffer overflow events |
| `capture_latency_us` | Capture processing time |
| `engine_latency_us` | Engine processing time |
| `output_latency_us` | Playback latency |

## Future Architecture

### Process Isolation

```
┌─────────────────────────────────────────┐
│         openspeechbridge-daemon          │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐  │
│  │PipeWire │  │ Engine  │  │ Metrics │  │
│  │  Audio  │  │Supervisor│  │         │  │
│  └─────────┘  └────┬────┘  └─────────┘  │
└────────────────────┼────────────────────┘
                     │ IPC
        ┌────────────┼────────────┐
        │            │            │
   ┌────▼────┐  ┌────▼────┐  ┌────▼────┐
   │  STT    │  │  Trans  │  │   TTS   │
   │ Process │  │ Process │  │ Process │
   │(Python) │  │(Python) │  │(Rust)   │
   └─────────┘  └─────────┘  └─────────┘
```

### Multi-Platform

```
osb-audio-backend/
├── pipewire/    # Linux
├── wasapi/      # Windows (future)
└── coreaudio/   # macOS (future)
```

## See Also

- [STATUS.md](STATUS.md) - Current implementation status
- [ROADMAP.md](ROADMAP.md) - Development plan
- [ENGINE_PROTOCOL.md](ENGINE_PROTOCOL.md) - Engine integration
- [adr/](adr/) - Architecture Decision Records
