# AGENTS.md - Coding Agent Instructions

This document is the primary source of truth for coding agents working on OpenSpeechBridge.

## Mission

OpenSpeechBridge is a **local-first, real-time speech-to-speech translation runtime** for desktop voice conversations. It captures speech, translates it, and outputs translated audio through virtual audio devices that applications can use.

**The runtime is the product. Models are replaceable engines.**

## Non-Negotiable Invariants

These rules must never be violated:

### 1. Local-First
- The baseline application must work without internet
- No mandatory cloud APIs or paid services
- Cloud backends may exist as *optional* plugins only

### 2. Real-Time Audio Safety
- **Audio callbacks must NEVER block on**:
  - Network requests
  - ML inference completion
  - Disk I/O
  - Slow locks
  - Unbounded queues
  - Python execution
- Use bounded buffers with backpressure
- Prefer dropping audio over blocking the audio thread

### 3. Engine Independence
- The runtime must not depend on any specific ML model
- Engines are replaceable through the protocol
- An engine crash should not kill the daemon

### 4. Open Source Licensing
- Project license: Apache-2.0
- No non-commercial (NC) models as default/required dependencies
- NC models may be supported as clearly documented *optional* engines
- Always verify: code license ≠ model weights license

### 5. Privacy
- Audio stays local by default
- No telemetry transmitting user data
- No persistent conversation recording by default
- Voice cloning requires explicit user action

### 6. Testing
- Tests required for meaningful behavior changes
- Benchmark performance-sensitive changes
- Never claim tests passed without running them

## Architecture Orientation

```
open-speech-bridge/
├── crates/
│   ├── osb-core/      # Types, errors, config (no I/O)
│   ├── osb-audio/     # Ring buffers, resampling
│   ├── osb-pipewire/  # PipeWire integration (Linux)
│   ├── osb-protocol/  # Engine protocol definitions
│   ├── osb-daemon/    # Runtime orchestration
│   └── osb-cli/       # Command-line interface
├── engines/
│   ├── mock/          # Testing engine
│   └── sdk-python/    # Python engine SDK
└── docs/
    └── adr/           # Architecture Decision Records
```

### Key Design Decisions

1. **Rust owns real-time audio** - Capture, buffers, playback, virtual devices
2. **Python for ML engines** - Isolated processes, communicate via protocol
3. **PipeWire/Linux first** - Primary platform, others later
4. **Bounded everywhere** - All buffers have fixed capacity
5. **Observable** - Metrics for latency, drops, buffer health

## Required Reading

Before making architectural changes, read:

1. `AGENTS.md` (this file)
2. `README.md`
3. `docs/ARCHITECTURE.md`
4. `docs/STATUS.md`
5. `docs/ROADMAP.md`
6. `docs/LICENSING.md`
7. `docs/adr/` (all ADRs)
8. `.kiro/steering/01-memory-bank.md` (Active session state & Memory Bank)
9. `GEMINI.md` / `.agents/rules/` (AGY System directives)

## Development Commands

```bash
# Build everything
cargo build

# Run all tests
cargo test

# Run specific crate tests
cargo test -p osb-core
cargo test -p osb-audio

# Check formatting
cargo fmt --check

# Run linter
cargo clippy -- -D warnings

# Run CLI
cargo run --bin openspeechbridge -- <command>

# Check system environment
cargo run --bin openspeechbridge -- doctor

# List devices
cargo run --bin openspeechbridge -- devices

# Format code
cargo fmt

# Generate docs
cargo doc --no-deps --open
```

## Change Discipline

When making changes:

1. **Read before writing** - Understand existing code first
2. **Update tests** - Add/modify tests for behavior changes
3. **Update docs** - Keep documentation in sync
4. **Update STATUS.md** - If project state changes
5. **Create ADRs** - For significant architectural decisions
6. **Check licenses** - Before adding dependencies
7. **Keep commits focused** - One logical change per commit
8. **Leave it buildable** - `cargo build` and `cargo test` must pass

## Commit Convention

Use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

### Types
- `feat` - New feature
- `fix` - Bug fix
- `docs` - Documentation only
- `refactor` - Code change that neither fixes nor adds
- `perf` - Performance improvement
- `test` - Adding/fixing tests
- `chore` - Build, CI, tooling changes

### Scopes
- `core` - osb-core crate
- `audio` - osb-audio crate
- `pipewire` - osb-pipewire crate
- `protocol` - osb-protocol crate
- `daemon` - osb-daemon crate
- `cli` - osb-cli crate
- `engine` - Engine-related changes
- `ci` - CI/CD changes
- `docs` - Documentation

### Examples
```
feat(audio): add lock-free ring buffer implementation
fix(pipewire): handle device disconnection gracefully
docs(architecture): document engine isolation strategy
perf(audio): reduce capture allocation rate
test(protocol): add engine handshake coverage
chore(ci): add cargo-deny workflow
```

## Handoff Protocol

Before ending substantial work, update `docs/STATUS.md` with:

1. **What works now** - Current functional capabilities
2. **Latest validated commands** - Commands known to work
3. **Known limitations** - What doesn't work or has issues
4. **Active questions** - Architectural decisions pending
5. **Next recommended tasks** - What to work on next

This prevents knowledge from existing only in conversation history.

## Adding Dependencies

Before adding a new dependency:

1. Check its license is compatible with Apache-2.0
2. Verify it's actively maintained
3. Consider if it's worth the compile time / binary size
4. Prefer well-known crates with good security track records
5. Pin to specific versions in workspace Cargo.toml

## Adding Engines

To add a new ML engine:

1. Read `docs/ENGINE_PROTOCOL.md`
2. Verify model code license
3. Verify model weights license (often different!)
4. Document in `docs/LICENSING.md`
5. If NC license, clearly mark as optional/non-default
6. Implement the engine protocol
7. Add to `engines/` directory
8. Update `docs/STATUS.md`

## Performance Work

For performance-sensitive changes:

1. Measure before changing
2. Add benchmarks if none exist
3. Document the measurement methodology
4. Record: hardware, OS, kernel, model, test method
5. Compare before/after
6. Update `docs/PERFORMANCE.md`

## Common Pitfalls

❌ **Don't:**
- Block the audio thread on anything slow
- Use unbounded channels or queues
- Assume model code and weights have the same license
- Add dependencies without checking licenses
- Claim features work without testing
- Make architectural changes without ADRs

✅ **Do:**
- Use bounded buffers everywhere
- Measure performance before optimizing
- Verify licenses of both code and models
- Write tests for new functionality
- Update documentation with changes
- Create ADRs for architectural decisions

## Questions?

- Check existing ADRs in `docs/adr/`
- Review `docs/ARCHITECTURE.md`
- Look at similar code in the codebase
- Ask if truly unclear

---

*This document should be the first thing any agent reads when working on this project.*
