---
name: docker-dev
description: >-
  Use this skill when building, testing, linting with clippy, formatting, or running the OpenSpeechBridge Rust workspace inside the Docker development container osb-dev.
---

# Skill: Docker Development Workflow for OpenSpeechBridge

This skill provides the standard runbook for operating with the `osb-dev` Docker environment on Windows.

---

## 1. Container Overview

- **Container Name**: `osb-dev`
- **Base Image**: `openspeechbridge-dev:latest` (Ubuntu 22.04 + Rust 1.87+ + PipeWire 0.3 dev libraries + Clang)
- **Mount Point**: Host workspace mounted at `/workspace`
- **Cached Volumes**: `osb-cargo-registry`, `osb-cargo-git`, `osb-cargo-target`

---

## 2. Standard Development Commands

### Building and Testing

```powershell
# Run all workspace unit and doc tests
docker compose exec dev cargo test

# Run tests for a specific crate (e.g. osb-pipewire or osb-audio)
docker compose exec dev cargo test -p osb-pipewire
docker compose exec dev cargo test -p osb-audio

# Run Clippy checks with zero-warnings enforcement
docker compose exec dev cargo clippy --all-targets -- -D warnings

# Format all workspace code
docker compose exec dev cargo fmt

# Check formatting without modifying
docker compose exec dev cargo fmt --check
```

### Running CLI Commands

```powershell
# Doctor diagnostic check
docker compose exec dev cargo run --bin openspeechbridge -- doctor

# Audio devices enumeration
docker compose exec dev cargo run --bin openspeechbridge -- devices

# Check runtime status
docker compose exec dev cargo run --bin openspeechbridge -- status

# Loopback test
docker compose exec dev cargo run --bin openspeechbridge -- loopback --help
```

---

## 3. PowerShell Helper (`.\scripts\dev.ps1`)

The repository includes a PowerShell wrapper in [`scripts/dev.ps1`](scripts/dev.ps1):

```powershell
.\scripts\dev.ps1 test     # Runs cargo test --all-targets
.\scripts\dev.ps1 lint     # Runs cargo clippy --all-targets -- -D warnings
.\scripts\dev.ps1 fmt      # Runs cargo fmt
.\scripts\dev.ps1 shell    # Opens interactive bash inside container
.\scripts\dev.ps1 status   # Shows container status
```

---

## 4. Troubleshooting

- **Container not running**:
  ```powershell
  docker compose up -d dev
  ```
- **Proxy issues**:
  Ensure `.env` contains the correct `HTTP_PROXY` and `HTTPS_PROXY` values.
- **Rebuilding image after Dockerfile changes**:
  ```powershell
  docker compose build dev
  docker compose up -d dev
  ```
