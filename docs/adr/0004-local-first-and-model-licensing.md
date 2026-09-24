# ADR-0004: Local-First Operation and Model Licensing Policy

## Status

Accepted

## Context

OpenSpeechBridge aims to be a genuine open-source project. We need to define:

1. Whether cloud services are required
2. How to handle ML model licensing
3. What models can be bundled as defaults

Many powerful ML models have restrictive licenses (NC = non-commercial), while some open models have lower quality.

## Decision

### Local-First Operation

**The baseline application must work entirely locally without requiring:**
- Internet connection
- Cloud APIs
- Paid services

Cloud backends may exist as **optional plugins** but must never be mandatory.

### Model Licensing Policy

**Default engines must use commercially-permissive licenses:**

| License Type | As Default | As Optional |
|--------------|------------|-------------|
| MIT, Apache-2.0 | ✅ Yes | ✅ Yes |
| CC-BY-4.0 | ✅ Yes (with attribution) | ✅ Yes |
| CC-BY-NC-4.0 | ❌ No | ✅ Yes (documented) |
| Proprietary | ❌ No | ⚠️ Case by case |

**Key rule: Model code license ≠ Model weights license**

Always verify both separately.

## Alternatives Considered

### Cloud-First
- ✅ Best quality (GPT-4, etc.)
- ✅ Simpler to implement
- ❌ Privacy concerns
- ❌ Ongoing costs
- ❌ Internet dependency
- ❌ Not truly open source

### Include NC Models by Default
- ✅ Better quality
- ❌ Users can't use commercially
- ❌ Confusing licensing
- ❌ Not truly "open source" project

### Only MIT/Apache Models
- ✅ Clear licensing
- ✅ Full commercial use
- ⚠️ May limit quality options
- ⚠️ Excludes CC-BY models

### Chosen Approach
- ✅ Clear default path
- ✅ Commercial use OK
- ✅ Quality options available
- ✅ NC models as documented options

## Consequences

### Positive
- Users can use the project commercially
- No surprise licensing issues
- Privacy by default
- Works offline
- Truly open source

### Negative
- Default models may not be highest quality
- Users wanting best quality need NC models
- Need to maintain licensing documentation

### Mitigation
- Document model quality vs licensing tradeoffs
- Make NC model installation easy but explicit
- Improve open models over time
- Clear LICENSING.md documentation

## Recommended Default Stack

Based on this policy:

| Component | Model | License | Notes |
|-----------|-------|---------|-------|
| STT | whisper.cpp (base/small) | MIT | Good quality, full commercial |
| Translation | OPUS-MT | CC-BY-4.0 | Requires attribution |
| TTS | Piper | MIT | Good quality, full commercial |

## Optional (NC) Models

These provide better quality but restrict commercial use:

| Component | Model | License |
|-----------|-------|---------|
| Translation | NLLB-200 | CC-BY-NC-4.0 |
| S2S | SeamlessM4T | CC-BY-NC-4.0 |

## Documentation Requirements

1. **docs/LICENSING.md** must document all dependencies
2. Each engine must declare its license
3. CLI should warn when loading NC models
4. README must clarify licensing

## Related Decisions

- ADR-0001: Rust as primary language
- ADR-0003: Engine isolation (separate processes for different engines)
