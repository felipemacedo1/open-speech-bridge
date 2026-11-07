# Open Speech Bridge — Architecture Research

> Technical discovery, build-vs-reuse analysis and architecture proposal  
> Snapshot: 2026-09-27  
> Scope: Ubuntu Linux, local/offline operation, ordinary CPU, no mandatory paid API  
> Status: proposal for review; this document does not supersede accepted ADRs

## Executive Summary

Open Speech Bridge (OSB) should remain a **Rust audio runtime and orchestration
layer**, not become another speech-model implementation. The repository already
contains the right foundations for that role: bounded lock-free audio buffers,
resampling, metrics, Linux/PipeWire capture, and a physically validated
PipeWire virtual source. It does not yet contain a functioning inference
pipeline, production engine IPC, native PipeWire registry discovery, or a real
daemon that moves audio through speech engines.

The recommended baseline is **Architecture A, a modular pipeline**, with an
important simplification for the first Portuguese-to-English vertical slice:
use Whisper's native `translate` task to perform speech-to-English-text in one
engine, then synthesize English with Kokoro. This removes a separate machine
translation stage from the MVP without coupling OSB to an end-to-end model:

```text
physical microphone
  -> PipeWire capture
  -> bounded Rust buffers + resampling + endpointing
  -> Silero VAD
  -> Whisper speech translation (PT speech -> EN text)
  -> Kokoro TTS (EN text -> EN speech)
  -> bounded output/jitter buffer
  -> PipeWire virtual microphone
  -> Meet / Discord / Teams / Zoom
```

The concrete engines must be selected by a reproducible benchmark, not by a
paper estimate. The first candidates should be `whisper.cpp` with quantized
`base`/`small` models and `faster-whisper` with CPU INT8, plus Kokoro through an
ONNX runtime such as sherpa-onnx. All have permissive code/model licensing for
this use according to their official repositories or model cards. Silero VAD
is the preferred endpointing candidate; WebRTC VAD is a small fallback.

**Meta Seamless is technically important but not a default OSB dependency.**
SeamlessM4T v2 and SeamlessStreaming can perform speech-to-speech translation,
and therefore can bypass distinct ASR, MT and TTS engines at the logical level.
They do not replace audio capture, device routing, buffering, endpoint/session
control, backpressure, observability, or necessarily front-end VAD. More
importantly, their official weights are non-commercial, large (billions of
parameters), and have no primary-source evidence of laptop-CPU real-time
operation. SeamlessExpressive preserves vocal style, prosody, pauses and
speaking rate; it is **not a general voice-cloning guarantee** and its published
expressive target-language set does not include Portuguese. Seamless should be
an optional research provider behind a `SpeechToSpeechEngine` interface.

For the MVP, a realistic CPU-only product target is **1.5–3 seconds after an
utterance boundary**, with 500 ms–1.5 s as a stretch target for first stable
text/first audio on tuned hardware. Sub-500 ms high-quality end-to-end speech
translation is not a credible general CPU-only promise because the system must
hear enough future speech to make a stable translation and then synthesize it.
These are planning bands, not measured OSB results.

## Research Method and Evidence Boundaries

This review combined:

- source inspection across all workspace crates, engine scaffolds, tests,
  workflows, scripts, ADRs, project documentation and TODO markers;
- local Git history, current branch, open changes and remote GitHub PR/issue/CI
  state through `gh api`;
- headless Docker validation of formatting, tests and Clippy;
- primary sources: official repositories, papers, documentation and model
  cards for capabilities and licensing.

The current checkout is `fix/m1-pipewire-buffer-sync` at `632eaad4d5c0` and has
pre-existing uncommitted M1 changes. The report did not modify those changes.
The repository's physical PipeWire results below are recorded evidence from
`docs/STATUS.md`; this research run did **not** repeat the physical test. The
headless checks performed for this review cannot validate real audio devices.

Confidence labels used implicitly throughout:

- **verified in this checkout**: read directly from code or tests;
- **recorded repository evidence**: documented with concrete prior commands or
  counters but not reproduced in this review;
- **upstream claim**: supported by the linked primary source;
- **estimate / hypothesis**: must be benchmarked before becoming a product
  commitment.

## Current Repository State

### Repository and delivery state

