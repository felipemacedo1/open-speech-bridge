# OpenSpeechBridge Development Container
# 
# This Dockerfile creates a development environment with all dependencies
# for building and testing OpenSpeechBridge on Linux.

FROM rust:1.82-bookworm AS base

# Build arguments for proxy (passed at build time)
ARG HTTP_PROXY
ARG HTTPS_PROXY
ARG NO_PROXY

# Set proxy environment variables
ENV HTTP_PROXY=${HTTP_PROXY}
ENV HTTPS_PROXY=${HTTPS_PROXY}
ENV NO_PROXY=${NO_PROXY}
ENV http_proxy=${HTTP_PROXY}
ENV https_proxy=${HTTPS_PROXY}
ENV no_proxy=${NO_PROXY}

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    # PipeWire and audio
    pipewire \
    pipewire-audio-client-libraries \
    libpipewire-0.3-dev \
    libspa-0.2-dev \
    # Build tools
    build-essential \
    pkg-config \
    cmake \
    # Debugging and utilities
    gdb \
    valgrind \
    strace \
    # Audio testing tools
    alsa-utils \
    pulseaudio-utils \
    # General utilities
    git \
    curl \
    vim \
    less \
    jq \
    && rm -rf /var/lib/apt/lists/*

# Install Rust components
RUN rustup component add rustfmt clippy rust-src rust-analyzer

# Install cargo tools
RUN cargo install cargo-watch cargo-expand cargo-deny cargo-audit

# Create non-root user for development
RUN useradd -m -s /bin/bash developer
USER developer
WORKDIR /home/developer

# Set up cargo directory
ENV CARGO_HOME=/home/developer/.cargo
ENV PATH="${CARGO_HOME}/bin:${PATH}"

# Working directory for the project
WORKDIR /workspace

# Default command
CMD ["/bin/bash"]

# ============================================================================
# Development stage with full tooling
# ============================================================================
FROM base AS development

USER root

# Install additional development tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    # Python for ML engine development
    python3 \
    python3-pip \
    python3-venv \
    # Additional debugging
    htop \
    iotop \
    # Documentation
    pandoc \
    && rm -rf /var/lib/apt/lists/*

USER developer

# Pre-create directories
RUN mkdir -p /home/developer/.cargo/registry \
    && mkdir -p /home/developer/.cargo/git

WORKDIR /workspace

# ============================================================================
# CI stage (minimal, for GitHub Actions)
# ============================================================================
FROM base AS ci

WORKDIR /workspace

# Copy project files
COPY --chown=developer:developer . .

# Build and test
RUN cargo build --all-targets
RUN cargo test --all-targets
RUN cargo clippy --all-targets -- -D warnings
RUN cargo fmt --check
