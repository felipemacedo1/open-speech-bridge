# OpenSpeechBridge - Agent Quick Reference Guide

> **Version**: 1.0.0 | **Last Updated**: 2024-09-24  
> **Purpose**: Single source of truth for AI agents working on this project.

---

## 🎯 Project Summary

**OpenSpeechBridge** is a local-first, real-time speech-to-speech translation runtime for desktop.

```
Physical Mic → PipeWire Capture → Ring Buffer → Translation Engine → Virtual Mic → Apps (Discord/Zoom/Meet)
```

| Attribute | Value |
|-----------|-------|
| Language | Rust 1.87+ (Edition 2021) |
| Audio Backend | PipeWire 0.3 (Linux) |
| License | Apache-2.0 |
| Dev Environment | Docker on Windows (Ubuntu 22.04 container) |

---

## 📍 Current Status (M1 - Audio Bridge)

| Component | Status | Notes |
|-----------|--------|-------|
| Ring Buffer (rtrb) | ✅ Done | Lock-free SPSC, `Send` safe |
| Capture Stream | ✅ Implemented | PR #11, awaiting Linux validation |
| Virtual Microphone | ✅ Implemented | PR #11, awaiting Linux validation |
| Virtual Sink | ✅ Implemented | PR #11, awaiting Linux validation |
| Device Enumeration | ⏳ Pending | Issue #6 |
| Linux Validation | ⏳ Pending | Requires real PipeWire daemon |

**Active PR**: [#11 - feat(pipewire): implement real PipeWire stream integration](https://github.com/felipemacedo1/open-speech-bridge/pull/11)

**RFC**: [#12 - Dual-path engine strategy (local free + Gemini premium)](https://github.com/felipemacedo1/open-speech-bridge/issues/12)

---

## 🚫 Non-Negotiable Rules

### 1. Real-Time Audio Safety
```rust
// ❌ NEVER in audio callback:
Vec::new()           // heap allocation
String::from()       // heap allocation
mutex.lock()         // blocking
file.read()          // blocking I/O
network_call()       // blocking I/O

// ✅ ALWAYS in audio callback:
stack_buffer: [f32; 512]  // stack allocation
ring_buffer.push()        // lock-free SPSC
atomic.load()             // non-blocking
```

### 2. Code Quality
```bash
# ALL must pass before any work is considered complete:
cargo clippy --all-targets -- -D warnings  # ZERO warnings
cargo test                                  # ALL tests pass
cargo fmt --check                           # Properly formatted
```

### 3. Completion Criteria
- ⚠️ **Build passing ≠ Feature complete**
- ⚠️ **Stub mode ≠ Real implementation**
- ✅ M1 is complete ONLY when validated on Linux with real PipeWire daemon

### 4. Local-First Principle
- No mandatory cloud APIs
- No telemetry transmitting user data
- Cloud features (like Gemini TTS) are OPTIONAL premium paths only

---

## 🛠️ Development Commands

All commands run inside Docker container `osb-dev`:

```powershell
# Build & Test
docker compose exec dev cargo build
docker compose exec dev cargo test
docker compose exec dev cargo clippy --all-targets -- -D warnings
docker compose exec dev cargo fmt

# CLI Commands
docker compose exec dev cargo run --bin openspeechbridge -- doctor
docker compose exec dev cargo run --bin openspeechbridge -- devices
docker compose exec dev cargo run --bin openspeechbridge -- loopback

# PowerShell Helper
.\scripts\dev.ps1 test
.\scripts\dev.ps1 lint
.\scripts\dev.ps1 fmt
.\scripts\dev.ps1 shell
```

---

## 📁 Key Files & Directories

```
voice/
├── AGENT_GUIDE.md          # 👈 YOU ARE HERE - Quick reference
├── AGENTS.md               # Detailed agent instructions
├── GEMINI.md               # AGY system directive
├── .kiro/steering/
│   ├── 00-project-context.md   # Architecture overview
│   ├── 01-memory-bank.md       # ⭐ SESSION STATE - Read first!
│   ├── 02-coding-standards.md  # Rust & real-time rules
│   └── 03-agent-behavior.md    # Agent behavior guidelines
├── .agents/
│   ├── rules/              # Implementation rules by topic
│   └── skills/             # Detailed runbooks for specific tasks
└── crates/
    ├── osb-audio/          # Ring buffers, resampling
    ├── osb-pipewire/       # PipeWire integration (the M1 focus)
    ├── osb-core/           # Types, errors, config, metrics
    ├── osb-protocol/       # Engine IPC protocol
    ├── osb-daemon/         # Runtime orchestration
    └── osb-cli/            # CLI commands
```

---

## 🔄 Session Workflow

### Starting a Session
1. **Read** `.kiro/steering/01-memory-bank.md` for current state
2. **Check** active PRs and issues on GitHub
3. **Continue** from where the last session left off

### Ending a Session
1. **Update** `.kiro/steering/01-memory-bank.md` with:
   - What was accomplished
   - Current test count and build status
   - Next steps
2. **Commit** documentation changes if significant

---

## 📊 Milestone Roadmap

| Milestone | Description | Status |
|-----------|-------------|--------|
| **M1** | Audio Bridge (PipeWire loopback) | 🔄 In Progress |
| M2 | Engine Runtime (IPC, mock engines) | ⏳ Pending |
| M3 | STT Integration (Whisper) | ⏳ Pending |
| M3.5 | Optional Gemini TTS (RFC #12) | 📋 RFC |
| M4 | Translation (OPUS-MT) | ⏳ Pending |
| M5 | TTS Integration (Piper) | ⏳ Pending |
| M6 | Voice Cloning | ⏳ Pending |
| M7 | GUI | ⏳ Pending |
| M8 | Distribution | ⏳ Pending |

---

## 🔗 Quick Links

- **Repository**: https://github.com/felipemacedo1/open-speech-bridge
- **PR #11 (M1 PipeWire)**: https://github.com/felipemacedo1/open-speech-bridge/pull/11
- **Issue #12 (Gemini RFC)**: https://github.com/felipemacedo1/open-speech-bridge/issues/12
- **GitHub Project M1**: https://github.com/users/felipemacedo1/projects/1

---

## 💡 Agent Tips

1. **Don't guess** - Read the code before making claims about it
2. **Don't assume completion** - Verify with actual tests on real environment
3. **Keep Memory Bank updated** - Future sessions depend on it
4. **Communicate in Portuguese** - Code/commits in English
5. **Ask if unclear** - Better to clarify than to break invariants

---

*This guide is version-controlled. Update the version number when making significant changes.*
