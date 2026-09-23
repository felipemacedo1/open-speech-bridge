# Engine Protocol

This document specifies the communication protocol between the OpenSpeechBridge runtime and speech processing engines.

## Overview

Engines are separate components that perform specific tasks:
- **STT**: Speech-to-text
- **MT**: Machine translation
- **TTS**: Text-to-speech
- **S2S**: Speech-to-speech (combined)
- **VAD**: Voice activity detection
- **Diarization**: Speaker identification

Engines can be:
- **Native**: Rust libraries loaded in-process
- **Python**: Separate Python processes
- **External**: Any process implementing the protocol

## Protocol Version

Current version: `0.1.0`

The protocol is versioned. Engines and runtime negotiate compatible versions.

## Transport

### Native Engines
- Direct function calls
- Shared memory for audio data

### Process Engines
- Unix domain sockets (Linux)
- JSON messages over stdio
- Binary audio data separately

## Message Format

All messages are JSON with a `type` field:

```json
{
  "type": "message_type",
  "...": "fields specific to message type"
}
```

## Lifecycle Messages

### Initialize

Runtime → Engine:
```json
{
  "type": "initialize",
  "config": {
    "model_path": "/path/to/model",
    "device": "cpu",
    "languages": ["pt-BR", "en-US"]
  }
}
```

Engine → Runtime:
```json
{
  "type": "initialized",
  "engine_id": "whisper-cpp-v1"
}
```

### Shutdown

Runtime → Engine:
```json
{
  "type": "shutdown"
}
```

Engine → Runtime:
```json
{
  "type": "shutdown_complete"
}
```

### Get Capabilities

Runtime → Engine:
```json
{
  "type": "get_capabilities"
}
```

Engine → Runtime:
```json
{
  "type": "capabilities",
  "info": {
    "id": "whisper-cpp",
    "name": "Whisper.cpp",
    "version": "1.5.0",
    "engine_type": "native",
    "capabilities": ["streaming_stt", "gpu"],
    "stt_languages": ["en", "pt", "es", "fr", "de"],
    "license": "MIT",
    "commercial_use": true
  }
}
```

### Health Check

Runtime → Engine:
```json
{
  "type": "ping",
  "timestamp_ms": 1699900000000
}
```

Engine → Runtime:
```json
{
  "type": "pong",
  "timestamp_ms": 1699900000000,
  "engine_timestamp_ms": 1699900000005
}
```

## Processing Messages

### Speech-to-Text

Runtime → Engine:
```json
{
  "type": "process_stt",
  "request_id": 1,
  "audio": [0.1, 0.2, -0.1, ...],
  "sample_rate": 16000,
  "is_final": false
}
```

Engine → Runtime (partial result):
```json
{
  "type": "stt_result",
  "request_id": 1,
  "result": {
    "status": "partial",
    "data": {
      "text": "Hello",
      "confidence": 0.85
    },
    "has_more": true
  }
}
```

Engine → Runtime (final result):
```json
{
  "type": "stt_result",
  "request_id": 1,
  "result": {
    "status": "success",
    "data": {
      "text": "Hello world",
      "language": "en",
      "confidence": 0.95,
      "words": [
        {"word": "Hello", "start_ms": 0, "end_ms": 300},
        {"word": "world", "start_ms": 320, "end_ms": 600}
      ]
    },
    "processing_ms": 150
  }
}
```

### Translation

Runtime → Engine:
```json
{
  "type": "process_translation",
  "request_id": 2,
  "text": "Olá mundo",
  "source_lang": "pt-BR",
  "target_lang": "en-US"
}
```

Engine → Runtime:
```json
{
  "type": "translation_result",
  "request_id": 2,
  "result": {
    "status": "success",
    "data": {
      "text": "Hello world",
      "detected_source": "pt"
    },
    "processing_ms": 50
  }
}
```

### Text-to-Speech

Runtime → Engine:
```json
{
  "type": "process_tts",
  "request_id": 3,
  "text": "Hello world",
  "language": "en-US",
  "voice_id": "en_US-amy-medium"
}
```

