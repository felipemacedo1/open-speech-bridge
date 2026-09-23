# Rules: Rust Development Workflow & Docker Execution

These rules govern the development, build, and validation workflow for OpenSpeechBridge.

## Docker-First Environment

The project compiles with Linux-specific dependencies (`pipewire-0.3`, `libspa-0.3`, `libclang`). On Windows hosts, all Cargo commands MUST be executed inside the running `osb-dev` container.

### Execution Commands:

```powershell
# Preferred via docker compose directly:
docker compose exec dev cargo build
docker compose exec dev cargo test
docker compose exec dev cargo clippy --all-targets -- -D warnings
docker compose exec dev cargo fmt --check

# Or via dev helper script:
.\scripts\dev.ps1 test
.\scripts\dev.ps1 lint
.\scripts\dev.ps1 fmt
```

## Quality Invariants

1. **Zero Warnings Policy**:
   `cargo clippy --all-targets -- -D warnings` must always succeed with zero warnings before any work is considered complete.

2. **Test Integrity**:
   `cargo test` must pass all un-ignored unit tests and doc-tests. New features must be accompanied by unit tests.

3. **Code Formatting**:
   All Rust source files must strictly follow `rustfmt` formatting. Run `docker compose exec dev cargo fmt` to ensure compliance.

4. **Structured Errors**:
   Use `thiserror` for library crates (`osb-core`, `osb-audio`, `osb-pipewire`, `osb-protocol`, `osb-daemon`) and `anyhow` only at top-level binary entry points (`osb-cli`).

5. **Audio Thread Rules**:
   - No `malloc` / `Vec::new()` / `String::new()` in the audio callback.
   - No blocking mutexes (`std::sync::Mutex`) on the audio thread; use atomic operations or lock-free SPSC channels.
