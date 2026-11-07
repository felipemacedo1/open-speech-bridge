# Project Status

> Last updated: 2026-11-07

## Live Profiles, M5 Clause-Chunked Streaming TTS & M4 Full Duplex (PT-BR ↔ EN) — 2026-11-07

- Organized and pushed 4 atomic commits (`1ec3ccf`, `de5f5c8`, `75166a1`, `2ab4548`) for M1 PipeWire fixes, M2 CPU research/benchmark, M2 Engine Runtime (`OSBP` v1), and M3 Local Translation to `origin`, then implemented ready-to-use live TOML profiles, **Milestone 5 (Clause-Chunked Streaming TTS)**, and **Milestone 4 (Full Duplex Bidirectional Pipeline)**:
- **Ready-to-Use Live TOML Profiles (`config/`)**:
  - [`config/live-ptbr-to-en.toml`](../config/live-ptbr-to-en.toml): Outgoing live voice-call profile (`PT-BR -> EN`, conversational VAD parameters, `faster-whisper-small` `int8` + `Kokoro-82M` `af_heart`, virtual mic `"OpenSpeechBridge"`).
  - [`config/full-duplex-ptbr-en.toml`](../config/full-duplex-ptbr-en.toml): Bidirectional live voice-call profile adding `text-translator` (`en-US <-> pt-BR`) and virtual sink `"OpenSpeechBridge Incoming"`.
  - Extended `Config` (`crates/osb-core/src/config.rs`) and `Runtime::from_config_file` (`crates/osb-daemon/src/runtime.rs`) with `VadConfigParams`, `virtual_mic_name`, `virtual_sink_name`, `enable_virtual_mic`, `enable_virtual_sink`, `voice_id`, `incoming_voice_id`, `chunked_tts`, `min_clause_words`, `enable_incoming`, worker `env` maps, and automatic Python interpreter fallback (`tools/benchmark/.venv/bin/python` <-> `python3`).
- **M5 Clause-Chunked Streaming TTS (`osb-daemon::pipeline`)**:
  - Implemented `split_text_into_tts_clauses` (`crates/osb-daemon/src/pipeline.rs`) splitting translated text on sentence/clause boundaries (`.`, `!`, `?`, `,`, `;`, `:`, `—` with `min_clause_words`) and `stream_tts_clauses_to_ring`, which pushes Clause 1's `48 kHz` stereo audio into the SPSC `AudioRingProducer` immediately before synthesizing Clause 2 and releases the `EngineSupervisor` lock between stages/clauses.
  - Added `first_audio_ms` (`TTFA`), `clauses_synthesized`, and `last_first_audio_ms` metrics to `TranslatedUtterance`, `PipelineMetricsSnapshot`, and `openspeechbridge translate --format json`.
- **M4 Full Duplex Incoming Path (`osb-pipewire`, `osb-daemon`, `osb-cli`, `engines/sdk-python`)**:
  - Upgraded `PlaybackStream::start()` (`crates/osb-pipewire/src/linux/playback.rs`) with a real PipeWire RT output thread (`Direction::Output`, `MEDIA_CATEGORY => "Playback"`, `MEDIA_ROLE => "Communication"`) draining `AudioRingConsumer` lock-free via `pop_or_silence`.
  - Added `text_translation_worker.py` (`engines/sdk-python/osb_engine_sdk/workers/text_translation_worker.py`) supporting `en-US <-> pt-BR` via local MarianMT or offline builtin conversational lexicon fallback, plus `Kokoro-82M` PT-BR synthesis (`lang_code="p"`).
  - Updated `openspeechbridge run` (`crates/osb-cli/src/commands/run.rs`) to orchestrate concurrent `OutgoingBridge` (`CaptureStream -> Pipeline -> VirtualMicrophone`) and `IncomingBridge` (`VirtualSink -> Pipeline -> PlaybackStream`) with `--duration <SECS>` and `SIGINT`/`SIGTERM` graceful cleanup.
