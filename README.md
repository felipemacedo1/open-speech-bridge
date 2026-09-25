# OpenSpeechBridge

[![CI](https://github.com/felipemacedo1/open-speech-bridge/actions/workflows/ci.yml/badge.svg)](https://github.com/felipemacedo1/open-speech-bridge/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Linux-lightgrey.svg)](https://pipewire.org/)

<!-- Future badges (uncomment when published):
[![Crates.io](https://img.shields.io/crates/v/openspeechbridge.svg)](https://crates.io/crates/openspeechbridge)
[![Documentation](https://docs.rs/openspeechbridge/badge.svg)](https://docs.rs/openspeechbridge)
-->

**Local-first real-time speech-to-speech translation runtime for desktop voice conversations.**

OpenSpeechBridge captures audio from your microphone, translates it in real-time, and outputs translated speech through a virtual microphone that any application can use — Discord, Zoom, Google Meet, games, or any voice chat software.

## What is OpenSpeechBridge?

OpenSpeechBridge is an **engine-agnostic speech translation runtime**. It handles the hard parts of real-time audio:

- 🎤 Capturing audio from your microphone via PipeWire
- 🔄 Managing bounded buffers and backpressure
- 🎯 Routing translated audio to a virtual microphone
- 📊 Measuring latency and performance
- 🔌 Supporting pluggable speech/translation engines

**Models are replaceable engines. The runtime is the product.**

## Current Status

> ⚠️ **Early Development** - Core infrastructure is being built.

### What Works Today

- ✅ Project structure and build system
- ✅ Core audio types and buffer management
- ✅ Lock-free ring buffers for real-time audio
- ✅ Engine protocol and capability definitions
- ✅ CLI framework with `doctor`, `devices`, `status` commands
- ✅ PipeWire integration structure (Linux)

### What's In Progress

- 🔨 PipeWire device enumeration
- 🔨 Virtual microphone creation
- 🔨 Audio loopback test

### What's Planned

- 📋 Speech-to-text engine integration
- 📋 Translation engine integration  
- 📋 Text-to-speech engine integration
- 📋 Full translation pipeline
- 📋 Streaming for low latency

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     OpenSpeechBridge Runtime                     │
│                                                                  │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐  │
│  │ Physical │    │  Ring    │    │ Engine   │    │ Virtual  │  │
│  │   Mic    │───▶│ Buffer   │───▶│ Pipeline │───▶│   Mic    │  │
│  └──────────┘    └──────────┘    └──────────┘    └──────────┘  │
│       │                               │                │        │
│       │         ┌─────────────────────┘                │        │
│       │         ▼                                      │        │
│       │    ┌──────────┐                                │        │
│       │    │  STT     │ ─┐                             │        │
│       │    │ Engine   │  │   ┌──────────┐              │        │
│       │    └──────────┘  ├──▶│   TTS    │──────────────┘        │
│       │    ┌──────────┐  │   │  Engine  │                       │
│       │    │  MT      │ ─┘   └──────────┘                       │
│       │    │ Engine   │                                         │
│       │    └──────────┘                                         │
│       │                                                         │
│  PipeWire ◀─────────────────────────────────────────────────────│
└─────────────────────────────────────────────────────────────────┘
```

Engines are **isolated and replaceable**. You can swap whisper.cpp for faster-whisper, or use a direct speech-to-speech model. The runtime doesn't care — it just needs audio in, audio out.

## Quick Start

### Prerequisites

- Linux with PipeWire (Ubuntu 22.04+ recommended)
- Rust 1.75+
- PipeWire development libraries

```bash
# Ubuntu/Debian
sudo apt install pipewire pipewire-audio-client-libraries libpipewire-0.3-dev

# Fedora
sudo dnf install pipewire pipewire-devel
```

### Build

```bash
git clone https://github.com/felipemacedo1/open-speech-bridge.git
cd open-speech-bridge

# Build all crates
cargo build

# Run tests
cargo test

# Check system compatibility
cargo run --bin openspeechbridge -- doctor
```

### CLI Commands

```bash
# Check system environment
openspeechbridge doctor

# List audio devices
openspeechbridge devices

# Show status
openspeechbridge status

# List available engines
openspeechbridge engines

# Run audio loopback test
openspeechbridge loopback

# Start the daemon (future)
openspeechbridge run
```

## Project Goals

1. **Local-first**: Works without internet or paid APIs
2. **Real-time**: Translation starts while you're still speaking
3. **Privacy**: Audio stays on your machine by default
4. **Engine-agnostic**: Swap models without changing code
5. **Observable**: Measure latency, quality, resource usage

## Non-Goals

- Replacing professional human interpreters
- Providing a single "best" model
- Cloud-first architecture
- Mobile apps (initially)

## Roadmap

See [docs/ROADMAP.md](docs/ROADMAP.md) for detailed milestones.

| Milestone | Status | Description |
|-----------|--------|-------------|
| M0: Foundation | ✅ | Repository, CI, documentation |
| M1: Audio Bridge | 🔨 | PipeWire capture/playback/virtual devices |
| M2: Engine Runtime | 📋 | Engine protocol, mock engine, process isolation |
| M3: First Translation | 📋 | VAD + STT + MT + TTS pipeline |
| M4: Streaming | 📋 | Incremental processing for low latency |

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

### For AI/Coding Agents

This repository is designed for agent-assisted development. Before making changes:

1. Read [AGENTS.md](AGENTS.md) for project invariants and conventions
2. Check [docs/STATUS.md](docs/STATUS.md) for current state
3. Review [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for design context

Agent-generated PRs are welcome provided they:
- Pass all tests (`cargo test`)
- Follow the project's coding standards
- Include appropriate documentation updates
- Respect licensing requirements

## License

Apache License 2.0 - See [LICENSE](LICENSE) for details.

This project uses only open-source compatible dependencies. See [docs/LICENSING.md](docs/LICENSING.md) for detailed license information about dependencies and ML models.

## Acknowledgments

OpenSpeechBridge builds on the work of many open-source projects:

- [PipeWire](https://pipewire.org/) - Modern audio/video infrastructure
- [whisper.cpp](https://github.com/ggerganov/whisper.cpp) - Speech recognition (MIT)
- [Piper](https://github.com/rhasspy/piper) - Text-to-speech (MIT)
- [OPUS-MT](https://github.com/Helsinki-NLP/Opus-MT) - Translation models (CC-BY-4.0)

---

*OpenSpeechBridge is not affiliated with OpenAI, Meta, or any specific ML vendor. It's an independent open-source project focused on local speech translation.*
