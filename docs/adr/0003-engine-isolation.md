# ADR-0003: Engine Process Isolation

## Status

Accepted

## Context

ML engines (STT, translation, TTS) need to integrate with the runtime. Options:

1. **In-process**: Load engines as dynamic libraries
2. **Same process, separate thread**: Run engines in background threads
3. **Separate process**: Run engines as child processes

Concerns:
- ML inference can be slow and unpredictable
- ML libraries may crash
- Python engines need Python runtime
- Real-time audio must never block

## Decision

**Run ML engines in separate processes, communicating via IPC.**

- Native Rust engines may run in-process for performance
- Python engines always run as separate processes
- Engine crashes should not kill the daemon

## Alternatives Considered

### Everything In-Process
- ✅ Lowest latency
- ✅ Simplest architecture
- ❌ Engine crash kills daemon
- ❌ Can't use Python engines
- ❌ Memory issues affect whole system

### Thread Isolation
- ✅ Lower latency than IPC
- ✅ Easier resource sharing
- ❌ Engine crash still kills process
- ❌ Python GIL issues
- ❌ Memory corruption possible

### Process Isolation
- ✅ Engine crash is contained
- ✅ Can restart failed engines
- ✅ Python engines work naturally
- ✅ Memory isolation
- ⚠️ IPC latency overhead
- ⚠️ More complex architecture

## Consequences

### Positive
- Robust: engine crashes don't affect audio
- Flexible: any language for engines
- Scalable: engines can run on different machines
- Secure: engines have limited access

### Negative
- IPC adds latency (typically <1ms)
- More complex debugging
- Need IPC protocol
- Resource overhead per process

### Mitigation
- Use Unix sockets for low-latency IPC
- Optional in-process mode for trusted native engines
- Shared memory for large audio buffers
- Good logging and tracing

## Architecture

```
┌─────────────────────────────────┐
│      openspeechbridge-daemon     │
│  ┌─────────┐     ┌───────────┐  │
│  │ Audio   │     │  Engine   │  │
│  │ Runtime │     │ Supervisor│  │
│  └─────────┘     └─────┬─────┘  │
└────────────────────────┼────────┘
                         │ IPC (Unix Socket)
           ┌─────────────┼─────────────┐
           │             │             │
     ┌─────▼─────┐ ┌─────▼─────┐ ┌─────▼─────┐
     │    STT    │ │   Trans   │ │    TTS    │
     │  (Python) │ │  (Python) │ │   (Rust)  │
     └───────────┘ └───────────┘ └───────────┘
```

## Protocol

See [ENGINE_PROTOCOL.md](../ENGINE_PROTOCOL.md) for the IPC protocol specification.

## Related Decisions

- ADR-0001: Rust as primary language
- ADR-0004: Local-first and model licensing
