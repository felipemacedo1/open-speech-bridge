# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial project structure with Rust workspace
- Core crate with audio types, errors, config, and metrics
- Audio crate with lock-free ring buffers and resampling
- Protocol crate with engine capability definitions
- PipeWire crate structure (Linux-only)
- Daemon crate with runtime orchestration
- CLI with doctor, devices, status, engines commands
- Docker development environment
- Comprehensive documentation (README, AGENTS.md, CONTRIBUTING.md)
- Architecture Decision Records (ADRs)
- GitHub Actions CI configuration
- Apache 2.0 license

### Infrastructure
- Rust workspace with 6 crates
- Docker and docker-compose for development
- Proxy support for corporate environments
- Development scripts for Windows PowerShell

## [0.1.0] - TBD

### Planned
- Working PipeWire audio capture
- Working PipeWire audio playback
- Virtual microphone creation
- Audio loopback test
- Basic metrics collection

---

[Unreleased]: https://github.com/user/open-speech-bridge/compare/main...HEAD
[0.1.0]: https://github.com/user/open-speech-bridge/releases/tag/v0.1.0
