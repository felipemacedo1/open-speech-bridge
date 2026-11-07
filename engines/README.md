# Engines

This directory contains speech processing engine implementations and SDKs speaking the **OpenSpeechBridge Binary Engine Wire Protocol (`OSBP` v1)** over local Unix Domain Sockets (`AF_UNIX`, `SOCK_STREAM`).

## Structure

```text
engines/
├── mock/           # Rust mock engine library + osb-engine-mock UDS worker binary
├── sdk-python/     # Python OSBP v1 SDK + persistent faster-whisper & Kokoro-82M UDS workers
└── README.md       # This file
```

## Engine Capabilities

An engine worker is an isolated OS process advertising one or more capabilities during the `Hello`/`Welcome` handshake:

- **STT**: Speech-to-text transcription (`process_stt`)
- **SpeechTranslation**: Direct speech-to-target-text translation (`process_speech_translation`, e.g., Whisper `translate`)
- **MT**: Text-to-text machine translation (`process_translation`)
- **TTS**: Text-to-speech synthesis (`process_tts`, returning raw `f32le` PCM)
- **S2S**: Direct speech-to-speech translation (`process_s2s`)

## Engine Protocol (`OSBP` v1)

Engines communicate with `osb-daemon` via the [Engine Protocol](../docs/ENGINE_PROTOCOL.md) (see [ADR-0005](../docs/adr/0005-uds-framed-engine-ipc.md)):

- 16-byte little-endian binary frame prefix (`magic = "OSBP"`, `version = 1`, `flags`, `header_len`, `payload_len`)
- Compact UTF-8 JSON control headers
- Contiguous raw `f32le` binary PCM payloads (zero JSON float-array serialization)
- Multiplexed `request_id` correlation, `Ping`/`Pong` health checks, `Cancel` propagation, and automatic crash recovery

## Available Engines

### 1. Mock Engine (`engines/mock`)

Deterministic Rust testing engine that validates `OSBP` v1 framing, capability negotiation, and crash supervision without ML weights.

```bash
cargo test -p osb-engine-mock
cargo build -p osb-engine-mock
```

### 2. Python SDK & Local CPU Workers (`engines/sdk-python`)

Zero-dependency core `OSBP` v1 Python SDK (`osb_engine_sdk`) plus two persistent local CPU workers:

- **`faster_whisper_worker.py`**: CTranslate2 `faster-whisper` (`small`/`int8` default) supporting `SpeechTranslation` (`PT-BR -> EN` direct audio-to-English text) and `Stt`. Code & weights: **MIT**.
- **`kokoro_worker.py`**: `Kokoro-82M` CPU TTS synthesizing `24 kHz` `f32le` audio (`af_heart` default). Code: **Apache-2.0** (verify individual voice pack provenance before redistributing bundled weights).

Run SDK unit and UDS integration tests:

```bash
python3 -m unittest discover -s engines/sdk-python/tests -v
```

Custom Python worker example:

```python
from osb_engine_sdk import BaseEngineWorker, pcm_floats_to_bytes

class MyTtsWorker(BaseEngineWorker):
    def engine_info(self):
        return {
            "id": "my-tts",
            "name": "Custom Local TTS",
            "version": "0.1.0",
            "engine_type": "python",
            "capabilities": {
                "capabilities": ["tts", "cpu_only"],
                "language_pairs": [],
                "stt_languages": [],
                "tts_languages": ["en-US"],
            },
            "model": None,
            "license": "Apache-2.0",
            "weights_license": "Apache-2.0",
            "commercial_use": True,
        }

    def process_tts(self, request):
        pcm = pcm_floats_to_bytes([0.0] * 2400)
        return (
            {
                "type": "tts_result",
                "request_id": int(request["request_id"]),
                "result": {
                    "status": "success",
                    "processing_ms": 1,
                    "data": {"audio": [], "sample_rate": 24000, "duration_ms": 100},
                },
            },
            pcm,
        )
```

## License Requirements

- Default engines: Must allow commercial use (`MIT`, `Apache-2.0`, `CC-BY`)
- Optional engines: Non-commercial (`NC`) models may exist only as opt-in plugins and must declare `commercial_use = false` plus `weights_license`

See [LICENSING.md](../docs/LICENSING.md) for details.