Engine → Runtime:
```json
{
  "type": "tts_result",
  "request_id": 3,
  "result": {
    "status": "success",
    "data": {
      "audio": [0.1, 0.2, ...],
      "sample_rate": 22050,
      "duration_ms": 1200
    },
    "processing_ms": 200
  }
}
```

### Speech-to-Speech

Runtime → Engine:
```json
{
  "type": "process_s2s",
  "request_id": 4,
  "audio": [...],
  "sample_rate": 16000,
  "source_lang": "pt-BR",
  "target_lang": "en-US",
  "is_final": true
}
```

Engine → Runtime:
```json
{
  "type": "s2s_result",
  "request_id": 4,
  "result": {
    "status": "success",
    "data": {
      "audio": [...],
      "sample_rate": 16000,
      "transcription": "Olá mundo",
      "translation": "Hello world"
    },
    "processing_ms": 500
  }
}
```

## Error Handling

### Error Response

```json
{
  "type": "error",
  "request_id": 1,
  "code": "MODEL_NOT_LOADED",
  "message": "Model file not found at /path/to/model"
}
```

### Error Codes

| Code | Description |
|------|-------------|
| `INIT_FAILED` | Engine initialization failed |
| `MODEL_NOT_LOADED` | Model file not found |
| `LANGUAGE_NOT_SUPPORTED` | Requested language not available |
| `PROCESSING_FAILED` | Processing error |
| `TIMEOUT` | Operation timed out |
| `OUT_OF_MEMORY` | Insufficient memory |
| `INTERNAL_ERROR` | Unexpected error |

### Cancellation

Runtime → Engine:
```json
{
  "type": "cancel",
  "request_id": 1
}
```

Engine → Runtime:
```json
{
  "type": "cancelled",
  "request_id": 1
}
```

## Capabilities

Engines advertise capabilities:

| Capability | Description |
|------------|-------------|
| `stt` | Batch speech-to-text |
| `streaming_stt` | Streaming speech-to-text |
| `vad` | Voice activity detection |
| `text_translation` | Text translation |
| `streaming_translation` | Streaming translation |
| `tts` | Batch text-to-speech |
| `streaming_tts` | Streaming TTS |
| `speech_to_speech` | Direct S2S |
| `streaming_speech_to_speech` | Streaming S2S |
| `diarization` | Speaker identification |
| `speaker_embedding` | Voice embedding extraction |
| `voice_conditioning` | Voice cloning |
| `gpu` | GPU acceleration |
| `cpu_only` | CPU-only operation |

## Implementing an Engine

### Python Example

```python
import json
import sys

class MySTTEngine:
    def __init__(self):
        self.model = None
    
    def handle_message(self, msg):
        if msg["type"] == "initialize":
            self.model = load_model(msg["config"]["model_path"])
            return {"type": "initialized", "engine_id": "my-stt"}
        
        elif msg["type"] == "get_capabilities":
            return {
                "type": "capabilities",
                "info": {
                    "id": "my-stt",
                    "name": "My STT Engine",
                    "version": "1.0.0",
                    "capabilities": ["stt", "streaming_stt"],
                    "stt_languages": ["en", "pt"]
                }
            }
        
        elif msg["type"] == "process_stt":
            text = self.model.transcribe(msg["audio"])
            return {
                "type": "stt_result",
                "request_id": msg["request_id"],
                "result": {
                    "status": "success",
                    "data": {"text": text},
                    "processing_ms": 100
                }
            }

def main():
    engine = MySTTEngine()
    for line in sys.stdin:
        msg = json.loads(line)
        response = engine.handle_message(msg)
        print(json.dumps(response))
        sys.stdout.flush()

if __name__ == "__main__":
    main()
```

### Rust Example

See `engines/mock/` for a complete example.

## Audio Format

- **Sample format**: float32 [-1.0, 1.0]
- **Channels**: Mono (1 channel)
- **Sample rate**: Specified per message (typically 16000 Hz for STT)

For large audio data, engines may use shared memory or separate binary channel.

## Versioning

The protocol uses semantic versioning. Breaking changes increment major version.

Engines should:
1. Report their protocol version
2. Reject incompatible runtime versions
3. Handle unknown message types gracefully