- **Validation**:
  - Rust gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` passed: **156 active Rust tests passed** (152 unit/integration + 4 doctests, including `test_m4_full_duplex_bidirectional_and_m5_chunked_tts`) and 15 PipeWire daemon tests remained ignored.
  - Python SDK gate: `python3 -m unittest discover -s engines/sdk-python/tests -v` passed (**5/5 tests**).
  - **M5 Chunked TTS (`PT-BR -> EN` on `short_01.wav`, `5.04 s` speech)**: `clauses_synthesized = 2`, `speech_stage_ms = 1,562 ms`, **`first_audio_ms = 2,944 ms` (`1,382 ms` TTS TTFA after speech stage, `< 1.5 s` target!)** vs. `total_turnaround_ms = 3,620 ms` (`0.718x` RTF).
  - **M4 Reverse Cascade (`EN -> PT-BR` on `/tmp/osb-m5-en.wav`)**: `CascadedSttMtTts(faster-whisper -> text-translator -> kokoro-tts)` produced `"Ele armou uma campainha sem fio. disse ele,"` (`4,025 ms` `48 kHz` stereo PT-BR audio, `clauses_synthesized = 2`, `first_audio_ms = 5,321 ms` vs. `total_turnaround_ms = 6,069 ms`).
  - **Live `run` Smoke Tests (`config/live-ptbr-to-en.toml` & `config/full-duplex-ptbr-en.toml`, `--foreground --duration 3`)**: verified clean worker startup (`2` and `3` supervised engines ready), active PipeWire Outgoing (`"OpenSpeechBridge"`) and Incoming (`"OpenSpeechBridge Incoming"`) bridges, and graceful shutdown with **0 orphan Python worker processes** and **0 leftover UDS sockets**.

## M3 First Local Translation (VAD, Pipeline, Python `OSBP` v1 Workers & CLI) — 2026-11-07

- Implemented the Milestone 3 local speech-to-speech translation pipeline across `osb-core`, `osb-audio`, `osb-daemon`, `osb-cli`, and `engines/sdk-python`:
- **Bounded VAD & Arbitrary-Chunk Resampling (`osb-audio`)**:
  - `VadSegmenter` (`crates/osb-audio/src/vad.rs`): hysteresis RMS voice activity detection with fixed pre-roll ring buffer, `min_speech_ms` click rejection, hangover silence endpointing, and strict `max_utterance_ms` upper bound.
  - `StreamingMonoResampler` & `resample_mono_buffer` (`crates/osb-audio/src/resample.rs`): stateful `rubato` resampling (`48 kHz -> 16 kHz` capture ingestion and `24 kHz -> 48 kHz` TTS adaptation) plus `SampleRate::TTS_24K`.
- **Capability-Composed Pipeline & TOML Configuration (`osb-core`, `osb-daemon`)**:
  - `Config::from_toml_str` / `WorkerProcessConfig` (`crates/osb-core/src/config.rs`) and `Runtime::from_config_file` (`crates/osb-daemon/src/runtime.rs`) automatically register configured UDS workers in `EngineSupervisor`.
  - `Pipeline` (`crates/osb-daemon/src/pipeline.rs`): negotiates `DirectSpeechTranslation` (`Whisper translate` + `TTS`), `CascadedSttMtTts` (`STT` + `MT` + `TTS`), or `EndToEndS2s`, bridging non-blocking SPSC `AudioRingBuffer`s (`run_ring_bridge`) or offline WAV streams without ever blocking PipeWire real-time callbacks or logging user transcripts.
- **Python `OSBP` v1 SDK & Persistent CPU Workers (`engines/sdk-python`)**:
  - Zero-dependency stdlib `OSBP` v1 binary codec (`osb_engine_sdk/framing.py`) and `BaseEngineWorker` (`osb_engine_sdk/worker.py`) with concurrent `Ping`/`Pong` + `Cancel` handling while single-worker ML inference executes.
  - Persistent offline-by-default workers (`HF_HUB_OFFLINE=1`): `faster_whisper_worker.py` (`small`/`int8`, MIT) and `kokoro_worker.py` (`Kokoro-82M` `24 kHz` `f32le`, Apache-2.0 code; verify voice pack provenance before redistributing weights).
- **CLI Commands (`osb-cli`)**:
  - Added `openspeechbridge translate` (`crates/osb-cli/src/commands/translate.rs` + RIFF/WAVE codec `crates/osb-cli/src/wav.rs`) with `--mock`, `--config`, or automatic Python worker spawning, and wired `openspeechbridge run --config` to bridge PipeWire capture -> `Pipeline` -> `VirtualMicrophone`.
- **Validation (Headless + Real Offline PT-BR -> EN on AC Power)**:
  - Rust gates: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo doc --no-deps --workspace`, and `cargo test --workspace` passed: **152 active Rust tests passed** (148 unit/integration + 4 doctests, including cross-language Rust <-> Python UDS pipeline integration) and 15 PipeWire daemon tests remained ignored.
  - Python SDK gate: `python3 -m unittest discover -s engines/sdk-python/tests -v` passed (**4/4 tests**).
  - Real offline PT-BR -> EN execution on AC power (`tools/benchmark/corpus/short_01.wav`, `5.04 s` human PT-BR speech -> `48 kHz` stereo English WAV via supervised `faster-whisper-small` `int8` + `Kokoro-82M` `af_heart`, 4 threads):
    - Full-utterance (`--no-vad`): cold worker startup `5.47 s`, speech translation `1.60 s`, TTS synthesis `1.43 s`, **total warm pipeline turnaround `3.03 s` (`0.601x` RTF)** -> `"He set up a little campaign without wire, he said."` (`3.28 s` valid audio, `RMS 0.0468`, `peak 0.3236`).
    - VAD-segmented default (`2` clauses detected): speech stage `2.88 s`, TTS stage `2.01 s`, **total turnaround `4.89 s` (`0.970x` RTF)**.
