# ADR-0005: Unix Domain Socket Binary Framing and Supervised Worker IPC

## Status

Accepted

## Context

ADR-0003 established that ML inference engines (such as Python `faster-whisper` / `Kokoro` or C++ `whisper.cpp`) must run in separate OS processes so crashes, memory spikes, or Python GIL contention never stall the Rust PipeWire audio runtime.

Milestone 2 requires turning that architectural boundary into a concrete, bounded wire protocol and supervision contract:

1. **Transport**: Local process IPC without exposing TCP ports or requiring HTTP/gRPC overhead.
2. **Audio Payload Efficiency**: Early protocol types represented audio as JSON `Vec<f32>`, which causes large ASCII serialization/parsing overhead and extra copies in Python/NumPy and Rust.
3. **Bounded Memory & Backpressure**: A slow CPU inference worker must never cause unbounded queue growth in the daemon.
4. **Crash & Timeout Resilience**: Worker hangs or unexpected exits must be detected immediately, cancelled or restarted within a bounded budget, and keep the daemon alive.

## Decision

### 1. Persistent Supervised Workers over Unix Domain Sockets (UDS)

- By default, `osb-daemon` creates a user-private runtime socket directory (`0o700`) and binds a `UnixListener` (`0o600`) **before** spawning the persistent worker child process (`--socket <path>` and `OSB_ENGINE_SOCKET=<path>`).
- Racing `UnixListener::accept()` against `Child::wait()` under `startup_timeout` eliminates socket-creation race conditions and detects immediate startup crashes in milliseconds.
- `WorkerSpec::connect(<path>)` is also supported for attaching to externally managed local workers during development or shared-model setups.

### 2. Hybrid Binary Wire Frame (`OSBP` v1)

Every message on a UDS connection uses a fixed 16-byte little-endian binary prefix followed by a UTF-8 JSON control header and an optional raw IEEE-754 `f32` little-endian (`f32le`) PCM payload:

```text
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                         Magic: "OSBP"                         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|         Version (u16)         |          Flags (u16)          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       Header Length (u32)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Payload Length (u32)                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                 UTF-8 JSON Control Header ...                 |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|               Raw f32le PCM Audio Payload ...                 |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

- **Strict Pre-Allocation Validation**: Both `header_len` (default max 64 KiB) and `payload_len` (default max 4 MiB, multiple of 4 bytes) are validated against `FrameLimits` *before* allocating buffers.
- **Zero JSON Float Arrays on Wire**: Audio-bearing requests (`ProcessStt`, `ProcessSpeechTranslation`, `ProcessS2s`) and responses (`TtsResult`, `S2sResult`) strip their sample buffers out of the JSON header (`"audio":[]`) and write raw `f32le` bytes into the binary payload section (`FrameFlags::HAS_PCM_PAYLOAD`).
- **Zero Extra Schema Toolchain**: Python workers can parse frames in ~10 lines of standard `struct.unpack("<4sHHII", ...)` + `json.loads(...)` + zero-copy `numpy.frombuffer(payload, dtype="<f4")`, avoiding mandatory `protoc`/C-extension build dependencies while keeping bit-exact `f32` fidelity.

### 3. Versioned Handshake, Credit Window, and Crash Recovery

- Connection startup executes `Hello` → `Welcome` (verifying SemVer protocol compatibility, negotiating `max_payload_bytes`, and capturing `EngineInfo` capabilities plus separate `license` and `weights_license` fields) followed by `Initialize` → `Initialized`.
- `EngineRpcClient` enforces a bounded in-flight `Semaphore` (`max_in_flight`), multiplexed `request_id` correlation, out-of-band `Ping`/`Pong` health checks, and per-request deadlines that automatically emit `EngineRequest::Cancel { request_id }` on timeout.
- `EngineSupervisor` monitors worker exit status and EOF disconnects, transitioning failed workers to `EngineState::Error` and automatically restarting them up to `RestartPolicy::max_restarts` with configurable backoff.

## Alternatives Considered

### JSON-Lines over `stdin`/`stdout`
- ✅ Trivial for toy scripts
- ❌ Accidental `print()` / C++ library stdout writes corrupt the protocol stream
- ❌ Base64 or JSON float arrays add substantial CPU and memory overhead for PCM audio

### Protobuf / gRPC over Localhost or UDS
- ✅ Strong cross-language codegen
- ❌ Adds `protoc` build-time requirements and heavier Python `grpcio` dependencies
- ❌ Unnecessary HTTP/2 framing complexity for single-hop local UDS pipes

### Shared Memory (`memfd` / POSIX `shm_open`) in M2
- ✅ Zero kernel copy for very high-bandwidth streams
- ❌ Premature lifetime, synchronization, and crash-cleanup complexity when mono 16–24 kHz speech requires only ~64–96 KiB/s over UDS

## Consequences

### Positive
- Zero real-time audio impact: all UDS I/O and Tokio tasks remain strictly on the daemon orchestration plane
- Bit-exact, zero-JSON-float PCM framing compatible with Rust, Python (`numpy`), and C/C++ workers
- Deterministic backpressure, request cancellation, health monitoring, and automatic crash recovery validated in headless CI

### Negative
- Control headers remain UTF-8 JSON (~100–300 bytes per frame), slightly larger than packed binary varints (negligible at utterance/chunk rates of 10–50 Hz)

## Related Decisions

- ADR-0001: Rust as primary language
- ADR-0003: Engine process isolation
- ADR-0004: Local-first operation and model licensing policy
