//! Capability-composed audio translation pipeline.
//!
//! Bridges non-blocking SPSC audio ring buffers (or offline PCM streams) to
//! supervised UDS engines via resampling and bounded VAD endpoint segmentation.
//! Never executes inside PipeWire real-time callbacks and never logs raw
//! transcripts or audio samples.

use crate::engine_supervisor::EngineSupervisor;
use osb_audio::{
    resample_mono_buffer, AudioRingConsumer, AudioRingProducer, SampleConverter,
    StreamingMonoResampler, UtteranceSegment, VadConfig, VadSegmenter,
};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleRate};
use osb_core::config::EngineConfig;
use osb_core::error::{EngineError, Error, Result};
use osb_protocol::capability::Capability;
use osb_protocol::engine::EngineId;
use osb_protocol::framing::FrameFlags;
use osb_protocol::message::{AudioFrameMeta, EngineRequest, EngineResponse, ProcessingResult};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, info, warn};

/// Pipeline stage identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineStage {
    /// Capture from microphone
    Capture,
    /// Resample for engine input
    ResampleIn,
    /// Voice activity detection
    Vad,
    /// Speech-to-text or direct speech translation
    Stt,
    /// Text translation
    Translation,
    /// Text-to-speech synthesis
    Tts,
    /// Resample for output
    ResampleOut,
    /// Output to virtual device
    Output,
}

/// Negotiated engine execution topology for an outgoing or incoming pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineRoute {
    /// Architecture A MVP: Direct speech-to-target-text (`Whisper translate`) + `TTS`.
    DirectSpeechTranslation {
        /// Engine handling `ProcessSpeechTranslation`
        speech_translation_engine: EngineId,
        /// Engine handling `ProcessTts`
        tts_engine: EngineId,
    },
    /// Three-stage modular cascade: `STT` -> `MT` -> `TTS`.
    CascadedSttMtTts {
        /// Engine handling `ProcessStt`
        stt_engine: EngineId,
        /// Engine handling `ProcessTranslation`
        mt_engine: EngineId,
        /// Engine handling `ProcessTts`
        tts_engine: EngineId,
    },
    /// Architecture B optional end-to-end `SpeechToSpeech` engine.
    EndToEndS2s {
        /// Engine handling `ProcessS2s`
        s2s_engine: EngineId,
    },
}

/// Pipeline configuration.
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Enable outgoing translation (mic → translated virtual mic)
    pub enable_outgoing: bool,
    /// Enable incoming translation (app audio → translated headphones)
    pub enable_incoming: bool,
    /// Input audio format (e.g., 48 kHz stereo)
    pub input_format: AudioFormat,
    /// Output audio format (e.g., 48 kHz stereo)
    pub output_format: AudioFormat,
    /// Engine input format (typically 16 kHz mono)
    pub engine_format: AudioFormat,
    /// Source spoken language code (e.g., `"pt-BR"`)
    pub source_language: String,
    /// Target spoken language code (e.g., `"en-US"`)
    pub target_language: String,
    /// Optional TTS voice identifier
    pub voice_id: Option<String>,
    /// VAD endpointing parameters
    pub vad: VadConfig,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            enable_outgoing: true,
            enable_incoming: false,
            input_format: AudioFormat::DESKTOP_STANDARD,
            output_format: AudioFormat::DESKTOP_STANDARD,
            engine_format: AudioFormat::SPEECH_RECOGNITION,
            source_language: "pt-BR".to_string(),
            target_language: "en-US".to_string(),
            voice_id: None,
            vad: VadConfig::default(),
        }
    }
}

/// Completed translated utterance formatted to `PipelineConfig::output_format`.
#[derive(Debug, Clone)]
pub struct TranslatedUtterance {
    /// Source VAD utterance ID
    pub utterance_id: u64,
    /// Interleaved `f32` PCM samples matching `output_format`
    pub output_samples: Vec<f32>,
    /// Output audio format
    pub output_format: AudioFormat,
    /// In-memory translated text (never persisted or logged by default)
    pub translated_text: Option<String>,
    /// STT / direct speech-translation stage wall time in ms
    pub speech_stage_ms: u64,
    /// Optional text-translation stage wall time in ms
    pub mt_stage_ms: u64,
    /// TTS stage wall time in ms
    pub tts_stage_ms: u64,
    /// End-to-end utterance processing wall time in ms
    pub total_ms: u64,
}