- **Known Limitations & Next Recommended Tasks**:
  1. Separate uncommitted M1 PipeWire changes, M2 benchmark/runtime changes, and M3 translation changes into clean focused commits/PRs.
  2. Perform a live human voice-call test (`openspeechbridge run --config ...` with Discord/Meet selecting `"OpenSpeechBridge"`) and verify `af_heart` voice redistribution terms before shipping any pre-packaged model bundle.
  3. Proceed to **Milestone 4 (Full Duplex: incoming virtual sink + bi-directional language pipelines)** or **Milestone 5 (Streaming Optimization: sentence/clause streaming TTS to reduce time-to-first-audio below 1.5 s)**.

## M2 Engine Runtime (UDS IPC, Binary Framing & Worker Supervision) — 2026-11-07

- Implemented the Milestone 2 Engine Runtime across `osb-protocol`, `osb-daemon`, `osb-cli`, and `engines/mock`, documented in [`adr/0005-uds-framed-engine-ipc.md`](adr/0005-uds-framed-engine-ipc.md) and [`ENGINE_PROTOCOL.md`](ENGINE_PROTOCOL.md).
- **`OSBP` v1 Bounded Binary Wire Framing (`osb-protocol::framing`)**:
  - 16-byte little-endian frame prefix (`magic = "OSBP"`, `version = 1`, `FrameFlags`, `header_len`, `payload_len`) validated against `FrameLimits` prior to allocation.
  - Strips `Vec<f32>` audio samples out of JSON headers and transmits bit-exact IEEE-754 `f32le` PCM bytes in the binary payload (`FrameFlags::HAS_PCM_PAYLOAD`).
  - Added `Hello` / `Welcome` versioned handshake, `ProcessSpeechTranslation` / `SpeechTranslationResult` (`AudioFrameMeta`), `Capability::SpeechTranslation`, and distinct `weights_license` tracking on `EngineInfo`.
- **Bounded Multiplexed IPC & Persistent Worker Supervision (`osb-daemon`)**:
  - `EngineRpcClient` (`crates/osb-daemon/src/ipc.rs`): multiplexed `request_id` correlation, `Semaphore`-bounded `max_in_flight` backpressure (`try_send_request` / `send_request`), out-of-band `Ping`/`Pong` health checks, and automatic `EngineRequest::Cancel` emission on deadline timeout.
  - `ManagedWorker` (`crates/osb-daemon/src/worker.rs`): binds restricted UDS sockets (`0o700` dir, `0o600` socket) before spawning persistent child processes (`--socket <path>` + `OSB_ENGINE_SOCKET=<path>`), racing `UnixListener::accept()` against `Child::wait()`, plus `WorkerSpec::connect` attach mode.
  - `EngineSupervisor` (`crates/osb-daemon/src/engine_supervisor.rs`) & `Runtime`: capability/language negotiation, per-engine metrics (`requests_total`, `requests_failed`, `timeouts_total`, `restarts_total`, `last_ping_rtt_us`), crash detection, and automatic supervised restarts within `RestartPolicy::max_restarts`.
