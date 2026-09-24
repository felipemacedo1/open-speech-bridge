# Licensing

This document explains the licensing of OpenSpeechBridge and its dependencies.

## Project License

**OpenSpeechBridge is licensed under Apache License 2.0.**

This is a permissive open-source license that:
- ✅ Allows commercial use
- ✅ Allows modification
- ✅ Allows distribution
- ✅ Allows private use
- ✅ Provides patent protection
- ⚠️ Requires license notice preservation
- ⚠️ Requires stating changes

## Dependency Licenses

### Rust Dependencies

All Rust dependencies are compatible with Apache 2.0:

| Crate | License | Commercial OK |
|-------|---------|---------------|
| tokio | MIT | ✅ |
| serde | MIT/Apache-2.0 | ✅ |
| clap | MIT/Apache-2.0 | ✅ |
| tracing | MIT | ✅ |
| thiserror | MIT/Apache-2.0 | ✅ |
| anyhow | MIT/Apache-2.0 | ✅ |
| ringbuf | MIT/Apache-2.0 | ✅ |
| rubato | MIT | ✅ |
| pipewire-rs | MIT | ✅ |

### PipeWire

- **License**: MIT (libpipewire), LGPL-2.1 (some plugins)
- **Commercial Use**: ✅ Yes
- **Note**: We use libpipewire which is MIT licensed

## ML Model Licensing

> ⚠️ **IMPORTANT**: Model code and model weights often have DIFFERENT licenses!

### Speech Recognition

| Model | Code License | Weights License | Commercial OK |
|-------|--------------|-----------------|---------------|
| whisper.cpp | MIT | MIT* | ✅ |
| faster-whisper | MIT | MIT* | ✅ |

*Whisper weights are released under MIT by OpenAI.

### Translation

| Model | Code License | Weights License | Commercial OK |
|-------|--------------|-----------------|---------------|
| OPUS-MT | MIT | CC-BY-4.0 | ✅ (with attribution) |
| NLLB-200 | MIT | CC-BY-NC-4.0 | ❌ Non-commercial only |
| MADLAD-400 | Apache-2.0 | Apache-2.0 | ✅ |

### Text-to-Speech

| Model | Code License | Weights License | Commercial OK |
|-------|--------------|-----------------|---------------|
| Piper | MIT | MIT | ✅ |
| Coqui TTS | MPL-2.0 | Various | ⚠️ Check each model |

### Speech-to-Speech

| Model | Code License | Weights License | Commercial OK |
|-------|--------------|-----------------|---------------|
| SeamlessM4T | CC-BY-NC-4.0 | CC-BY-NC-4.0 | ❌ Non-commercial only |

## Default Engine Policy

For OpenSpeechBridge to be a genuine open-source project:

1. **Default engines MUST be commercially usable**
   - No NC (non-commercial) licenses for defaults
   - Users should be able to use the project commercially

2. **NC-licensed models may be supported as OPTIONAL engines**
   - Must be clearly documented
   - Not installed by default
   - User must explicitly choose to use them

3. **Attribution requirements must be documented**
   - CC-BY models require attribution
   - We maintain this document for compliance

## Recommended Default Stack

Based on licensing compatibility:

| Component | Recommended | License | Notes |
|-----------|-------------|---------|-------|
| STT | whisper.cpp | MIT | Full commercial use |
| Translation | OPUS-MT | CC-BY-4.0 | Attribution required |
| TTS | Piper | MIT | Full commercial use |

## Attribution Requirements

When using CC-BY licensed components:

### OPUS-MT (Helsinki-NLP)

Include in your application/documentation:
```
Translation powered by OPUS-MT models from the University of Helsinki.
https://github.com/Helsinki-NLP/Opus-MT
Licensed under CC-BY-4.0
```

## Checking New Dependencies

Before adding a dependency:

1. **Check the code license** in Cargo.toml or package.json
2. **Check the model weights license** (often in MODEL_CARD.md or separate LICENSE)
3. **Verify commercial use** is permitted
4. **Document attribution** requirements
5. **Update this file**

## Questions?

If you're unsure about licensing:
- Open a GitHub issue
- Check the original project's license files
- When in doubt, err on the side of caution