/// Observable counters and stage latencies for [`Pipeline`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineMetricsSnapshot {
    /// Utterances translated and synthesized successfully
    pub utterances_processed: u64,
    /// Utterances skipped because the engine produced empty text
    pub utterances_empty_skipped: u64,
    /// Utterances that failed due to engine error or timeout
    pub utterances_failed: u64,
    /// Output audio frames pushed to consumer ring
    pub output_frames_emitted: u64,
    /// Output audio frames dropped due to output ring overflow
    pub output_frames_dropped: u64,
    /// Most recent speech recognition / speech translation latency in ms
    pub last_speech_stage_ms: u64,
    /// Most recent text translation latency in ms
    pub last_mt_stage_ms: u64,
    /// Most recent TTS synthesis latency in ms
    pub last_tts_stage_ms: u64,
    /// Most recent total utterance turnaround latency in ms
    pub last_total_ms: u64,
}

/// Stateful non-RT audio translation pipeline.
pub struct Pipeline {
    config: PipelineConfig,
    is_running: bool,
    session_id: u64,
    next_request_id: AtomicU64,
    input_resampler: Option<StreamingMonoResampler>,
    vad: VadSegmenter,
    metrics: PipelineMetricsSnapshot,
}

impl Pipeline {
    /// Create a new pipeline with the given configuration.
    pub fn new(config: PipelineConfig) -> Self {
        let mut vad_cfg = config.vad;
        vad_cfg.sample_rate = config.engine_format.sample_rate.hz();

        Self {
            input_resampler: None,
            vad: VadSegmenter::new(vad_cfg),
            config,
            is_running: false,
            session_id: 1,
            next_request_id: AtomicU64::new(1),
            metrics: PipelineMetricsSnapshot::default(),
        }
    }

    /// Pipeline configuration.
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    /// Snapshot of pipeline counters and latencies.
    pub fn metrics(&self) -> PipelineMetricsSnapshot {
        self.metrics
    }

    /// Check if the pipeline is marked running.
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Mark the pipeline as started and initialize input resampling state.
    pub fn start(&mut self) -> Result<()> {
        if self.is_running {
            return Ok(());
        }
        info!(
            source = %self.config.source_language,
            target = %self.config.target_language,
            "starting processing pipeline"
        );
        self.ensure_input_resampler()?;
        self.is_running = true;
        Ok(())
    }

