# Roadmap

This document outlines the development roadmap for OpenSpeechBridge.

> **Note**: Dates are not provided. Milestones are ordered by dependency and priority.

## Milestone 0: Foundation ✅

**Status: Complete**

- [x] Repository structure
- [x] Rust workspace with crates
- [x] Core types and errors
- [x] Audio buffer implementation
- [x] Engine protocol definition
- [x] CLI framework
- [x] Docker development environment
- [x] Documentation (README, AGENTS.md, CONTRIBUTING)
- [x] Architecture Decision Records
- [x] License compliance documentation

## Milestone 1: Audio Bridge 🔨

**Status: In Progress**

Goal: Working audio passthrough from microphone to virtual microphone.

- [ ] PipeWire device enumeration
- [ ] PipeWire capture stream
- [ ] PipeWire playback stream
- [ ] Virtual microphone creation
- [ ] Audio loopback test (`openspeechbridge loopback`)
- [ ] Latency measurement
- [ ] Buffer health metrics
- [ ] Manual validation on Ubuntu

**Success Criteria**: User can select "OpenSpeechBridge" as microphone in Discord and hear their own voice.

## Milestone 2: Engine Runtime

**Status: Planned**

Goal: Pluggable engine system with mock implementation.

- [ ] Engine process spawning
- [ ] IPC protocol implementation
- [ ] Engine health monitoring
- [ ] Mock engine (echo/transform)
- [ ] Engine capability negotiation
- [ ] Graceful engine restart
- [ ] Engine timeout handling

**Success Criteria**: Mock engine receives audio, transforms it, returns it.

## Milestone 3: First Local Translation

**Status: Planned**

Goal: Complete PT-BR → EN translation pipeline.

- [ ] Voice Activity Detection (VAD)
- [ ] whisper.cpp integration (STT)
- [ ] OPUS-MT integration (translation)
- [ ] Piper integration (TTS)
- [ ] Pipeline orchestration
- [ ] Language configuration
- [ ] Basic quality metrics

**Success Criteria**: User speaks Portuguese, virtual mic outputs English.

## Milestone 4: Full Duplex

**Status: Planned**

Goal: Simultaneous incoming and outgoing translation.

- [ ] Virtual sink for capturing app audio
- [ ] Incoming audio pipeline
- [ ] Echo cancellation consideration
- [ ] Barge-in handling
- [ ] Adaptive buffering
- [ ] Per-direction language config

**Success Criteria**: Two-way conversation translation in a voice call.

## Milestone 5: Streaming Optimization

**Status: Planned**

Goal: Low-latency incremental processing.

- [ ] Streaming STT (partial results)
- [ ] Streaming translation
- [ ] Streaming TTS
- [ ] Time-to-first-audio optimization
- [ ] Latency benchmarks
- [ ] Quality vs latency tradeoffs

**Success Criteria**: Translated audio begins while source is still speaking.

## Milestone 6: Voice Preservation

**Status: Future**

Goal: Preserve speaker characteristics in translation.

- [ ] Speaker embedding extraction
- [ ] Voice-conditioned TTS
- [ ] Prosody preservation
- [ ] Voice cloning consent flow
- [ ] Privacy controls

**Success Criteria**: Translated voice sounds like the original speaker.

## Milestone 7: Multi-Speaker

**Status: Future**

Goal: Handle conversations with multiple speakers.

- [ ] Speaker diarization
- [ ] Per-speaker tracking
- [ ] Per-speaker voice preservation
- [ ] Speaker identification
- [ ] Multi-channel support

**Success Criteria**: Group call with correct speaker attribution.

## Milestone 8: Platform Expansion

**Status: Future**

Goal: Support Windows and macOS.

- [ ] Windows WASAPI backend
- [ ] macOS CoreAudio backend
- [ ] Virtual audio device (platform-specific)
- [ ] Cross-platform testing
- [ ] Platform-specific installers

**Success Criteria**: Works on Ubuntu, Windows 10+, macOS 12+.

## Milestone 9: Network Features

**Status: Future**

Goal: Distributed and mobile support.

- [ ] Remote engine support
- [ ] WebRTC integration
- [ ] Mobile companion app
- [ ] Multi-device sync
- [ ] Cloud engine option (optional)

**Success Criteria**: Translation works across devices.

## Non-Goals

Things we explicitly won't do:

- Replace human interpreters for critical situations
- Provide a single "best" model
- Require cloud services for basic operation
- Support every language from day one
- Build mobile apps initially
- Compete with commercial products on features

## Contributing to the Roadmap

Want to help? Check:

1. [Issues](https://github.com/user/open-speech-bridge/issues) for current work
2. [docs/STATUS.md](STATUS.md) for project state
3. [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines

Propose new features via GitHub Discussions.
