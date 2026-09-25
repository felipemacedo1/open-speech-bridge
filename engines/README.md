# Engines

This directory contains speech processing engine implementations and SDKs.

## Structure

```
engines/
├── mock/           # Mock engine for testing
├── sdk-python/     # Python SDK for building engines
└── README.md       # This file
```

## What is an Engine?

An engine is a component that performs a specific speech processing task:

- **STT**: Speech-to-text (transcription)
- **MT**: Machine translation
- **TTS**: Text-to-speech (synthesis)
- **S2S**: Speech-to-speech (combined)
- **VAD**: Voice activity detection

## Engine Protocol

Engines communicate with the runtime via the [Engine Protocol](../docs/ENGINE_PROTOCOL.md).

Key points:
- JSON messages for control
- Audio data as float32 arrays
- Capability negotiation
- Health monitoring

## Available Engines

### Mock Engine

A testing engine that echoes or transforms audio without ML models.

```bash
# Build and test
cd mock
cargo build
cargo test
```

### Python SDK

Tools for building Python-based engines:

```python
from osb_engine_sdk import Engine, Capability

class MySTTEngine(Engine):
    capabilities = [Capability.STT, Capability.STREAMING_STT]
    
    def process_stt(self, audio, sample_rate, is_final):
        # Your STT logic here
        return {"text": "transcribed text"}
```

## Adding a New Engine

1. Read [ENGINE_PROTOCOL.md](../docs/ENGINE_PROTOCOL.md)
2. Check model license (see [LICENSING.md](../docs/LICENSING.md))
3. Implement the protocol
4. Add tests
5. Document in this README
6. Submit PR

## License Requirements

- Default engines: Must allow commercial use (MIT, Apache-2.0, CC-BY)
- Optional engines: Can be NC but must be clearly documented

See [LICENSING.md](../docs/LICENSING.md) for details.