    /// Stop the pipeline.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }
        info!("stopping processing pipeline");
        self.is_running = false;
        Ok(())
    }

    /// Select an execution [`PipelineRoute`] from `supervisor` and optional `EngineConfig`.
    pub fn negotiate_route(
        &self,
        supervisor: &EngineSupervisor,
        engine_cfg: Option<&EngineConfig>,
    ) -> Result<PipelineRoute> {
        if let Some(cfg) = engine_cfg {
            if let Some(explicit) = resolve_explicit_route(cfg) {
                return Ok(explicit);
            }
        }

        let src = Some(self.config.source_language.as_str());
        let dst = Some(self.config.target_language.as_str());

        if let (Ok(st_id), Ok(tts_id)) = (
            supervisor.negotiate_capability(&[Capability::SpeechTranslation], src, dst),
            supervisor.negotiate_capability(&[Capability::Tts], None, dst),
        ) {
            return Ok(PipelineRoute::DirectSpeechTranslation {
                speech_translation_engine: st_id,
                tts_engine: tts_id,
            });
        }

        if let (Ok(stt_id), Ok(mt_id), Ok(tts_id)) = (
            supervisor.negotiate_capability(&[Capability::Stt], src, None),
            supervisor.negotiate_capability(&[Capability::TextTranslation], src, dst),
            supervisor.negotiate_capability(&[Capability::Tts], None, dst),
        ) {
            return Ok(PipelineRoute::CascadedSttMtTts {
                stt_engine: stt_id,
                mt_engine: mt_id,
                tts_engine: tts_id,
            });
        }

        let s2s_id = supervisor.negotiate_capability(&[Capability::SpeechToSpeech], src, dst)?;
        Ok(PipelineRoute::EndToEndS2s { s2s_engine: s2s_id })
    }

    /// Convert an interleaved input chunk to `16 kHz` mono and segment utterances via VAD.
    pub fn ingest_interleaved(
        &mut self,
        interleaved_samples: &[f32],
    ) -> Result<Vec<UtteranceSegment>> {
        if interleaved_samples.is_empty() {
            return Ok(Vec::new());
        }

        let mono = to_mono_samples(interleaved_samples, self.config.input_format.channels);
        self.ensure_input_resampler()?;
        let resampled = self
            .input_resampler
            .as_mut()
            .expect("initialized above")
            .push(&mono)?;

        Ok(self.vad.push_samples(&resampled))
    }

    /// Flush buffered resampler and VAD state at end-of-stream.
    pub fn flush_input(&mut self) -> Result<Vec<UtteranceSegment>> {
        let mut segments = Vec::new();
        if let Some(ref mut resampler) = self.input_resampler {
            let tail = resampler.flush()?;
            if !tail.is_empty() {
                segments.extend(self.vad.push_samples(&tail));
            }
        }
        if let Some(last) = self.vad.flush() {
            segments.push(last);
        }
        Ok(segments)
    }

    /// Execute `route` for one segmented utterance and adapt the synthesized audio to `output_format`.
    pub async fn process_utterance(
        &mut self,
        supervisor: &mut EngineSupervisor,
        route: &PipelineRoute,
        segment: &UtteranceSegment,
    ) -> Result<Option<TranslatedUtterance>> {
        let started = Instant::now();
        let outcome = match route {
            PipelineRoute::DirectSpeechTranslation {
                speech_translation_engine,
                tts_engine,
            } => {
                self.execute_direct_speech_translation(
                    supervisor,
                    speech_translation_engine,
                    tts_engine,
                    segment,
                )
                .await
            }
            PipelineRoute::CascadedSttMtTts {
                stt_engine,
                mt_engine,
                tts_engine,
            } => {
                self.execute_cascaded_stt_mt_tts(
                    supervisor, stt_engine, mt_engine, tts_engine, segment,
                )
                .await
            }
            PipelineRoute::EndToEndS2s { s2s_engine } => {
                self.execute_end_to_end_s2s(supervisor, s2s_engine, segment)
                    .await
            }
        };

        let Some(raw) = (match outcome {
            Ok(opt) => opt,
            Err(err) => {
                self.metrics.utterances_failed += 1;
                return Err(err);
            }
        }) else {
            self.metrics.utterances_empty_skipped += 1;
            return Ok(None);
        };

        let output_samples = format_output_audio(
            &raw.mono_samples,
            SampleRate::new(raw.sample_rate),
            self.config.output_format,
        )?;
        let total_ms = started.elapsed().as_millis() as u64;

        self.metrics.utterances_processed += 1;
        self.metrics.last_speech_stage_ms = raw.speech_ms;
        self.metrics.last_mt_stage_ms = raw.mt_ms;
        self.metrics.last_tts_stage_ms = raw.tts_ms;
        self.metrics.last_total_ms = total_ms;

        debug!(
            utterance_id = segment.utterance_id,
            input_ms = segment.duration_ms(),
            speech_ms = raw.speech_ms,
            mt_ms = raw.mt_ms,
            tts_ms = raw.tts_ms,
            total_ms,
            "completed utterance translation"
        );

        Ok(Some(TranslatedUtterance {
            utterance_id: segment.utterance_id,
            output_samples,
            output_format: self.config.output_format,
            translated_text: raw.translated_text,
            speech_stage_ms: raw.speech_ms,
            mt_stage_ms: raw.mt_ms,
            tts_stage_ms: raw.tts_ms,
            total_ms,
        }))
    }

    /// Run a continuous non-RT bridge draining `capture_rx` and writing translated audio to `output_tx`.
    pub async fn run_ring_bridge(
        &mut self,
        supervisor: Arc<Mutex<EngineSupervisor>>,
        mut capture_rx: AudioRingConsumer,
        mut output_tx: AudioRingProducer,
        mut shutdown_rx: broadcast::Receiver<()>,
    ) -> Result<()> {
        self.start()?;
        let route = {
            let sup = supervisor.lock().await;
            self.negotiate_route(&sup, None)?
        };

        let in_channels = self.config.input_format.channels.channels() as usize;
        let chunk_samples = (self.config.input_format.sample_rate.samples_for_ms(20) as usize)
            .max(256)
            * in_channels;
        let mut read_buf = vec![0.0f32; chunk_samples];

        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => break,
                _ = tokio::time::sleep(Duration::from_millis(10)) => {
                    loop {
                        let popped = capture_rx.pop(&mut read_buf);
                        if popped == 0 {
                            break;
                        }
                        let segments = self.ingest_interleaved(&read_buf[..popped])?;
                        for seg in segments {
                            let res = {
                                let mut sup = supervisor.lock().await;
                                self.process_utterance(&mut sup, &route, &seg).await
                            };
                            match res {
                                Ok(Some(translated)) => {
                                    self.push_to_output_ring(&mut output_tx, &translated.output_samples).await;
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    warn!(utterance_id = seg.utterance_id, error = %err, "utterance translation failed; continuing stream");
                                }
                            }
                        }
                    }
                }
            }
        }

        self.stop()?;
        Ok(())
    }

    async fn push_to_output_ring(&mut self, output_tx: &mut AudioRingProducer, samples: &[f32]) {
        let out_ch = self.config.output_format.channels.channels() as usize;
        let step = (self.config.output_format.sample_rate.samples_for_ms(20) as usize * out_ch)
            .max(out_ch);
        let mut offset = 0usize;

        while offset < samples.len() {
            let end = (offset + step).min(samples.len());
            let slice = &samples[offset..end];
            let written = output_tx.push(slice);
            self.metrics.output_frames_emitted += (written / out_ch) as u64;

            if written < slice.len() {
                // Brief non-RT yield to allow PipeWire output consumer to drain before dropping tail.
                tokio::time::sleep(Duration::from_millis(15)).await;
                let retry = output_tx.push(&slice[written..]);
                self.metrics.output_frames_emitted += (retry / out_ch) as u64;
                let dropped_samples = slice.len() - written - retry;
                self.metrics.output_frames_dropped += (dropped_samples / out_ch) as u64;
            }
            offset = end;
        }
    }

    fn ensure_input_resampler(&mut self) -> Result<()> {
        if self.input_resampler.is_none() {
            self.input_resampler = Some(StreamingMonoResampler::new(
                self.config.input_format.sample_rate,
                self.config.engine_format.sample_rate,
                1024,
            )?);
        }
        Ok(())
    }

    fn next_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn execute_direct_speech_translation(
        &self,
        supervisor: &mut EngineSupervisor,
        st_engine: &EngineId,
        tts_engine: &EngineId,
        segment: &UtteranceSegment,
    ) -> Result<Option<RawSynthesis>> {
        let st_started = Instant::now();
        let req_id = self.next_id();
        let flags = FrameFlags::SPEECH_START
            .union(FrameFlags::SPEECH_END)
            .union(FrameFlags::END_OF_UTTERANCE);

        let resp = supervisor
            .send_request_with_flags(
                st_engine,
                EngineRequest::ProcessSpeechTranslation {
                    request_id: req_id,
                    audio: segment.samples.clone(),
                    sample_rate: segment.sample_rate,
                    source_lang: self.config.source_language.clone(),
                    target_lang: self.config.target_language.clone(),
                    is_final: true,
                    meta: Some(AudioFrameMeta {
                        session_id: self.session_id,
                        utterance_id: segment.utterance_id,
                        chunk_id: 0,
                        timestamp_us: segment.start_timestamp_us,
                        channels: 1,
                    }),
                },
                flags,
                false,
            )
            .await?;
        let speech_ms = st_started.elapsed().as_millis() as u64;

        let text = extract_speech_translation_text(st_engine, resp)?;
        if text.trim().is_empty() {
            return Ok(None);
        }

        let (mono_samples, sample_rate, tts_ms) =
            self.synthesize_tts(supervisor, tts_engine, &text).await?;

        Ok(Some(RawSynthesis {
            mono_samples,
            sample_rate,
            translated_text: Some(text),
            speech_ms,
            mt_ms: 0,
            tts_ms,
        }))
    }

    async fn execute_cascaded_stt_mt_tts(
        &self,
        supervisor: &mut EngineSupervisor,
        stt_engine: &EngineId,
        mt_engine: &EngineId,
        tts_engine: &EngineId,
        segment: &UtteranceSegment,
    ) -> Result<Option<RawSynthesis>> {
        let stt_started = Instant::now();
        let stt_resp = supervisor
            .send_request_with_flags(
                stt_engine,
                EngineRequest::ProcessStt {
                    request_id: self.next_id(),
                    audio: segment.samples.clone(),
                    sample_rate: segment.sample_rate,
                    is_final: true,
                },
                FrameFlags::END_OF_UTTERANCE,
                false,
            )
            .await?;
        let speech_ms = stt_started.elapsed().as_millis() as u64;

        let transcript = extract_stt_text(stt_engine, stt_resp)?;
        if transcript.trim().is_empty() {
            return Ok(None);
        }

        let mt_started = Instant::now();
        let mt_resp = supervisor
            .send_request(
                mt_engine,
                EngineRequest::ProcessTranslation {
                    request_id: self.next_id(),
                    text: transcript,
                    source_lang: self.config.source_language.clone(),
                    target_lang: self.config.target_language.clone(),
                },
            )
            .await?;
        let mt_ms = mt_started.elapsed().as_millis() as u64;

        let translated = extract_mt_text(mt_engine, mt_resp)?;
        if translated.trim().is_empty() {
            return Ok(None);
        }

        let (mono_samples, sample_rate, tts_ms) = self
            .synthesize_tts(supervisor, tts_engine, &translated)
            .await?;

        Ok(Some(RawSynthesis {
            mono_samples,
            sample_rate,
            translated_text: Some(translated),
            speech_ms,
            mt_ms,
            tts_ms,
        }))
    }

    async fn execute_end_to_end_s2s(
        &self,
        supervisor: &mut EngineSupervisor,
        s2s_engine: &EngineId,
        segment: &UtteranceSegment,
    ) -> Result<Option<RawSynthesis>> {
        let s2s_started = Instant::now();
        let resp = supervisor
            .send_request_with_flags(
                s2s_engine,
                EngineRequest::ProcessS2s {
                    request_id: self.next_id(),
                    audio: segment.samples.clone(),
                    sample_rate: segment.sample_rate,
                    source_lang: self.config.source_language.clone(),
                    target_lang: self.config.target_language.clone(),
                    is_final: true,
                },
                FrameFlags::END_OF_UTTERANCE,
                false,
            )
            .await?;
        let total = s2s_started.elapsed().as_millis() as u64;

        match resp {
            EngineResponse::S2sResult { result, .. } => match result {
                ProcessingResult::Success { data, .. } | ProcessingResult::Partial { data, .. } => {
                    if data.audio.is_empty() {
                        return Ok(None);
                    }
                    Ok(Some(RawSynthesis {
                        mono_samples: data.audio,
                        sample_rate: data.sample_rate,
                        translated_text: data.translation,
                        speech_ms: total,
                        mt_ms: 0,
                        tts_ms: 0,
                    }))
                }
                ProcessingResult::Failed { code, message } => Err(Error::Engine(
                    EngineError::protocol(format!("{s2s_engine} S2S failed ({code}): {message}")),
                )),
            },
            EngineResponse::Error { code, message, .. } => Err(Error::Engine(
                EngineError::protocol(format!("{s2s_engine} error ({code}): {message}")),
            )),
            other => Err(Error::Engine(EngineError::protocol(format!(
                "unexpected S2S response: {other:?}"
            )))),
        }
    }

    async fn synthesize_tts(
        &self,
        supervisor: &mut EngineSupervisor,
        tts_engine: &EngineId,
        text: &str,
    ) -> Result<(Vec<f32>, u32, u64)> {
        let tts_started = Instant::now();
        let resp = supervisor
            .send_request(
                tts_engine,
                EngineRequest::ProcessTts {
                    request_id: self.next_id(),
                    text: text.to_string(),
                    language: self.config.target_language.clone(),
                    voice_id: self.config.voice_id.clone(),
                },
            )
            .await?;
        let tts_ms = tts_started.elapsed().as_millis() as u64;

        match resp {
            EngineResponse::TtsResult { result, .. } => match result {
                ProcessingResult::Success { data, .. } | ProcessingResult::Partial { data, .. } => {
                    Ok((data.audio, data.sample_rate, tts_ms))
                }
                ProcessingResult::Failed { code, message } => Err(Error::Engine(
                    EngineError::protocol(format!("{tts_engine} TTS failed ({code}): {message}")),
                )),
            },
            EngineResponse::Error { code, message, .. } => Err(Error::Engine(
                EngineError::protocol(format!("{tts_engine} error ({code}): {message}")),
            )),
            other => Err(Error::Engine(EngineError::protocol(format!(
                "unexpected TTS response: {other:?}"
            )))),
        }
    }
}