- **Standalone Persistent Mock Worker (`engines/mock`)**:
  - Added `engines/mock` (`osb-engine-mock`) to the Cargo workspace with binary target `osb-engine-mock` supporting `--socket` and `--listen`, deterministic `ProcessS2s` gain transform, `ProcessSpeechTranslation`, in-flight task cancellation, and configurable delay/crash fault injection.
- **Validation**:
  - `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo doc --no-deps --workspace`, and `cargo test --workspace` passed on Ubuntu 24.04 (`rustc 1.98.1`): **141 active tests passed** (137 unit/integration + 4 doctests) and 15 PipeWire daemon tests remained ignored.
  - Validated `cargo run --bin openspeechbridge -- engines` and `engines --json`.
- **Next Recommended Tasks**:
  1. Separate and commit pending M1 PipeWire virtual-mic fixes vs. M2 research/runtime changes cleanly.
  2. Repeat `faster-whisper small` + `Kokoro` benchmark on AC power and finalize voice license verification.
  3. Build the M3 Python UDS worker adapter (`faster-whisper` `translate` + TTS) speaking `OSBP` v1 frames and connect VAD + audio rings in `osb-daemon::pipeline`.

## M2 CPU inference benchmark — 2026-09-27

- Completed the isolated CPU-only comparison in
  [`CPU_INFERENCE_BENCHMARK.md`](CPU_INFERENCE_BENCHMARK.md). No PipeWire, VAD,
  daemon or production-provider integration was added.
- Human PT-BR evidence: 20 complete FLEURS validation utterances plus one
  deterministic 68.57 s composite. Corpus audio/caches remain gitignored;
  structured measurements and manual English references are reproducible.
- Provisional default: faster-whisper 1.2.1 / CTranslate2 4.8.2, multilingual
  `small`, CPU `int8`, four threads, one persistent worker. Warm medians were
  1.67 s for short and 1.93 s for medium clips; full-run median RTF 0.239 and
  peak RSS 870 MiB. `base` was faster but made materially worse translations.
- whisper.cpp v1.9.4 remained a viable native/low-memory alternative, but
  `small-q5_1` was materially slower in this run; `base-q5_1` quality was not
  sufficient for the default.
- Kokoro-82M/`af_heart` generated valid 24 kHz English WAVs. RTF varied from
  about 0.47 early to 0.94 median under sustained battery load; peak RSS reached
  2.03 GiB. Round-trip ASR preserved the benchmark texts, but no human listening
  test was performed.
- Offline PT-BR WAV -> English text -> English WAV: PASS. The selected sentence
  translated correctly. Under the late-session power state, warm translation +
  synthesis was 7.14 s and cold total was 14.61 s.
- Latency verdict: 1.5–3 s is **UNCERTAIN**, not demonstrated. The laptop was
  unplugged at 16% battery using `powersave`, and sustained performance fell by
  about 2x. Repeat on AC and test sentence-level first-audio before an SLA.
- Runtime implication: spawning per utterance is rejected; Whisper and Kokoro
  should remain warm in supervised local workers. A versioned Unix-domain-socket
  spike is recommended later, not implemented here.
- Licensing: Whisper/whisper.cpp/faster-whisper/CTranslate2 are MIT; Kokoro code
  and model repository are Apache-2.0. Separate `af_heart` redistribution terms
  were not found: **LICENSE REVIEW REQUIRED** before bundling that voice.
- M2 live integration is **not complete**. Next: controlled AC repeat,
  spontaneous private PT-BR corpus, human TTS listening and voice-license review.
- Final validation in the Docker development environment: `cargo fmt --check`,
  `cargo test --workspace`, and
  `cargo clippy --workspace --all-targets -- -D warnings` passed; 123 tests
  passed and 15 were ignored. `git diff --check`, Python byte-compilation and
  shell syntax validation also passed.

