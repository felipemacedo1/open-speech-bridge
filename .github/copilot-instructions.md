# GitHub Copilot Instructions for OpenSpeechBridge

This file provides context for GitHub Copilot and similar AI coding assistants.

## Project Overview

OpenSpeechBridge is a local-first, real-time speech translation runtime for Linux desktop. It captures audio, translates speech, and outputs to virtual audio devices.

## Key Files to Read

Before making changes, read:

1. **[AGENTS.md](../AGENTS.md)** - Primary source of truth for coding guidelines
2. **[docs/STATUS.md](../docs/STATUS.md)** - Current project state
3. **[docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md)** - System design

## Non-Negotiable Rules

1. **Real-time audio safety**: Never block audio callbacks
2. **Engine independence**: Runtime doesn't depend on specific ML models
3. **Local-first**: No mandatory cloud APIs
4. **License compliance**: No NC models as defaults
5. **Testing**: Tests required for meaningful changes

## Code Style

- Use `rustfmt` for formatting
- Follow `clippy` suggestions
- Use Conventional Commits
- Document public APIs

## Common Commands

```bash
cargo build          # Build
cargo test           # Test
cargo clippy         # Lint
cargo fmt            # Format
cargo run --bin openspeechbridge -- doctor  # Run CLI
```

## Architecture

```
crates/
├── osb-core/      # Types, no I/O
├── osb-audio/     # Ring buffers
├── osb-pipewire/  # Linux audio
├── osb-protocol/  # Engine protocol
├── osb-daemon/    # Runtime
└── osb-cli/       # CLI
```

For detailed guidelines, see [AGENTS.md](../AGENTS.md).