impl Default for Pipeline {
    fn default() -> Self {
        Self::new(PipelineConfig::default())
    }
}

struct RawSynthesis {
    mono_samples: Vec<f32>,
    sample_rate: u32,
    translated_text: Option<String>,
    speech_ms: u64,
    mt_ms: u64,
    tts_ms: u64,
}

fn resolve_explicit_route(cfg: &EngineConfig) -> Option<PipelineRoute> {
    if let Some(ref s2s) = cfg.s2s_engine {
        return Some(PipelineRoute::EndToEndS2s {
            s2s_engine: EngineId::new(s2s),
        });
    }
    if let (Some(ref st), Some(ref tts)) = (&cfg.speech_translation_engine, &cfg.tts_engine) {
        return Some(PipelineRoute::DirectSpeechTranslation {
            speech_translation_engine: EngineId::new(st),
            tts_engine: EngineId::new(tts),
        });
    }
    if let (Some(ref stt), Some(ref mt), Some(ref tts)) =
        (&cfg.stt_engine, &cfg.translation_engine, &cfg.tts_engine)
    {
        return Some(PipelineRoute::CascadedSttMtTts {
            stt_engine: EngineId::new(stt),
            mt_engine: EngineId::new(mt),
            tts_engine: EngineId::new(tts),
        });
    }
    None
}