## Architecture research — 2026-09-27

- Completed the repository-wide discovery, build-vs-reuse and licensing review
  in [`ARCHITECTURE_RESEARCH.md`](ARCHITECTURE_RESEARCH.md). This is a proposal
  for review, not an accepted ADR or an implemented inference pipeline.
- Recommended baseline: capability-composed modular pipeline, using Whisper's
  direct speech-to-English translation plus generic Kokoro TTS for the first
  PT-BR -> English CPU-only benchmark. Keep end-to-end speech-to-speech behind
  an optional `SpeechToSpeechEngine` provider.
- Meta Seamless verdict: valuable S2S/streaming research provider, but not a
  default dependency because the inspected weights are non-commercial, the
  runtime/model footprint is large, and laptop-CPU real-time performance is
  not established by primary-source evidence.
- No model, runtime dependency or architecture implementation was added.
- Latest headless validation for this review, in the Docker development
  environment: `cargo fmt --check`, `cargo test --workspace`, and
  `cargo clippy --workspace --all-targets -- -D warnings` passed. Tests: 123
  passed and 15 ignored. This did not repeat physical PipeWire validation.
- Active decisions: benchmark `whisper.cpp` versus `faster-whisper`, verify the
  exact Kokoro voice artifacts, select the local IPC schema in an ADR, and name
  the CPU/conferencing application used as the MVP acceptance target.
- Next recommended task, only after architecture review approval: M2 external
  mock worker and reproducible benchmark harness; do not download large models
  or implement the proposed stack before that decision.

## CI repair audit — 2026-09-24

- PR #11: corrected five CI toolchain references and the release reference to
  `dtolnay/rust-toolchain@stable`; derived audio enum defaults without changing
  F32/Stereo defaults; removed an unused capture-state mirror and its own test.
