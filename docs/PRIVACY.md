# Privacy

OpenSpeechBridge is designed with privacy as a core principle.

## Local-First Processing

By default, **all audio processing happens on your local machine**:

- ✅ Audio is captured locally
- ✅ Speech recognition runs locally
- ✅ Translation runs locally
- ✅ Speech synthesis runs locally
- ✅ No audio sent to cloud services

## Data Handling

### Audio Data

- **Not recorded by default**: Audio flows through the pipeline but is not saved
- **Not transmitted**: Audio stays on your device
- **Bounded buffers**: Old audio is overwritten, not accumulated

### Transcriptions

- **Ephemeral**: Text from STT is processed and discarded
- **Not logged by default**: Debug logging doesn't include transcriptions
- **No history**: No conversation history is maintained

### Voice Data

- **No voice profiles by default**: Speaker characteristics are not stored
- **Explicit consent required**: Voice cloning features require user action
- **Local storage only**: Any voice data stays on-device

## What We Don't Do

- ❌ Send audio to external servers (in default configuration)
- ❌ Store conversation recordings
- ❌ Track usage patterns
- ❌ Collect telemetry with user data
- ❌ Share any data with third parties

## Optional Cloud Features

Future versions may support optional cloud engines:

- **Explicit opt-in**: User must explicitly configure
- **Clearly documented**: What data goes where
- **Not required**: Local operation always available

## Configuration Options

```toml
# Example privacy-related configuration

[privacy]
# Never log audio content
log_audio = false

# Never log transcriptions
log_transcriptions = false

# Disable any telemetry
telemetry = false

# Don't persist any session data
persist_sessions = false
```

## Security Considerations

- **Process isolation**: ML engines run in separate processes
- **No network by default**: Daemon doesn't open network ports
- **Local IPC only**: Engine communication via local sockets

## For Developers

When contributing:

1. **Never log audio content** in release builds
2. **Never add telemetry** without explicit opt-in
3. **Never store conversations** without user request
4. **Document any data flow** that leaves the machine

## Compliance Notes

OpenSpeechBridge's local-first design helps with:

- **GDPR**: No personal data transmitted
- **HIPAA**: Audio stays on-device
- **Corporate policies**: No cloud dependency

However, users are responsible for their own compliance.

## Questions?

For privacy concerns, open a GitHub issue or discussion.