fn to_mono_samples(samples: &[f32], layout: ChannelLayout) -> Vec<f32> {
    match layout {
        ChannelLayout::Mono => samples.to_vec(),
        ChannelLayout::Stereo => {
            let mut mono = vec![0.0f32; samples.len() / 2];
            SampleConverter::stereo_to_mono(samples, &mut mono);
            mono
        }
    }
}

fn format_output_audio(
    mono_samples: &[f32],
    src_rate: SampleRate,
    target_format: AudioFormat,
) -> Result<Vec<f32>> {
    let resampled = resample_mono_buffer(mono_samples, src_rate, target_format.sample_rate)?;
    match target_format.channels {
        ChannelLayout::Mono => Ok(resampled),
        ChannelLayout::Stereo => {
            let mut stereo = vec![0.0f32; resampled.len() * 2];
            SampleConverter::mono_to_stereo(&resampled, &mut stereo);
            Ok(stereo)
        }
    }
}

fn extract_speech_translation_text(engine_id: &EngineId, resp: EngineResponse) -> Result<String> {
    match resp {
        EngineResponse::SpeechTranslationResult { result, .. } => match result {
            ProcessingResult::Success { data, .. } | ProcessingResult::Partial { data, .. } => {
                Ok(data.text)
            }
            ProcessingResult::Failed { code, message } => {
                Err(Error::Engine(EngineError::protocol(format!(
                    "{engine_id} speech translation failed ({code}): {message}"
                ))))
            }
        },
        EngineResponse::Error { code, message, .. } => Err(Error::Engine(EngineError::protocol(
            format!("{engine_id} error ({code}): {message}"),
        ))),
        other => Err(Error::Engine(EngineError::protocol(format!(
            "unexpected speech translation response: {other:?}"
        )))),
    }
}