- GitHub Actions run [36080014937](https://github.com/felipemacedo1/open-speech-bridge/actions/runs/36080014937)
  passed all six jobs on commit `7a5b134`: Check, Test, Clippy, Format,
  Documentation and Security Audit.
- `cargo test --workspace` in that run: 102 unit tests and 4 doctests passed;
  11 unit tests and 4 doctests were ignored. Ignored tests are not audio evidence.
- Local Docker (Debian 12, Rust 1.87.0): `cargo build`, `cargo fmt --check`,
  `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`
  passed. Local tests reproduced 102 unit tests + 4 doctests passing and 15 ignored.
- CLI `doctor`, `devices`, and `loopback --help` executed in Docker. No PipeWire
  daemon or WirePlumber is available there; device listing was empty. This does
  not validate real microphone capture or audio routing.
- SonarQube Cloud project `felipemacedo1_open-speech-bridge` in organization
  `felipemacedo1` is confirmed. The new workflow imports Clippy JSON and waits
  for the Quality Gate. `SONAR_TOKEN` is registered as an Actions secret.
  Authenticated run 36082523834 completed analysis at `f27f55b`; the gate
  failed on new duplicated lines (30.5%, limit 3%). Three critical complexity
  code smells remain in capture.rs and virtual_device.rs. See SONARQUBE.md.
- Next: refactor the Sonar duplication/complexity findings, pass the Quality
  Gate, and validate real PipeWire audio on Linux
  before merge. The main branch and Dependabot PRs do not yet contain this fix.
- Release workflow reference was corrected but no release/tag was triggered.

## M1 PipeWire virtual-microphone signal fix - 2026-09-25

**Branch:** `fix/m1-pipewire-buffer-sync`

### Root Cause
- The virtual microphone consumed samples from its ring buffer while connected
  as a PipeWire `Direction::Input` stream. The callback therefore updated local
  counters and wrote a dequeued input buffer, but did not publish those samples
  as source output. `VM consumed` measured the ring read, not valid audio at the
  consumer.
- Once corrected to an output stream, two independent issues remained visible:
  `Audio/Source/Virtual` was not linked as a normal capture device by the active
  WirePlumber policy, and the capture stream could be automatically retargeted
  to the new virtual source instead of the requested physical device.
- The output callback also rendered all of `spa_data.maxsize` (131072 bytes)
  rather than the requested quantum (1024 frames / 8192 bytes for stereo F32),
  which produced incorrect timing and excess silence.

### Fixes Applied
- Virtual microphone now publishes a canonical `Audio/Source` node through a
  `Direction::Output` stream with `node.virtual=true`.
- Capture uses `target.object=<device>` and disables policy reconnection/target
  restoration, keeping device 52 as the physical source.
- Output size is `min(spa_data.maxsize, pw_buffer.requested * frame_stride)`.
  The callback writes interleaved F32LE from offset zero and publishes matching
  `spa_chunk.offset`, byte `size`, and frame `stride`.
- Loopback drains capture but only feeds the virtual ring while callback
  activity is present.
- Aggregate A-D signal diagnostics report RMS, peak and nonzero samples for the
  capture callback, capture ring input, virtual ring output, and before/after
  SPA copy. They are enabled only by `loopback --verbose`; callbacks contain no
  logging, allocation, blocking, or I/O.

### Regression Coverage
- Added virtual-microphone tests that fail if nonzero mono or stereo ring input
  becomes zero after F32LE SPA-buffer serialization.
- Added assertions for output-stream direction and requested-frame limiting.
- `cargo test --workspace`: 123 active tests passed and 15 were ignored (the
  ignored tests require a PipeWire daemon).

### Validation Status
- Physical PipeWire 1.0.5/WirePlumber validation completed with device 52,
  release build, 30-second verbose loopback, and a simultaneous 10-second
  `pw-record` from `openspeechbridge-virtual-mic`.
- Final OSB measurements: capture callbacks 1407, virtual callbacks 1407,
  virtual frames consumed 1367552. Signal stayed nonzero from A through D;
  D after copy reported RMS 0.000357, peak 0.004771, nonzero 2734531.
- Recorded WAV: 960512 samples, RMS 0.00000001, peak 0.00000010, nonzero
  911097. The lower downstream level matches the PipeWire source volume set to
  0.03 for both the physical and virtual sources; it is applied after the SPA
  buffer and does not represent zeroing in OSB.
- SPA geometry at the end of the run: initial chunk offset 0, size 8192,
  maxsize 131072, requested 1024 frames, published size 8192 bytes, stride 8.

### Latest Validated Commands

```bash
docker compose exec dev cargo fmt
docker compose exec dev cargo clippy --workspace --all-targets -- -D warnings
docker compose exec dev cargo test --workspace
cargo build --release --workspace
target/release/openspeechbridge loopback --input 52 --duration 30 --verbose
timeout 10s pw-record --media-category Capture --media-role Communication \
  --target openspeechbridge-virtual-mic --rate 48000 --channels 2 \
  --format f32 /tmp/osb-virtual-final.wav
```

### Known Limitations and Next Work
- PipeWire/WirePlumber integration tests remain ignored in environments without
  a user-session daemon; the physical run above covers the corrected path.
- The active system source volume was 0.03, so the consumer WAV is strongly
  attenuated. Repeat with an explicitly chosen source volume if an amplitude
  comparison across the session-manager mixer is required.
- No merge was performed. CI and SonarQube should be checked after a future
  push; the previously reported Sonar duplication findings are independent of
  this physical signal fix.

The bootstrap inventory below is historical and has not been revalidated.

## Current State: Foundation Complete

The project infrastructure is in place. Core Rust crates are defined with types, errors, and basic implementations. Docker development environment is configured.

## What Works Today

### ✅ Build System
- Rust workspace with 6 crates compiles (on Linux with PipeWire)
- Docker development container configured
- Proxy support for corporate environments

### ✅ Core Types (osb-core)
- Audio format types (SampleRate, SampleFormat, ChannelLayout, AudioFormat)
- Error types with severity levels
- Configuration structures
- Metrics definitions

### ✅ Audio Buffers (osb-audio)
- Lock-free SPSC ring buffer (AudioRingBuffer)
- Sample format conversion (i16↔f32, mono↔stereo)
- Resampling wrapper (via rubato)
- Unit tests pass

### ✅ Engine Protocol (osb-protocol)
- Capability definitions
- Engine info and state types
- Request/Response message types
- JSON serialization

### ✅ CLI Framework (osb-cli)
- Command structure with clap
- `doctor` command (system diagnostics)
- `devices` command (list audio devices)
- `status` command (show runtime status)
- `engines` command (list engines)
- JSON output support

### ✅ Documentation
- README with architecture overview
- AGENTS.md for coding agents
- CONTRIBUTING.md
- Architecture documentation
- ADR structure

## What's Partially Working

### 🔨 PipeWire Integration (osb-pipewire)
- Module structure defined
- Types for devices, streams, virtual devices
- Stub implementations for non-Linux
- **NOT YET**: Actual PipeWire stream creation
- **NOT YET**: Real device enumeration
- **NOT YET**: Virtual microphone creation

### 🔨 Daemon (osb-daemon)
- Runtime state machine defined
- Audio manager structure
- Engine supervisor structure
- Pipeline structure
- **NOT YET**: Actual audio processing
- **NOT YET**: Engine process management

## What's Not Implemented Yet

### 📋 Audio Pipeline
- [ ] Real PipeWire capture stream
- [ ] Real PipeWire playback stream
- [ ] Virtual microphone device
- [ ] Virtual sink device
- [ ] Audio routing

### ✅ Engine System (M2)
- [x] Engine process spawning (`ManagedWorker` over restricted UDS)
- [x] IPC with engines (`OSBP` v1 binary header + JSON control + `f32le` PCM payload)
- [x] Mock engine implementation (`osb-engine-mock` library and UDS worker binary)
- [x] Health monitoring, bounded backpressure, timeouts, and supervised crash restart

### 📋 ML Integration
- [ ] VAD (voice activity detection)
- [ ] STT engine adapter
- [ ] Translation engine adapter
- [ ] TTS engine adapter

### 📋 Features
- [ ] Configuration file loading
- [ ] Metrics export
- [ ] Graceful shutdown
- [ ] Hot-reload configuration

## Validated Commands

These commands are known to work:

```bash
# In Docker container (Linux)
cargo build                              # Builds all crates
cargo test                               # Runs all tests
cargo run --bin openspeechbridge -- --help  # Shows CLI help

# On Windows (limited - won't compile PipeWire)
# Use Docker for full builds
```

## Known Limitations

1. **Linux Only**: PipeWire integration requires Linux
2. **No Real Audio Yet**: PipeWire streams are stubs
3. **No ML Engines**: Mock engine not fully implemented
4. **Docker Required on Windows**: Native Windows build won't include PipeWire

## Active Architectural Questions

1. **IPC Mechanism**: Unix sockets vs shared memory for engine communication?
2. **Config Format**: TOML vs YAML for configuration files?
3. **Metrics Export**: Prometheus vs OpenTelemetry?

## Next Recommended Tasks

1. **Complete PipeWire device enumeration**
   - Use pipewire-rs registry listener
   - Parse device properties correctly
   - Handle device hot-plug

2. **Implement capture stream**
   - Create PipeWire stream
   - Connect to device
   - Push samples to ring buffer

3. **Implement virtual microphone**
   - Create PipeWire source node
   - Make it visible to applications
   - Serve audio from ring buffer

4. **Create mock engine**
   - Implement protocol messages
   - Echo or transform audio
   - Test engine supervisor

5. **Add integration tests**
   - Synthetic audio tests
   - Protocol message tests
   - Buffer behavior tests

## Environment Requirements

### Linux Development (Native or Docker)
- Ubuntu 22.04+ or Fedora 38+
- PipeWire 0.3.x
- Rust 1.75+

### Docker Development
- Docker Desktop with Linux containers
- 8GB RAM recommended
- Proxy configuration if behind corporate firewall

## How to Continue Development

1. Start Docker:
   ```powershell
   .\scripts\dev.ps1 build
   .\scripts\dev.ps1 shell
   ```

2. Inside container:
   ```bash
   cargo build
   cargo test
   cargo run --bin openspeechbridge -- doctor
   ```

3. Check this file for next tasks

4. Update this file when completing work

---

*This file should be updated whenever significant changes are made to the project.*