- Public repository, Apache-2.0, default branch `main`.
- Working branch: `fix/m1-pipewire-buffer-sync`; open
  [PR #14](https://github.com/felipemacedo1/open-speech-bridge/pull/14)
  contains the M1 buffer synchronization and physical validation work.
- [PR #11](https://github.com/felipemacedo1/open-speech-bridge/pull/11)
  (CI/Sonar remediation) and
  [PR #15](https://github.com/felipemacedo1/open-speech-bridge/pull/15)
  (experimental-status documentation) are merged. The most recent inspected
  `main` commit was `348b73a` (`docs: clarify experimental project status`).
- Latest inspected `main`
  [CI run 36238131386](https://github.com/felipemacedo1/open-speech-bridge/actions/runs/36238131386)
  and [Sonar run 36238131374](https://github.com/felipemacedo1/open-speech-bridge/actions/runs/36238131374)
  passed. PR #14's inspected
  [CI run 36146724428](https://github.com/felipemacedo1/open-speech-bridge/actions/runs/36146724428)
  and [Sonar run 36146724438](https://github.com/felipemacedo1/open-speech-bridge/actions/runs/36146724438)
  also passed. These are workflow results, not proof of physical audio or model
  quality. There are no GitHub releases.
- Relevant open work includes the
  [dual-path architecture RFC #12](https://github.com/felipemacedo1/open-speech-bridge/issues/12),
  [native PipeWire registry enumeration #6](https://github.com/felipemacedo1/open-speech-bridge/issues/6),
  [integration tests #10](https://github.com/felipemacedo1/open-speech-bridge/issues/10),
  and [M1 epic #5](https://github.com/felipemacedo1/open-speech-bridge/issues/5).
  Other open PRs observed were dependency/action updates (#1, #2, #3 and #13),
  not alternate product architectures.
- The GitHub API exposed no active repository ruleset and reported `main` as
  unprotected at this snapshot. This is a delivery/governance risk, not an
  audio-architecture constraint.
- Documentation contains historical sections that predate the physical M1
  validation. README/older status statements must not override newer code and
  evidence.

### Crate-level assessment

| Area | State | Evidence and consequence |
|---|---|---|
| `osb-core` | solid foundation | Audio/config/error/metric types exist without model coupling. Keep it small and I/O-free. |
| `osb-audio` | working | Bounded `rtrb` SPSC queues, conversion, metrics and Rubato resampling have meaningful tests. Blocking helper APIs must never be called from real-time callbacks. |
| `osb-pipewire` capture | working on Linux branch | Actual PipeWire stream callbacks write to bounded rings. Device selection fixes are present in the working tree. |
| PipeWire virtual microphone | working, branch-level evidence | The virtual source publishes F32 audio; repository evidence records end-to-end consumption by `pw-record`. Not yet a released capability. |
| PipeWire enumeration | partial / transitional | `context.rs` shells out to `pw-cli` and parses human-oriented output. Native registry listeners are still needed. |
| Playback / virtual sink | stub / absent | `PlaybackStream` is an in-memory lifecycle/ring abstraction, not a real PipeWire playback stream. Virtual sink is incomplete. |
| `osb-protocol` | useful vocabulary, not production transport | Message types cover STT/MT/TTS/S2S and partials, but PCM is represented as JSON `Vec<f32>`; framing, negotiation, cancellation, flow control and binary transport are absent. |
| `osb-daemon` | skeleton | Runtime, Pipeline and Supervisor model lifecycle, but do not spawn workers, implement IPC or execute the speech graph. |
| mock engine | test scaffold | In-process Rust handler returns deterministic data/tone. It is not an isolated engine process and S2S is not implemented. |
| Python SDK | scaffold | Direction is consistent with ADRs, but it is not yet a complete runnable provider contract. |
| CLI | diagnostic scaffold | `doctor`, device and loopback work are useful; `status`, engine registry and `run` include hard-coded or lifecycle-only behavior. |

### Validation snapshot

The following were run inside the project's Docker development environment on
2026-09-27:

```text
cargo fmt --check                                      PASS
cargo test --workspace                                 PASS
cargo clippy --workspace --all-targets -- -D warnings  PASS
```

The test run contained **123 passing tests** (119 unit tests plus 4 doctests)
and **15 ignored tests** (11 unit tests plus 4 doctests). Ignored PipeWire tests
and a container without the host audio session are not physical-audio proof.

Newer repository evidence in `docs/STATUS.md` records a physical PipeWire
1.0.5/WirePlumber run with 1,407 capture callbacks, 1,407 virtual callbacks,
1,367,552 virtual frames consumed, non-zero signal through internal A–D probes,
and a simultaneous `pw-record` consumer. The very low downstream WAV amplitude
is documented alongside source volume 0.03. That proves a narrow branch-level
audio bridge; it does not prove model inference or conferencing-app support.

## What Already Works

1. Workspace and crate boundaries encode the intended ownership correctly:
   Rust owns audio and orchestration; model runtimes are replaceable.
2. Audio format, frames, bounded buffering, overflow accounting, resampling and
   basic timing metrics exist with tests.
3. PipeWire capture and virtual-source code is real, not only an interface.
4. The branch has evidence of physical microphone capture reaching an external
   PipeWire consumer through the virtual microphone.
5. CI, formatting, workspace tests and strict Clippy are operational.
6. ADRs already establish local-first operation, process isolation, bounded
   queues, engine independence, Apache-2.0 compatibility and Linux/PipeWire
   priority.

## What Works Partially or Experimentally

- PipeWire device discovery depends on the `pw-cli` executable and text parsing.
- Device policy/pinning and virtual-source publication fixes live on an open
  branch with uncommitted follow-up changes in this checkout.
- Loopback diagnostics are an M1 validation path, not the product pipeline.
- Protocol structures express desired operations but are not a streaming wire
  protocol.
- The mock engine is good for deterministic unit tests but does not exercise
  process supervision or IPC failure modes.
- Daemon and CLI lifecycle states exist, but do not run inference.
- Architecture B is discussed in RFC/issue form; it has not been integrated or
  benchmarked.

## What Does Not Exist Yet

- A complete `physical speech -> translated speech -> virtual mic` execution.
- A production engine worker executable, handshake and protocol transport.
- VAD/endpointing, ASR/speech translation, MT, TTS or S2S providers.
- Stable partial-result/revision semantics and utterance commit rules.
- Deadline, cancellation, restart, health, capability and model-license policy.
- A real output scheduler/jitter buffer connected to virtual-mic timing.
- Native PipeWire registry enumeration and robust hot-plug/reconnection.
- Reproducible speech quality/latency benchmark corpus and harness.
- End-user packaging, permission UX and conferencing-application compatibility
  matrix.

## Keep, Refactor, Retire

### Keep

- Current crate boundaries and the Rust/PipeWire-first direction.
- Bounded SPSC buffers, drop/overflow metrics and callback safety rules.
- Rubato resampling rather than writing a resampler.
- Capability-oriented protocol concepts including partial results and S2S.
- Process isolation from the ADRs and a deterministic mock engine.
- The narrow physical PipeWire validation script and its evidence discipline.

### Refactor or complete

- Split engine **control plane** from binary audio **data plane**.
- Turn daemon lifecycle types into a real supervised worker/session graph.
- Replace shell-based device discovery with PipeWire registry listeners.
- Make timestamps, discontinuities, utterance IDs and cancellation first-class.
- Replace hard-coded CLI states with daemon state queried over a local control
  socket.
- Make playback naming/documentation explicit until a real PipeWire playback
  implementation exists.
- Consolidate historical status/README claims so the newest evidence is
  authoritative.

### Retire from the production path

- JSON arrays of floating-point PCM samples across process boundaries.
- `pw-cli` parsing as the long-term discovery mechanism.
- In-process Python/PyTorch calls from the audio runtime.
- Any claim that the mock engine or lifecycle-only `run` command is a working
  translation pipeline.
- The blanket assumption that “Piper is MIT”: the maintained Piper repository
  is GPL-3.0 and voice model licenses vary.

## What Should Be Reused

| Need | Preferred reuse | Why |
|---|---|---|
| VAD/endpointing | Silero VAD; WebRTC VAD fallback | Small, permissive, mature; do not train or implement VAD. |
| PT speech -> EN text | Whisper via `whisper.cpp` or `faster-whisper` | Direct translation task removes one MVP stage; permissive weights/code. |
| General text MT | exact-language Marian/OPUS or Bergamot model after per-model license verification | CPU-friendly and modular; avoid NLLB as mandatory because its weights are NC. |
| English TTS | Kokoro-82M through ONNX/sherpa-onnx | Small relative to modern neural TTS, permissive official model card, CPU-oriented runtime. |
| Optional denoise | RNNoise; SpeexDSP fallback | Mature DSP components; keep disabled/optional until measured. |
| Resampling | existing Rubato integration | Already integrated and tested. |
| Audio graph | PipeWire + WirePlumber policy | Native Linux desktop routing and virtual devices. |
| S2S research | SeamlessM4T/Streaming provider | Strong capability baseline, but NC and hardware-heavy. |

## What Should NOT Be Built

OSB should not train or reimplement speech recognition, translation, neural
speech synthesis, voice conversion, VAD, denoising, codecs, resampling, or a
general multimedia graph. It should also avoid inventing a remote-RPC framework
before the local worker protocol is proven. The differentiating work is audio
correctness, orchestration, bounded streaming, provider interoperability,
latency management, reliability, privacy and desktop UX.

## Existing Technologies

### Speech recognition and speech translation

- **Whisper** supports multilingual transcription and direct translation from
  non-English speech to English. Its [official repository](https://github.com/openai/whisper)
  licenses both code and model weights under MIT. The official model card says
  it is not real-time out of the box. `turbo` is not the translation choice;
  the README states it is not trained for translation tasks.
- **whisper.cpp** is an MIT C/C++ implementation with CPU quantization,
  streaming examples and a C API. Official model listings put unquantized
  models at roughly 75 MB (`tiny`), 142 MB (`base`), 466 MB (`small`), 1.5 GB
  (`medium`) and 2.9 GB (`large`); quantized variants reduce this. It is the
  cleanest native-process candidate for Rust integration.
- **faster-whisper** is MIT and uses MIT-licensed **CTranslate2**. Its official
  repository documents CPU INT8 operation and reports up to 4x the speed of
  the reference implementation under its benchmark conditions. That claim is
  upstream evidence, not an OSB measurement.
- **Omnilingual ASR** is notable for more than 1,600 languages and Apache-2.0
  code/models, with official checkpoints from 300M to 7B parameters. It is an
  excellent future coverage provider, not the smallest proven PT/EN MVP.
- **MMS** provides broad ASR/TTS/language coverage, but the inspected Meta model
  cards use CC-BY-NC 4.0. Keep it optional/research-only.

### Text translation

- **Whisper direct translation** is the smallest PT-speech-to-EN-text MVP. It
  does not provide arbitrary target languages.
- **Marian / OPUS-MT** and **Bergamot** are CPU-capable, quantization-friendly
  paths. Code licensing is permissive/MPL respectively, but every downloaded
  model card/artifact must be checked independently before bundling.
- **Argos Translate** is an MIT/CC0 Python application/library based on
  CTranslate2. Its package-model licensing is not stated clearly enough for a
  default commercial bundle; an official-repository issue explicitly asks for
  clarification. Use only after model-by-model legal review.
- **LibreTranslate** is an AGPL-3.0 HTTP server around Argos. It adds a server
  layer without resolving model licensing and is unnecessary for the local
  OSB baseline; it can remain an optional adapter.
- **NLLB-200** offers broad language coverage but official weights such as the
  distilled 600M model are CC-BY-NC 4.0 and unsuitable as a required/default
  commercial engine.
- **Omnilingual MT** is an important 1,600-language research announcement. No
  official generally downloadable production weights/license were located in
  this review. Availability and redistribution are therefore **UNKNOWN —
  LICENSE REQUIRES REVIEW**.
- **SONAR** is a multilingual embedding/decoder ecosystem useful for research;
  its released checkpoints carry non-commercial restrictions and it is not a
  simpler PT->EN product pipeline.

### VAD and noise suppression

- **Silero VAD** is MIT, has ONNX support and a roughly 2 MB model. Its official
  repository reports processing a 30+ ms chunk in under 1 ms on one CPU thread
  under its stated conditions. Benchmark on the OSB target machine.
- **WebRTC VAD** is a minimal fallback with strict mono PCM framing: 10, 20 or
  30 ms frames at 8/16/32/48 kHz. The commonly used wrapper is MIT and the
  underlying WebRTC code is BSD-style.
- **RNNoise** is BSD-3-Clause and designed for real-time recurrent noise
  suppression. It is preferable to building denoising; keep it optional
  because denoising can damage recognition in some environments.
- **SpeexDSP** remains a mature BSD-licensed DSP alternative. It is useful as a
  fallback, not a reason to construct a custom DSP stack.

### Speech synthesis

- **Kokoro-82M** is an 82M-parameter model whose official Hugging Face card is
  Apache-2.0; the main weight file is about 327 MB. English is its strongest
  fit for this MVP. Verify every selected voice asset at packaging time.
- **sherpa-onnx** is Apache-2.0, exposes C/C++ APIs and documents CPU offline
  TTS support for Kokoro and Piper. It is a practical reusable runtime boundary.
- **Piper** needs precise naming. The old `rhasspy/piper` project was archived;
  the maintained `OHF-Voice/piper1-gpl` code is GPL-3.0 and individual voice
  models carry their own licenses/model cards. Support as an external optional
  engine only after distribution design and legal review.
- **Coqui TTS** community code is MPL-2.0. Model licenses vary independently.
  **XTTS-v2** weights use the Coqui Public Model License, which restricts use to
  non-commercial purposes. It must not be the default commercial-capable path.
- **MeloTTS** code is presented as MIT, but a sufficiently authoritative,
  consistent license statement for all redistributed checkpoints/voices was
  not established here: **LICENSE REQUIRES REVIEW**.

### Audio and media

- **PipeWire** is the correct primary Linux graph: it provides low-latency
  process/device graphs, IPC and real-time scheduling primitives. **WirePlumber**
  is its session/policy manager and must be treated as part of integration
  testing, not replaced inside OSB.
- **JACK** offers professional low-latency routing, but PipeWire already exposes
  JACK compatibility. A separate JACK backend is not MVP work.
- **CPAL** is a useful Apache/MIT cross-platform device abstraction for later
  platforms, but it does not replace direct PipeWire node metadata, virtual
  source creation and WirePlumber policy integration.
- **Rodio** is playback-focused and does not solve virtual microphone routing.
- **FFmpeg** and **GStreamer** are valuable file/codec/diagnostic adapters, but
  are too broad to become OSB's core live-audio scheduler. Their effective
  licenses depend on build/plugins (FFmpeg LGPL/GPL; GStreamer core LGPL).

## Meta Seamless Investigation

### What it can replace

SeamlessM4T v2 exposes ASR, speech-to-text translation, text-to-text
translation, text-to-speech translation and speech-to-speech translation.
Its official model card lists Portuguese and English as supported speech input
and speech output languages. SeamlessStreaming adds simultaneous streaming
S2ST/S2TT/ASR so output can start before the complete utterance. At the logical
engine level, either can replace separate ASR + MT + TTS components for a
supported language pair.

It does **not** eliminate:

- physical capture and virtual-device publication;
- real-time-safe bounded buffers and sample-rate/channel conversion;
- session lifecycle, cancellation, health checks and crash isolation;
- endpoint/turn policy, even if the model consumes incremental audio;
- output pacing/jitter handling and application integration;
- privacy controls, logs and license policy.

Therefore the accurate statement is: Seamless can replace the three semantic
inference stages behind one provider, but it cannot replace OSB and should not
be assumed to replace all VAD/segmentation responsibilities.

### Models, runtime and integration

| Family | Capability | Published scale/artifact | Practical OSB integration |
|---|---|---|---|
| SeamlessM4T v2 Large | offline ASR/S2TT/T2TT/T2ST/S2ST | 2.3B parameters; HF repository observed around 29.9 GB, individual major checkpoints around 9–11 GB | Persistent Python/fairseq2 worker first; do not load inside daemon. |
| SeamlessStreaming | simultaneous S2ST/S2TT/ASR | 2.5B parameters; HF artifacts observed around 8 GB | Persistent Python worker with incremental protocol and cancellation. |
| SeamlessExpressive | expressive S2ST | large gated research assets | Research-only worker; published expressive target set is EN/ZH/DE/FR/IT/ES, not PT. |
| Seamless unified model | streaming + expressive research | custom-gated model | Same isolation; not a commercial default. |

The official `seamless_communication` runtime uses Python, PyTorch and
`fairseq2`. `fairseq2` supports Linux x86-64 and CPU packages, so CPU execution
is technically possible. That is not evidence of interactive speed on a normal
laptop CPU. With 2.3B–2.5B parameters, large checkpoint files and vocoder
processing, memory bandwidth and RAM pressure are substantial. No primary
source benchmark located here demonstrates real-time PT<->EN S2S on the target
CPU class. GPU acceleration should be considered operationally expected for
the best experience until OSB measures otherwise.

The native `unity.cpp` path documented by Meta is limited to S2TT/ASR and does
not supply the full released speech-generation path. A Python daemon/subprocess
is therefore the honest integration route for S2S today. A Rust binding should
not be written merely to avoid a supervised process.

### Expressivity and voice identity

The SeamlessExpressive paper and documentation evaluate preservation of speech
rate, pauses and vocal style/prosody while translating. These properties can
make the output feel closer to the speaker. They do not establish consistent
biometric identity/timbre cloning for arbitrary speakers, and the project
should not describe the feature as voice cloning. Portuguese is supported by
SeamlessM4T v2 broadly, but it is not among the documented expressive target
languages for SeamlessExpressive evaluation.

### Meta Seamless verdict

**Best end-to-end research candidate; reject as the default/MVP dependency.**

Reasons:

1. It proves Architecture B is technically real and deserves a provider slot.
2. PT/EN speech input/output exists in M4T v2.
3. Streaming can reduce wait-for-end-of-sentence latency.
4. Official M4T v2/Streaming weights are CC-BY-NC 4.0; Expressive/unified use a
   custom non-commercial research license.
5. Scale and unproven laptop-CPU RTF conflict with the baseline hardware goal.
6. The Python/fairseq2 surface and project churn increase packaging risk.

## Modular Pipeline Investigation

### Recommended first composition

```text
PipeWire 48 kHz capture
  -> RT-safe SPSC queue
  -> non-RT downmix/resample to engine format
  -> Silero VAD + utterance/incremental chunk policy
  -> Whisper translate PT speech directly to EN text
  -> stability/commit policy
  -> Kokoro EN synthesis
  -> resample/channel map to 48 kHz F32
  -> bounded output queue + underrun policy
  -> PipeWire virtual source callback
```

`whisper.cpp` and `faster-whisper` should be benchmarked, not prematurely
standardized. The selection procedure should test quantized `base`, `small`
and, only if resources allow, `medium`; choose the smallest model meeting the
quality and latency acceptance gate on representative Brazilian Portuguese.

When the target is not English, insert a `TextTranslationEngine` after ASR or
choose a direct multilingual speech-translation provider. This is why the
protocol should not hard-code the MVP shortcut.

### Streaming techniques worth implementing in the bridge

- 10–30 ms capture frames into bounded rings; never enqueue unbounded PCM.
- Rolling inference windows with explicit overlap and deduplication.
- VAD hysteresis, minimum speech, trailing silence and maximum utterance cap.
- Partial text with `provisional`, `revised`, and `committed` states.
- Translation only on stable spans; optionally speculate, then cancel or revise.
- Sentence/clause segmentation rather than waiting for long turns.
- Persistent warm workers and preloaded models.
- Streaming/chunked TTS where supported; otherwise synthesize committed clauses.
- Stage overlap: capture N+1 while translating/synthesizing N.
- Output high/low watermarks, silence on underrun, and discontinuity counters.

## License Matrix

This matrix is an engineering screen, not legal advice. “Commercial?” refers
to the examined official default artifact, not every third-party derivative.
Every exact model and voice must be recorded with URL, revision and hash before
distribution.

| Technology | Code | Model/weights | Commercial? | Redistribution? | Notes |
|---|---|---|---|---|---|
| Seamless Communication runtime | MIT for code/non-generative parts | varies | code: yes | code: yes | Model terms below control checkpoints. |
| SeamlessM4T / v2 | MIT code | CC-BY-NC 4.0 | **No** for commercial default | Yes only under CC terms, NC + attribution | Optional research provider. |
| SeamlessStreaming | MIT code | CC-BY-NC 4.0 | **No** | Yes only under CC terms, NC + attribution | Optional research provider. |
| SeamlessExpressive / unified | MIT portions | Meta Seamless license | **No; non-commercial research** | Only under custom terms | Gated; outputs also subject to terms. |
| fairseq2 | MIT | N/A | Yes | Yes, with notice | Runtime, not a model license. |
| MMS | MIT ecosystem code | CC-BY-NC 4.0 inspected cards | **No** | NC terms + attribution | Check every checkpoint. |
| NLLB-200 | MIT/fairseq ecosystem | CC-BY-NC 4.0 | **No** | NC terms + attribution | Not a required/default engine. |
| Omnilingual ASR | Apache-2.0 | Apache-2.0 | Yes | Yes, with notices | Strong future multilingual candidate. |
| Omnilingual MT | not established for release | not established | **UNKNOWN** | **UNKNOWN** | **LICENSE REQUIRES REVIEW; public weights not located.** |
| SONAR | MIT code | NC terms for inspected checkpoints | **No** for those weights | Under model terms | Research embeddings/decoders. |
| Whisper | MIT | MIT | Yes | Yes, with notice | Direct PT-speech -> EN-text candidate. |
| whisper.cpp | MIT | uses Whisper MIT weights | Yes | Yes, with notice | Native CPU/quantization candidate. |
| faster-whisper | MIT | converted Whisper weights remain MIT | Yes | Yes, with notice | CTranslate2 runtime. |
| CTranslate2 | MIT | model-dependent | Yes for code | Yes for code | SSE4.1 minimum; optimized AVX/AVX2/AVX512 paths. |
| Silero VAD | MIT | MIT repository/model distribution | Yes | Yes, with notice | Preferred VAD candidate. |
| WebRTC VAD | BSD-style core; wrappers vary (common Python wrapper MIT) | embedded algorithm | Yes | Yes, with notices | Verify chosen binding. |
| RNNoise | BSD-3-Clause | bundled/trained model under repository terms | Yes | Yes, with notice | Optional denoise. |
| SpeexDSP | revised BSD | N/A | Yes | Yes, with notice | Optional DSP fallback. |
| Kokoro-82M | Apache-2.0 repository | Apache-2.0 model card | Yes | Yes, with notices | Re-check each packaged voice asset. |
| sherpa-onnx | Apache-2.0 | model-dependent | Yes for code | Yes for code | Good C/C++ inference boundary. |
| Piper (current OHF) | GPL-3.0 | per voice/model card | Depends | GPL + model-specific | **LICENSE REQUIRES REVIEW** for distribution architecture. |
| Piper (archived rhasspy) | MIT repository, archived | per voice/model card | Depends | model-specific | Do not infer current Piper is MIT. |
| Coqui TTS | MPL-2.0 | model-dependent | Code: yes | MPL obligations | Audit each model separately. |
| XTTS-v2 | MPL toolkit | Coqui Public Model License | **No** (weights) | Restricted | Non-commercial model/output terms. |
| MeloTTS | MIT-stated code | unclear across assets | **UNKNOWN** | **UNKNOWN** | **LICENSE REQUIRES REVIEW.** |
| Argos Translate | MIT/CC0 code/data files | language-package license unclear | **UNKNOWN for bundled models** | **UNKNOWN** | **LICENSE REQUIRES REVIEW.** |
| LibreTranslate | AGPL-3.0 | Argos packages, unclear per package | Depends | AGPL source obligations + model terms | Optional server adapter only. |
| Marian NMT | MIT | per model | Code: yes | per model | Pin and audit exact OPUS artifact. |
| OPUS-MT models | N/A | often Apache-2.0, card-specific | Often yes | card-specific | Never generalize across all models. |
| Bergamot translator | MPL-2.0 | Mozilla model artifacts MPL-2.0 in official distribution | Yes with obligations | Yes with MPL obligations | Audit exact binary/model bundle. |
| PipeWire | primarily MIT; repository exceptions | N/A | Yes | With notices | Audit distro/bundled components. |
| WirePlumber | MIT | N/A | Yes | With notice | External system component preferred. |
| JACK/JACK2 | GPL family with linking/interface nuances | N/A | Yes | License obligations | **LICENSE REQUIRES REVIEW** before bundling. |
| CPAL | Apache-2.0 | N/A | Yes | Yes, with notices | Future portable device I/O. |
| Rodio | MIT OR Apache-2.0 | N/A | Yes | Yes, with notices | Playback helper, not virtual mic. |
| FFmpeg | LGPL-2.1+ or GPL depending build/options | codec-dependent | Depends on build | Depends on build | **LICENSE REQUIRES REVIEW** for distributed binary. |
| GStreamer | LGPL-2.1+ core | plugin/codec-dependent | Depends on bundle | Depends on bundle | Prefer system packages/adapters. |

## CPU-only Feasibility

| Candidate | CPU/RAM/startup assessment | SIMD/quantization | Verdict for baseline |
|---|---|---|---|
| Silero VAD ONNX | Tiny (~2 MB), warm quickly; negligible beside ASR | ONNX CPU optimizations | Yes. |
| WebRTC VAD | Extremely small and deterministic | Native integer/DSP paths | Yes, fallback. |
| whisper.cpp base/small | Hundreds of MB unquantized; quantization reduces RAM/disk; multithreaded | quantized variants, SIMD/BLAS options | Most promising native CPU baseline; benchmark accuracy. |
| faster-whisper base/small | Python worker plus CTranslate2; CPU INT8 and threads | SSE4.1 minimum; AVX-family optimizations | Strong alternative; process overhead is amortized by persistent worker. |
| Kokoro-82M ONNX | ~327 MB main weights plus runtime/voices; much smaller than multi-billion S2S | ONNX CPU graph optimization/possible quantization | Plausible; benchmark first-audio and RTF. |
| Marian/Bergamot small pair | Hundreds of MB is typical but exact artifact determines RAM | 8-bit/CPU-oriented paths available | Plausible when separate MT is required. |
| NLLB 600M | Multi-GB repository/runtime pressure | quantization possible | Technically possible, but NC and not the minimum path. |
| SeamlessM4T v2 / Streaming | 2.3B–2.5B params, multi-GB checkpoints, PyTorch/fairseq2/vocoder | CPU supported, GPU commonly advantageous | Not credible as default laptop-CPU real-time without new measurements. |

Do not publish a universal RTF from upstream marketing. RTF changes with CPU,
thread count, model, quantization, language, utterance length, decoder settings
and thermal state. The benchmark plan below is the decision gate.

## Latency Analysis

### Planning bands

| User-visible band | Interpretation |
|---|---|
| `< 500 ms` | Excellent, but generally unrealistic for stable, high-quality CPU-only translated speech because translation needs linguistic look-ahead. Treat as an experimental partial-output target. |
| `500 ms–1.5 s` | Stretch target for first stable text/first synthesized audio on short clauses and tuned hardware. |
| `1.5–3 s` | Honest MVP acceptance band after end-of-utterance on ordinary CPU, subject to measurement. |
| `> 3 s` | Poor for turn-taking; may remain usable for long-form/assistive modes but should fail the conversation preset. |

### Budget hypothesis, not a benchmark result

For a short utterance, an initial engineering budget after speech endpoint is:

- endpoint trailing silence: 200–500 ms;
- ASR/direct translation: 400–1,500 ms;
- text commit/normalization: under 50 ms;
- first TTS audio: 150–800 ms;
- output queue: 40–120 ms.

Stages must overlap, so totals are not a simple sum. Pre-speech capture latency
and the duration needed to understand a clause must be reported separately from
post-endpoint compute. This distinction prevents misleading “latency” numbers.

### Runtime graph and backpressure

```text
PipeWire RT callback
  -> bounded capture SPSC (drop + discontinuity metric on overflow)
  -> Rust non-RT preprocessor / VAD / segmenter
  -> bounded inference-job queue
  -> supervised persistent engine worker(s)
  -> bounded partial/commit event queue
  -> TTS worker
  -> bounded jitter/output ring
  -> PipeWire virtual-source RT callback (silence + underrun metric if empty)
```

No worker completion, Python call, socket operation, allocation-heavy logging or
disk I/O belongs in either PipeWire callback. Backpressure must be explicit:
prefer dropping the oldest stale capture span, marking a discontinuity and
recovering at a speech boundary over blocking real-time audio.

## Voice Preservation Analysis

### 1. Generic TTS

Generic TTS speaks translated text with a selected synthetic voice. It can be
clear, fast and legally simpler, but it does not preserve the user's timbre or
identity. This is the correct MVP scope.

### 2. Voice cloning

Voice cloning estimates speaker characteristics from reference audio and uses
them in synthesis. It raises consent, biometric-data, impersonation, retention
and model-license risks. XTTS-v2 is a familiar example but its model license is
non-commercial. OSB should require an explicit user action, clear active-state
indicator, revocation/deletion controls, and never silently collect enrollment
audio. It is not required to prove product value.

### 3. Expressive speech-to-speech translation

Expressive S2S attempts to transfer prosody, tempo, pauses and emotional/vocal
style across languages. That can sound recognizably similar in performance
without reproducing biometric voice identity. SeamlessExpressive supports this
category according to its paper, but it does not promise general voice cloning,
has limited evaluated expressive language directions, large runtime needs and
non-commercial terms. OSB must label “expressivity preservation” and “voice
identity cloning” separately in UX, protocol capabilities and documentation.

### What is realistic now

- MVP: a good generic English voice with controllable rate and low first-audio
  latency.
- Later, permissively licensed style/voice adapters after consent and legal
  review.
- Research mode: SeamlessExpressive comparison, explicitly non-commercial.
- Not a valid promise: perfect identity, emotion and timing preservation on
  ordinary CPU across arbitrary languages.

## Proposed Provider Architecture

Use capability-oriented, streaming interfaces rather than a single universal
`transcribe()` function. Names are illustrative:

```rust
trait VoiceActivityEngine {
    fn capabilities(&self) -> VadCapabilities;
    async fn stream(&self, input: AudioStream) -> Result<VadEventStream>;
}

trait SpeechRecognitionEngine {
    fn capabilities(&self) -> RecognitionCapabilities;
    async fn recognize(&self, request: RecognitionRequest)
        -> Result<TranscriptEventStream>;
}

trait SpeechTranslationEngine {
    fn capabilities(&self) -> SpeechTranslationCapabilities;
    async fn translate_speech(&self, request: SpeechTranslationRequest)
        -> Result<TextEventStream>;
}

trait TextTranslationEngine {
    fn capabilities(&self) -> TranslationCapabilities;
    async fn translate(&self, request: TranslationRequest)
        -> Result<TextEventStream>;
}

trait SpeechSynthesisEngine {
    fn capabilities(&self) -> SynthesisCapabilities;
    async fn synthesize(&self, request: SynthesisRequest)
        -> Result<AudioEventStream>;
}

trait SpeechToSpeechEngine {
    fn capabilities(&self) -> S2sCapabilities;
    async fn translate_audio(&self, request: S2sRequest)
        -> Result<AudioEventStream>;
}
```

`AudioSource`, `AudioSink`, resampler and scheduler belong to the Rust runtime
port layer, not the ML-provider registry. This keeps real-time ownership clear.

Every provider advertises at least:

- operation modes and supported source/target languages;
- streaming vs batch, partial/revision/commit semantics;
- accepted sample formats/rates/channels and produced formats;
- CPU/GPU runtime and quantization modes;
- model ID, immutable revision/hash, code license and weights license;
- commercial-use policy and redistribution policy;
- warmup/readiness, maximum chunk/context and concurrency;
- cancellation, deadlines and restart behavior.

The pipeline planner chooses either:

```text
VAD -> SpeechTranslationEngine -> SpeechSynthesisEngine
```

or:

```text
VAD -> SpeechRecognitionEngine -> TextTranslationEngine -> SpeechSynthesisEngine
```

or:

```text
SpeechToSpeechEngine
```

without changing the PipeWire graph.

## IPC / Runtime Architecture

### Decision

Use a **persistent supervised local worker process** per heavy runtime/model
family, communicating over a **Unix Domain Socket (UDS)** by default.

| Option | Assessment |
|---|---|
| Embed Python/PyTorch in daemon | Reject for baseline: large failure surface, GIL/runtime lifecycle, allocator and native-library conflicts; violates crash-isolation intent. |
| Spawn process per utterance | Reject: model load/startup dominates latency. |
| Persistent child subprocess | Good MVP: simple ownership and restart; connect with inherited or filesystem UDS. |
| Independent local daemon | Good evolution: model sharing and independent lifecycle; requires authentication/version management. |
| localhost HTTP | Not preferred: unnecessary serialization/security surface for local PCM. |
| UDS | Preferred local transport: filesystem permissions, low overhead and clear local-only boundary. |
| gRPC | Optional remote/plugin transport later; useful for typed cross-language APIs but not required for local MVP. |
| In-process C/C++ | Allow only for small audited engines with stable C ABI when a crash taking down OSB is acceptable; never call from RT callbacks. |

### Wire protocol shape

- Versioned handshake with provider/model/capability/license metadata.
- Control messages encoded with a length-prefixed schema (for example
  Protobuf, CBOR or MessagePack after a focused ADR).
- Binary PCM frames, not JSON floats.
- `session_id`, `stream_id`, `utterance_id`, monotonically increasing
  `chunk_id`, capture timestamp, audio format and flags on each frame.
- Flags for discontinuity, speech start/end, end-of-utterance and end-of-stream.
- Bounded credit/window flow control so a slow worker cannot grow memory.
- Partial/revised/committed text events with span IDs.
- Explicit deadlines, cancel, health, warmup, error category and retryability.

Start with UDS plus framed binary copies. Add shared-memory/memfd rings only if
profiling shows IPC copies are material; premature shared memory would multiply
lifetime and synchronization complexity.

## Architecture A — Modular Pipeline

```text
AudioSource -> VAD -> (ASR/direct speech translation) -> optional MT -> TTS -> AudioSink
```

Strengths:

- permissively licensed commercial-capable default stack is available;
- each stage can be measured, replaced and degraded independently;
- realistic CPU-only models exist;
- text provides debugging, accessibility/captions and auditability when opted in;
- broad language extension can select exact model pairs;
- failures can fall back (for example S2S -> text pipeline).

Costs:

- handoff/segmentation errors accumulate across stages;
- synthetic voice normally loses source expressivity/identity;
- more scheduling interfaces and buffer boundaries;
- speculative partials and revisions require careful UX and TTS cancellation.

## Architecture B — End-to-End Speech-to-Speech

```text
AudioSource -> SpeechToSpeechEngine -> AudioSink
```

Strengths:

- one semantic engine boundary;
- models can preserve more prosodic information than a text bottleneck;
- simultaneous models can overlap understanding and generation;
- fewer externally orchestrated inference stages.

Costs:

- leading open candidates are large and GPU-oriented in practice;
- Meta candidates have NC/custom licenses incompatible with a required default;
- harder to inspect/correct text and harder to isolate quality failures;
- language/mode coverage varies and native deployment is less mature;
- one large engine failure removes the whole semantic path.

## Comparison

| Criterion | Architecture A: modular | Architecture B: S2S |
|---|---|---|
| CPU-only feasibility | **Higher** with quantized ASR + small TTS | Low/unknown for current high-quality candidates |
| RAM/disk | Selectable; roughly hundreds of MB to low GB | Commonly many GB |
| Latency | More stages, but stages can overlap and use small models | Potential algorithmic advantage; target-CPU evidence missing |
| Translation quality | Strong and debuggable; cascading errors possible | Strong research results; opaque error attribution |
| Voice/prosody | Generic TTS unless extra model added | Better expressive potential |
| Commercial default licensing | **Feasible** with permissive exact artifacts | Meta candidates fail NC requirement |
| Integration maturity | C/C++/ONNX/Python options | Mostly Python/PyTorch for full S2S |
| Extensibility | **High**, stage-by-stage providers | Model/language matrix controls everything |
| Maintenance | More contracts, smaller replaceable parts | Simpler outer graph, much larger engine dependency |
| Risk | Moderate, measurable | High for CPU, license and packaging |
| Best role | Default product architecture and MVP | Optional experimental provider |

## Recommended MVP

### Scope

One Ubuntu user speaks Brazilian Portuguese into a selected physical
microphone. OSB produces intelligible English speech through one PipeWire
virtual microphone that a real external consumer can record. Generic English
voice is acceptable. No GUI, cloning, Windows/macOS, arbitrary language matrix
or cloud account is required.

### Candidate stack

1. Existing PipeWire capture and virtual-source path.
2. Existing bounded `osb-audio` rings and Rubato conversion.
3. Silero VAD ONNX (or WebRTC VAD if integration/benchmark wins).
4. Benchmark winner: `whisper.cpp` quantized base/small versus
   `faster-whisper` CPU INT8, using Whisper `translate`.
5. Kokoro-82M English through sherpa-onnx/ONNX worker.
6. Persistent supervised workers over UDS; binary framed PCM.

### Acceptance criteria

- A fixed set of live PT-BR phrases is heard as semantically correct English
  from `pw-record` attached to the OSB virtual source.
- One conferencing app receives the same virtual microphone in a documented
  manual test.
- No network/API key is required after explicitly installed model artifacts.
- No audio or transcript persists by default.
- Real-time callbacks contain no inference, IPC, disk I/O, logging or blocking.
- Queue overflow/underrun, stage latency, CPU and RSS are observable.
- p95 post-endpoint first-audio is at most 3 s on the named acceptance machine,
  or the milestone remains experimental with the measured limitation recorded.

## Benchmark Plan

### Corpus

- 10 short PT-BR phrases (greetings, confirmations, numbers, names).
- 10 medium phrases (multi-clause everyday conversation).
- 1 minute of continuous speech with pauses, repairs and natural rate.
- At least three speakers if consented; retain a synthetic/public alternative
  for CI. Include quiet and one controlled noise condition.
- Human-authored English references; record ambiguity and acceptable variants.

### Reproducibility record

Record OSB commit/dirty state, model URL/revision/hash/license, OS/kernel,
PipeWire/WirePlumber, CPU model and instruction set, physical/logical cores,
RAM, power mode, thread count, quantization, beam/decoder settings, sample
format, capture quantum, warm/cold state and ambient condition.

### Clocked events

Use a monotonic clock and propagate capture timestamps:

1. first captured speech frame;
2. VAD speech start and endpoint;
3. first provisional text;
4. first stable/committed text;
5. translation committed (if separate MT);
6. first synthesized PCM frame;
7. first frame written by virtual-source callback;
8. first frame observed by a `pw-record`/consumer tap.

### Metrics

- latency-to-first-provisional-text and first-stable-text;
- latency-to-first-audio from speech start and from endpoint;
- end-to-end p50/p95/p99, plus utterance duration;
- per-stage and pipeline RTF (processing time / input duration);
- process CPU, peak/RSS RAM, model startup and warmup time;
- captured/published frames, drops, discontinuities, ring high-watermarks,
  underruns and overflows;
- ASR WER/CER on the transcription path;
- BLEU/chrF and preferably COMET for translation, always paired with blinded
  human adequacy/fluency review because valid translations vary;
- intelligibility/naturalness listening score for synthesized output.

### Experiment matrix

- whisper.cpp base/small, available q5/q8 variants, fixed threads;
- faster-whisper base/small CPU INT8, fixed threads;
- VAD thresholds/end silence 200/350/500 ms;
- cold start once, warm run at least 10 repetitions per phrase;
- batch utterance baseline versus incremental rolling window;
- output through an actual PipeWire consumer, not only internal counters.

Export raw event rows as JSONL/CSV without audio or text by default. A separate
explicit research flag may retain redacted transcripts/audio with consent.

## Security / Privacy

- Process audio in memory and discard bounded buffers after consumption.
- Disable conversation recording and transcript persistence by default.
- Logs contain IDs, durations, sizes and error categories—not raw PCM or text.
- Bind UDS inside a user-private runtime directory with restrictive permissions;
  reject other-user clients and version-mismatched workers.
- Treat model download as a separate explicit operation with checksum, origin,
  revision and license manifest; runtime operation can then be fully offline.
- Sandboxing should deny network to local workers where practical and grant
  only required model/runtime paths.
- Clear buffers and terminate workers on session stop; document that OS-level
  swapping/crash dumps may require additional hardening for high-risk users.
- Voice embeddings/enrollment audio are biometric data. Voice cloning requires
  explicit consent, visible enablement, purpose limitation and deletion.
- Never auto-select a virtual microphone for unrelated applications or record
  other system streams without explicit configuration.

## Risks

| Risk | Impact | Mitigation / decision gate |
|---|---|---|
| CPU ASR/TTS misses 3 s p95 | MVP feels unusable | Benchmark base/small, quantization, clause sizes; define hardware floor; optional GPU provider. |
| Partial translations change meaning | Wrong speech is emitted and cannot be “unsaid” | Speak only committed spans initially; add speculation only with cancellation UX/quality tests. |
| PipeWire policy differs by distro | Virtual mic absent or links incorrectly | Native registry, explicit target IDs, Fedora/Ubuntu matrix, WirePlumber diagnostics. |
| Feedback loop captures OSB output | Repeated/unstable speech | Pin physical target, exclude own node, graph-cycle detection and clear UX. |
| Worker crash/backlog | Audio thread starvation or memory growth | Supervision, bounded credits, deadlines, silence fallback, restart limits. |
| Model license ambiguity | Distribution/commercial exposure | Per-artifact SBOM/license manifest; fail closed; no NC model in default install. |
| Python packaging/runtime drift | Installation failures | Pin worker environment/artifacts; health handshake; prefer stable ONNX/C APIs where justified. |
| Translation quality varies by accent/noise | Loss of trust | PT-BR corpus, human evaluation, confidence/caption option, optional denoise. |
| Voice cloning misuse | Privacy and impersonation harm | Exclude from MVP; explicit consent and local biometric-data lifecycle. |
| Documentation overstates branch evidence | False readiness | Label branch/release/current-run evidence separately, as this report does. |

## Open Questions

1. What exact CPU/RAM machine is the acceptance baseline?
2. Is the initial UX push-to-talk, automatic VAD, or both? Push-to-talk is a
   valuable diagnostic/fallback mode even if VAD is the default.
3. May English text be shown transiently for correction/captions, or is the
   first UX audio-only?
4. Which conferencing app is the first formal compatibility target?
5. Does distribution bundle models or provide an explicit downloader? The
   latter simplifies licensing and package size but still needs license gates.
6. Which wire schema should the focused IPC ADR select after a small measured
   spike: Protobuf, CBOR or MessagePack?
7. Does `whisper.cpp` or `faster-whisper` win on the named CPU and PT-BR corpus?
8. Does Kokoro meet real-time and voice-quality thresholds on that CPU, and
   which exact voices are legally distributable?
9. What behavior is preferred when inference falls behind: drop to next
   utterance, emit delayed speech, or switch to captions?
10. Should optional NC research engines be compiled/distributed separately to
    avoid confusing the Apache-2.0 product surface?

## RECOMMENDED ARCHITECTURE

Adopt **Architecture A as the default capability-composed runtime**, using
direct Whisper speech translation for the PT->EN MVP and preserving a parallel
`SpeechToSpeechEngine` provider interface for Architecture B experiments.

The core deployment is:

```text
Rust daemon
  owns PipeWire, timestamps, bounded buffers, scheduling, policy and metrics
    |
    +-- UDS -> persistent VAD/ASR/speech-translation worker
    +-- UDS -> persistent optional MT worker
    +-- UDS -> persistent TTS worker
    +-- UDS -> optional S2S worker
```

This is the smallest architecture that satisfies all hard constraints:

- it keeps real-time audio safe and independent of Python;
- it works offline without a recurring fee;
- it has a plausible permissively licensed CPU-only default;
- it permits engine crashes/restarts without killing the audio daemon;
- it can incorporate Seamless later without redesigning audio routing;
- it avoids implementing any speech model.

Architecture B is retained as a first-class optional path, not selected as the
baseline, because current promising end-to-end candidates fail the commercial
license and/or ordinary-CPU evidence gates.

## WHAT WE SHOULD BUILD

- Native, robust PipeWire discovery, capture, virtual microphone and reconnection.
- Real-time-safe bounded graph, timestamps, discontinuities, jitter/output
  pacing, backpressure and failure policy.
- Provider capability registry, pipeline planner and per-session configuration.
- Worker supervision, UDS framing, binary audio flow control, health, deadlines,
  cancellation and restart behavior.
- Partial/revised/committed utterance semantics and safe clause scheduling.
- Metrics, benchmark harness, compatibility tests and diagnostic UX.
- Model artifact inventory, hash/license policy, offline installer/downloader
  workflow and privacy controls.
- End-user selection of physical input, virtual output and languages.

## WHAT WE SHOULD REUSE

- PipeWire/WirePlumber for Linux graph and policy integration.
- Existing `rtrb` buffers and Rubato resampling.
- Silero/WebRTC for VAD; RNNoise/SpeexDSP if denoising proves necessary.
- Whisper/whisper.cpp/faster-whisper/CTranslate2 for ASR/direct EN translation.
- Marian/Bergamot or another exact permissive model for future text MT.
- Kokoro/sherpa-onnx for baseline English synthesis.
- Seamless, Omnilingual ASR and other large research models only as optional
  providers under their actual license/hardware constraints.
- Existing schema/serialization libraries after a focused protocol ADR; do not
  design a bespoke serialization format.

## Proposed Roadmap

### M2 — Engine Contract and Reproducible Harness

- **Objective:** turn protocol concepts into a versioned, bounded local-worker
  contract and measure candidate engines independently of live audio.
- **Dependencies:** focused IPC ADR; accepted license-manifest fields; PT-BR
  benchmark corpus.
- **Acceptance:** external mock worker handshake over UDS, framed binary PCM,
  cancellation/backpressure tests, crash/restart test, benchmark event schema.
- **Risks:** overdesigning shared memory or coupling the schema to Whisper.

### M3 — Offline PT-BR -> English Speech Prototype

- **Objective:** prove real speech -> translated text -> generic English audio
  completely offline.
- **Dependencies:** M2; benchmark and license selection for Whisper runtime,
  Silero and exact Kokoro artifacts.
- **Acceptance:** 20-phrase + continuous-speech corpus produces intelligible
  English; raw latency/RTF/CPU/RSS recorded; no persistent audio/text by default.
- **Risks:** CPU p95, endpointing and TTS first-audio latency.

### M4 — Virtual Mic End-to-End

- **Objective:** join M3 to the existing physical capture/virtual source path.
- **Dependencies:** PR #14/M1 stabilization; native registry work; M3.
- **Acceptance:** selected physical microphone reaches `pw-record` and one real
  conferencing app as English speech; no callback blocking; underrun/overflow
  metrics and reconnection tests exist.
- **Risks:** WirePlumber policy, feedback loops, distro/device variation.

### M5 — Streaming and Low Latency

- **Objective:** incremental ASR/direct translation, clause commits, stage
  overlap and chunked synthesis.
- **Dependencies:** reliable M4 metrics and recorded baseline.
- **Acceptance:** named target CPU meets agreed p95 band without regression in
  human adequacy; partial revisions never corrupt already emitted speech;
  1-minute continuous run has no unbounded growth.
- **Risks:** speculative errors, thermal throttling and queue oscillation.

### M6 — Reliability, Packaging and Multi-language

- **Objective:** distributable offline package plus first language pair that
  requires separate MT.
- **Dependencies:** model download/license manifest, worker sandboxing,
  compatibility matrix.
- **Acceptance:** clean Ubuntu install, offline second run, exact artifacts and
  notices recorded; provider fallback and failure UX tested.
- **Risks:** package size, per-model licenses and language quality variance.

### M7 — Voice Preservation / End-to-End Research

- **Objective:** compare expressive S2S and consented voice-preservation options
  without changing the default stack.
- **Dependencies:** stable `SpeechToSpeechEngine`, consent/data policy, suitable
  hardware and separately distributed research engines.
- **Acceptance:** blind comparison of prosody, timing, timbre similarity,
  adequacy and latency; UI never labels expressivity as cloning; NC restrictions
  are explicit and isolated.
- **Risks:** licenses, GPU requirements, biometric privacy, misuse and quality
  claims that exceed evidence.

## Primary Sources

### Meta / FAIR

- [Seamless Communication repository and model/license table](https://github.com/facebookresearch/seamless_communication)
- [SeamlessM4T v2 Large model card](https://huggingface.co/facebook/seamless-m4t-v2-large)
- [SeamlessStreaming model card](https://huggingface.co/facebook/seamless-streaming)
- [SeamlessExpressive documentation](https://github.com/facebookresearch/seamless_communication/blob/main/docs/expressive/README.md)
- [Seamless paper: streaming and expressive translation](https://arxiv.org/abs/2312.05187)
- [Meta Seamless custom license](https://ai.meta.com/resources/models-and-libraries/seamless-license/)
- [fairseq2 repository and platform support](https://github.com/facebookresearch/fairseq2)
- [MMS ASR model card](https://huggingface.co/facebook/mms-1b-all)
- [MMS English TTS model card](https://huggingface.co/facebook/mms-tts-eng)
- [NLLB-200 distilled 600M model card](https://huggingface.co/facebook/nllb-200-distilled-600M)
- [Omnilingual ASR official repository](https://github.com/facebookresearch/omnilingual-asr)
- [Omnilingual MT publication page](https://ai.meta.com/research/publications/omnilingual-mt-machine-translation-for-1600-languages/)
- [SONAR repository and model licensing](https://github.com/facebookresearch/SONAR)

### Whisper ecosystem

- [OpenAI Whisper repository, models and license](https://github.com/openai/whisper)
- [Whisper model card](https://github.com/openai/whisper/blob/main/model-card.md)
- [whisper.cpp repository](https://github.com/ggml-org/whisper.cpp)
- [whisper.cpp model sizes and downloads](https://github.com/ggml-org/whisper.cpp/blob/master/models/README.md)
- [faster-whisper repository and benchmarks](https://github.com/SYSTRAN/faster-whisper)
- [CTranslate2 repository](https://github.com/OpenNMT/CTranslate2)
- [CTranslate2 hardware support](https://opennmt.net/CTranslate2/hardware_support.html)

### VAD / denoising

- [Silero VAD repository](https://github.com/snakers4/silero-vad)
- [WebRTC VAD wrapper and framing documentation](https://github.com/wiseman/py-webrtcvad)
- [RNNoise repository](https://github.com/xiph/rnnoise)
- [SpeexDSP repository](https://github.com/xiph/speexdsp)

### TTS

- [Kokoro-82M model card and license](https://huggingface.co/hexgrad/Kokoro-82M)
- [Kokoro official repository](https://github.com/hexgrad/kokoro)
- [sherpa-onnx repository](https://github.com/k2-fsa/sherpa-onnx)
- [Current Piper repository](https://github.com/OHF-Voice/piper1-gpl)
- [Archived rhasspy Piper repository](https://github.com/rhasspy/piper)
- [Coqui community TTS repository](https://github.com/idiap/coqui-ai-TTS)
- [XTTS-v2 model license](https://huggingface.co/coqui/XTTS-v2/blob/main/LICENSE.txt)
- [MeloTTS repository](https://github.com/myshell-ai/MeloTTS)

### Translation

- [Argos Translate repository](https://github.com/argosopentech/argos-translate)
- [Argos model-license clarification issue](https://github.com/argosopentech/argos-translate/issues/533)
- [LibreTranslate repository](https://github.com/LibreTranslate/LibreTranslate)
- [Marian repository](https://github.com/marian-nmt/marian)
- [OPUS-MT model organization](https://huggingface.co/Helsinki-NLP)
- [Bergamot translator repository](https://github.com/browsermt/bergamot-translator)
- [Mozilla translation models and licenses](https://github.com/mozilla/translations)

### Audio

- [PipeWire architecture and documentation](https://pipewire.pages.freedesktop.org/pipewire/)
- [PipeWire source and licensing](https://github.com/PipeWire/pipewire)
- [WirePlumber documentation](https://pipewire.pages.freedesktop.org/wireplumber/)
- [JACK project](https://jackaudio.org/)
- [CPAL repository](https://github.com/RustAudio/cpal)
- [Rodio repository](https://github.com/RustAudio/rodio)
- [FFmpeg legal/licensing information](https://ffmpeg.org/legal.html)
- [GStreamer licensing FAQ](https://gstreamer.freedesktop.org/documentation/frequently-asked-questions/licensing.html)

---

The fundamental build-vs-reuse answer is: **OSB should create only the safe,
observable desktop bridge around replaceable engines.** Its minimum unique
technology is the reliable path from physical audio, through bounded and
supervised inference, back into an application-visible virtual microphone.