fn extract_stt_text(engine_id: &EngineId, resp: EngineResponse) -> Result<String> {
    match resp {
        EngineResponse::SttResult { result, .. } => match result {
            ProcessingResult::Success { data, .. } | ProcessingResult::Partial { data, .. } => {
                Ok(data.text)
            }
            ProcessingResult::Failed { code, message } => Err(Error::Engine(
                EngineError::protocol(format!("{engine_id} STT failed ({code}): {message}")),
            )),
        },
        EngineResponse::Error { code, message, .. } => Err(Error::Engine(EngineError::protocol(
            format!("{engine_id} error ({code}): {message}"),
        ))),
        other => Err(Error::Engine(EngineError::protocol(format!(
            "unexpected STT response: {other:?}"
        )))),
    }
}

fn extract_mt_text(engine_id: &EngineId, resp: EngineResponse) -> Result<String> {
    match resp {
        EngineResponse::TranslationResult { result, .. } => match result {
            ProcessingResult::Success { data, .. } | ProcessingResult::Partial { data, .. } => {
                Ok(data.text)
            }
            ProcessingResult::Failed { code, message } => Err(Error::Engine(
                EngineError::protocol(format!("{engine_id} MT failed ({code}): {message}")),
            )),
        },
        EngineResponse::Error { code, message, .. } => Err(Error::Engine(EngineError::protocol(
            format!("{engine_id} error ({code}): {message}"),
        ))),
        other => Err(Error::Engine(EngineError::protocol(format!(
            "unexpected MT response: {other:?}"
        )))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_supervisor::EngineHandler;
    use osb_audio::AudioRingBuffer;
    use osb_engine_mock::MockEngine;

    struct MockAdapter(MockEngine);
    impl EngineHandler for MockAdapter {
        fn handle(&self, request: EngineRequest) -> EngineResponse {
            self.0.handle_request(request)
        }
    }

    fn make_48k_stereo_speech_with_silence() -> Vec<f32> {
        let mut pcm = vec![0.0f32; 4_800 * 2];
        // 350 ms 440 Hz tone @ 48 kHz stereo
        for i in 0..16_800 {
            let s = ((i as f32 / 48_000.0) * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.35;
            pcm.push(s);
            pcm.push(s);
        }
        // 450 ms trailing silence @ 48 kHz stereo to trigger VAD endpoint
        pcm.resize(pcm.len() + 21_600 * 2, 0.0f32);
        pcm
    }

    #[test]
    fn test_pipeline_lifecycle() {
        let mut pipeline = Pipeline::default();
        assert!(!pipeline.is_running());

        pipeline.start().unwrap();
        assert!(pipeline.is_running());

        pipeline.stop().unwrap();
        assert!(!pipeline.is_running());
    }

    #[tokio::test]
    async fn test_pipeline_direct_speech_translation_and_ring_bridge() {
        let mut supervisor = EngineSupervisor::new();
        let mock = MockEngine::new();
        supervisor.register_in_process(mock.info().clone(), Arc::new(MockAdapter(mock)));
        supervisor.start_all().await.unwrap();

        let sup = Arc::new(Mutex::new(supervisor));
        let pipeline = Pipeline::default();

        let (mut cap_tx, cap_rx) = AudioRingBuffer::new(48_000 * 2, 2);
        let (out_tx, mut out_rx) = AudioRingBuffer::new(48_000 * 4, 2);
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);

        let sup_clone = Arc::clone(&sup);
        let bridge_handle = tokio::spawn(async move {
            let mut p = Pipeline::default();
            p.run_ring_bridge(sup_clone, cap_rx, out_tx, shutdown_rx)
                .await
                .unwrap();
            p.metrics()
        });

        let synth_input = make_48k_stereo_speech_with_silence();
        for chunk in synth_input.chunks(1_920) {
            cap_tx.push(chunk);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        tokio::time::sleep(Duration::from_millis(80)).await;
        let _ = shutdown_tx.send(());

        let metrics = bridge_handle.await.unwrap();
        assert_eq!(metrics.utterances_processed, 1);
        assert!(metrics.output_frames_emitted > 0);

        let mut drained = vec![0.0f32; 8_192];
        let read_samples = out_rx.pop(&mut drained);
        assert!(read_samples > 0);
        assert!(drained[..read_samples].iter().any(|s| s.abs() > 0.01));

        // Also verify direct route negotiation
        let route = pipeline.negotiate_route(&*sup.lock().await, None).unwrap();
        assert!(matches!(
            route,
            PipelineRoute::DirectSpeechTranslation { .. }
        ));
    }
}
