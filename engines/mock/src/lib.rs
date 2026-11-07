//! Mock Engine and persistent UDS worker for OpenSpeechBridge.
//!
//! Provides deterministic, zero-model audio/text transformations, protocol
//! handshake negotiation, cancellation, and optional fault injection for
//! testing Milestone 2 supervision and IPC.

use osb_protocol::capability::{Capability, CapabilitySet, LanguagePair};
use osb_protocol::engine::{EngineInfo, EngineType};
use osb_protocol::framing::tokio_io::{read_request_frame_async, write_response_frame_async};
use osb_protocol::framing::{FrameFlags, FrameLimits, FramingError, DEFAULT_MAX_PAYLOAD_BYTES};
use osb_protocol::message::{
    EngineRequest, EngineResponse, ProcessingResult, S2sOutput, SpeechTranslationOutput, SttOutput,
    TranslationOutput, TtsOutput,
};
use osb_protocol::PROTOCOL_VERSION;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::task::AbortHandle;

/// Default maximum in-flight requests advertised by the mock worker.
pub const DEFAULT_MOCK_MAX_IN_FLIGHT: u32 = 8;

/// Runtime behavior configuration for [`MockEngine`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MockEngineConfig {
    /// Artificial processing delay in milliseconds per processing request.
    pub simulated_delay_ms: u64,
    /// Amplitude multiplier applied to audio samples in `ProcessS2s`.
    pub s2s_gain: f32,
    /// Optional request ID on which the worker simulates an immediate fatal crash.
    pub crash_on_request_id: Option<u64>,
}

impl Default for MockEngineConfig {
    fn default() -> Self {
        Self {
            simulated_delay_ms: 0,
            s2s_gain: 0.5,
            crash_on_request_id: None,
        }
    }
}

/// Deterministic mock engine implementation.
#[derive(Clone)]
pub struct MockEngine {
    info: EngineInfo,
    config: Arc<RwLock<MockEngineConfig>>,
}

impl MockEngine {
    /// Create a new mock engine with default settings.
    pub fn new() -> Self {
        Self::with_config(MockEngineConfig::default())
    }

    /// Create a new mock engine with explicit configuration.
    pub fn with_config(config: MockEngineConfig) -> Self {
        let mut capabilities = CapabilitySet::new();
        capabilities
            .add(Capability::Stt)
            .add(Capability::StreamingStt)
            .add(Capability::SpeechTranslation)
            .add(Capability::StreamingSpeechTranslation)
            .add(Capability::TextTranslation)
            .add(Capability::Tts)
            .add(Capability::SpeechToSpeech)
            .add(Capability::CpuOnly)
            .add_stt_language("en-US")
            .add_stt_language("pt-BR")
            .add_tts_language("en-US")
            .add_tts_language("pt-BR")
            .add_language_pair(LanguagePair::pt_en())
            .add_language_pair(LanguagePair::en_pt());

        let info = EngineInfo::builder("mock")
            .name("Mock Engine")
            .version("0.1.0")
            .engine_type(EngineType::Mock)
            .capabilities(capabilities)
            .license("Apache-2.0")
            .weights_license("Apache-2.0")
            .commercial_use(true)
            .build();

        Self {
            info,
            config: Arc::new(RwLock::new(config)),
        }
    }

    /// Get engine metadata and capability set.
    pub fn info(&self) -> &EngineInfo {
        &self.info
    }

    /// Snapshot current mock configuration.
    pub fn config(&self) -> MockEngineConfig {
        self.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    /// Synchronously handle a single [`EngineRequest`].
    pub fn handle_request(&self, request: EngineRequest) -> EngineResponse {
        match request {
            EngineRequest::Hello {
                max_in_flight,
                max_payload_bytes,
                ..
            } => EngineResponse::Welcome {
                protocol_version: PROTOCOL_VERSION.to_string(),
                info: self.info.clone(),
                max_in_flight: max_in_flight.clamp(1, DEFAULT_MOCK_MAX_IN_FLIGHT),
                max_payload_bytes: max_payload_bytes.min(DEFAULT_MAX_PAYLOAD_BYTES),
            },
            EngineRequest::Initialize { config } => {
                if !config.is_null() {
                    if let Ok(parsed) = serde_json::from_value::<MockEngineConfig>(config) {
                        if let Ok(mut guard) = self.config.write() {
                            *guard = parsed;
                        }
                    }
                }
                EngineResponse::Initialized {
                    engine_id: self.info.id.clone(),
                }
            }
            EngineRequest::Shutdown => EngineResponse::ShutdownComplete,
            EngineRequest::GetCapabilities => EngineResponse::Capabilities {
                info: serde_json::to_value(&self.info).unwrap_or_default(),
            },
            EngineRequest::Ping { timestamp_ms } => EngineResponse::Pong {
                timestamp_ms,
                engine_timestamp_ms: current_epoch_ms(),
            },
            EngineRequest::ProcessStt {
                request_id,
                audio,
                is_final,
                ..
            } => handle_stt(request_id, &audio, is_final),
            EngineRequest::ProcessSpeechTranslation {
                request_id,
                audio,
                source_lang,
                target_lang,
                is_final,
                ..
            } => {
                handle_speech_translation(request_id, &audio, &source_lang, &target_lang, is_final)
            }
            EngineRequest::ProcessTranslation {
                request_id,
                text,
                source_lang,
                target_lang,
            } => handle_translation(request_id, &text, source_lang, &target_lang),
            EngineRequest::ProcessTts {
                request_id, text, ..
            } => handle_tts(request_id, &text),
            EngineRequest::ProcessS2s {
                request_id,
                audio,
                sample_rate,
                source_lang,
                target_lang,
                is_final,
            } => handle_s2s(
                request_id,
                &audio,
                sample_rate,
                &source_lang,
                &target_lang,
                is_final,
                self.config().s2s_gain,
            ),
            EngineRequest::Cancel { request_id } => EngineResponse::Cancelled { request_id },
        }
    }
}

impl Default for MockEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn current_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn mean_abs_energy(audio: &[f32]) -> f32 {
    if audio.is_empty() {
        return 0.0;
    }
    audio.iter().map(|s| s.abs()).sum::<f32>() / audio.len() as f32
}

fn handle_stt(request_id: u64, audio: &[f32], is_final: bool) -> EngineResponse {
    let start = Instant::now();
    let text = if mean_abs_energy(audio) > 0.01 {
        "[mock: speech detected]".to_string()
    } else {
        "[mock: silence]".to_string()
    };

    let result = if is_final {
        ProcessingResult::Success {
            data: SttOutput {
                text,
                language: Some("en".to_string()),
                confidence: Some(0.95),
                words: None,
            },
            processing_ms: start.elapsed().as_millis() as u64,
        }
    } else {
        ProcessingResult::Partial {
            data: SttOutput {
                text,
                language: None,
                confidence: None,
                words: None,
            },
            has_more: true,
        }
    };

    EngineResponse::SttResult { request_id, result }
}

fn handle_speech_translation(
    request_id: u64,
    audio: &[f32],
    source_lang: &str,
    target_lang: &str,
    is_final: bool,
) -> EngineResponse {
    let start = Instant::now();
    let text = if mean_abs_energy(audio) > 0.01 {
        format!("[mock {source_lang}->{target_lang}: translated speech]")
    } else {
        String::new()
    };

    let output = SpeechTranslationOutput {
        text,
        detected_source: Some(source_lang.to_string()),
        target_lang: target_lang.to_string(),
        confidence: Some(0.93),
    };

    let result = if is_final {
        ProcessingResult::Success {
            data: output,
            processing_ms: start.elapsed().as_millis() as u64,
        }
    } else {
        ProcessingResult::Partial {
            data: output,
            has_more: true,
        }
    };

    EngineResponse::SpeechTranslationResult { request_id, result }
}

fn handle_translation(
    request_id: u64,
    text: &str,
    source_lang: String,
    target_lang: &str,
) -> EngineResponse {
    let start = Instant::now();
    let translated = format!("[{source_lang} → {target_lang}] {text}");

    EngineResponse::TranslationResult {
        request_id,
        result: ProcessingResult::Success {
            data: TranslationOutput {
                text: translated,
                detected_source: Some(source_lang),
            },
            processing_ms: start.elapsed().as_millis() as u64,
        },
    }
}

fn handle_tts(request_id: u64, text: &str) -> EngineResponse {
    let start = Instant::now();
    let duration_ms = ((text.len() as u64) * 50).clamp(50, 5_000);
    let sample_rate = 24_000u32;
    let num_samples = (u64::from(sample_rate) * duration_ms / 1_000) as usize;

    let audio: Vec<f32> = (0..num_samples)
        .map(|i| {
            let t = i as f32 / sample_rate as f32;
            (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.3
        })
        .collect();

    EngineResponse::TtsResult {
        request_id,
        result: ProcessingResult::Success {
            data: TtsOutput {
                audio,
                sample_rate,
                duration_ms,
            },
            processing_ms: start.elapsed().as_millis() as u64,
        },
    }
}

fn handle_s2s(
    request_id: u64,
    audio: &[f32],
    sample_rate: u32,
    source_lang: &str,
    target_lang: &str,
    is_final: bool,
    gain: f32,
) -> EngineResponse {
    let start = Instant::now();
    let transformed: Vec<f32> = audio.iter().map(|s| (*s * gain).clamp(-1.0, 1.0)).collect();
    let has_speech = mean_abs_energy(audio) > 0.01;

    let data = S2sOutput {
        audio: transformed,
        sample_rate,
        transcription: has_speech.then(|| format!("[mock {source_lang} speech]")),
        translation: has_speech.then(|| format!("[mock {target_lang} translation]")),
    };

    let result = if is_final {
        ProcessingResult::Success {
            data,
            processing_ms: start.elapsed().as_millis() as u64,
        }
    } else {
        ProcessingResult::Partial {
            data,
            has_more: true,
        }
    };

    EngineResponse::S2sResult { request_id, result }
}

/// Serve `OSBP` v1 framed requests over any bidirectional async byte stream.
///
/// Supports concurrent processing, out-of-band `Ping`/`Pong`, task-level
/// `Cancel` handling, and clean exit on `Shutdown` or EOF.
pub async fn serve_stream<S>(
    stream: S,
    engine: MockEngine,
    limits: FrameLimits,
) -> Result<(), FramingError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut reader, writer) = tokio::io::split(stream);
    let writer = Arc::new(Mutex::new(writer));
    let active_tasks: Arc<Mutex<HashMap<u64, AbortHandle>>> = Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (request, _flags) = match read_request_frame_async(&mut reader, limits).await {
            Ok(pair) => pair,
            Err(FramingError::UnexpectedEof) => return Ok(()),
            Err(err) => return Err(err),
        };

        if let Some(req_id) = request.request_id() {
            if engine.config().crash_on_request_id == Some(req_id)
                && !matches!(request, EngineRequest::Cancel { .. })
            {
                // Drop connection immediately (or exit process in standalone mode) to simulate crash.
                return Err(FramingError::UnexpectedEof);
            }
        }

        match request {
            EngineRequest::Shutdown => {
                abort_all_active(&active_tasks).await;
                let mut w = writer.lock().await;
                write_response_frame_async(
                    &mut *w,
                    &EngineResponse::ShutdownComplete,
                    FrameFlags::END_OF_STREAM,
                    limits,
                )
                .await?;
                return Ok(());
            }
            EngineRequest::Cancel { request_id } => {
                if let Some(handle) = active_tasks.lock().await.remove(&request_id) {
                    handle.abort();
                }
                let mut w = writer.lock().await;
                write_response_frame_async(
                    &mut *w,
                    &EngineResponse::Cancelled { request_id },
                    FrameFlags::NONE,
                    limits,
                )
                .await?;
            }
            req if req.request_id().is_some() => {
                let req_id = req.request_id().unwrap_or_default();
                spawn_processing_task(
                    req_id,
                    req,
                    engine.clone(),
                    Arc::clone(&writer),
                    Arc::clone(&active_tasks),
                    limits,
                )
                .await;
            }
            control_req => {
                let response = engine.handle_request(control_req);
                let mut w = writer.lock().await;
                write_response_frame_async(&mut *w, &response, FrameFlags::NONE, limits).await?;
            }
        }
    }
}

async fn abort_all_active(active_tasks: &Mutex<HashMap<u64, AbortHandle>>) {
    let mut map = active_tasks.lock().await;
    for (_, handle) in map.drain() {
        handle.abort();
    }
}

async fn spawn_processing_task<W>(
    req_id: u64,
    request: EngineRequest,
    engine: MockEngine,
    writer: Arc<Mutex<W>>,
    active_tasks: Arc<Mutex<HashMap<u64, AbortHandle>>>,
    limits: FrameLimits,
) where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let delay_ms = engine.config().simulated_delay_ms;
    let tasks_cleanup = Arc::clone(&active_tasks);

    let join = tokio::spawn(async move {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        let response = engine.handle_request(request);
        tasks_cleanup.lock().await.remove(&req_id);
        let mut w = writer.lock().await;
        let _ = write_response_frame_async(&mut *w, &response, FrameFlags::NONE, limits).await;
    });

    active_tasks
        .lock()
        .await
        .insert(req_id, join.abort_handle());
}

/// Connect to a supervisor Unix Domain Socket and run the persistent mock worker loop.
#[cfg(unix)]
pub async fn connect_and_serve_uds(
    socket_path: &std::path::Path,
    engine: MockEngine,
    limits: FrameLimits,
) -> Result<(), FramingError> {
    let stream = tokio::net::UnixStream::connect(socket_path).await?;
    serve_stream(stream, engine, limits).await
}

/// Listen on a Unix Domain Socket and serve incoming client sessions sequentially.
#[cfg(unix)]
pub async fn listen_and_serve_uds(
    socket_path: &std::path::Path,
    engine: MockEngine,
    limits: FrameLimits,
) -> Result<(), FramingError> {
    let _ = tokio::fs::remove_file(socket_path).await;
    let listener = tokio::net::UnixListener::bind(socket_path)?;
    let (stream, _) = listener.accept().await?;
    let res = serve_stream(stream, engine, limits).await;
    let _ = tokio::fs::remove_file(socket_path).await;
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_engine_creation() {
        let engine = MockEngine::new();
        assert_eq!(engine.info().id.as_str(), "mock");
        assert_eq!(engine.info().name, "Mock Engine");
        assert!(engine.info().capabilities.can_translate_speech());
        assert!(engine.info().capabilities.can_speech_to_speech());
    }

    #[test]
    fn test_mock_stt() {
        let engine = MockEngine::new();
        let request = EngineRequest::ProcessStt {
            request_id: 1,
            audio: vec![0.5; 1000],
            sample_rate: 16_000,
            is_final: true,
        };
        match engine.handle_request(request) {
            EngineResponse::SttResult { request_id, result } => {
                assert_eq!(request_id, 1);
                assert!(result.is_success());
            }
            _ => panic!("Expected SttResult"),
        }
    }

    #[test]
    fn test_mock_speech_translation() {
        let engine = MockEngine::new();
        let request = EngineRequest::ProcessSpeechTranslation {
            request_id: 11,
            audio: vec![0.25; 800],
            sample_rate: 16_000,
            source_lang: "pt-BR".to_string(),
            target_lang: "en-US".to_string(),
            is_final: true,
            meta: None,
        };
        match engine.handle_request(request) {
            EngineResponse::SpeechTranslationResult { request_id, result } => {
                assert_eq!(request_id, 11);
                let data = result.data().unwrap();
                assert!(data.text.contains("pt-BR->en-US"));
            }
            _ => panic!("Expected SpeechTranslationResult"),
        }
    }

    #[test]
    fn test_mock_translation() {
        let engine = MockEngine::new();
        let request = EngineRequest::ProcessTranslation {
            request_id: 2,
            text: "Hello".to_string(),
            source_lang: "en".to_string(),
            target_lang: "pt".to_string(),
        };
        match engine.handle_request(request) {
            EngineResponse::TranslationResult { request_id, result } => {
                assert_eq!(request_id, 2);
                if let ProcessingResult::Success { data, .. } = result {
                    assert!(data.text.contains("Hello"));
                }
            }
            _ => panic!("Expected TranslationResult"),
        }
    }

    #[test]
    fn test_mock_tts() {
        let engine = MockEngine::new();
        let request = EngineRequest::ProcessTts {
            request_id: 3,
            text: "Test".to_string(),
            language: "en".to_string(),
            voice_id: None,
        };
        match engine.handle_request(request) {
            EngineResponse::TtsResult { request_id, result } => {
                assert_eq!(request_id, 3);
                if let ProcessingResult::Success { data, .. } = result {
                    assert!(!data.audio.is_empty());
                    assert_eq!(data.sample_rate, 24_000);
                }
            }
            _ => panic!("Expected TtsResult"),
        }
    }

    #[test]
    fn test_mock_s2s_transforms_audio() {
        let engine = MockEngine::with_config(MockEngineConfig {
            simulated_delay_ms: 0,
            s2s_gain: 0.5,
            crash_on_request_id: None,
        });
        let request = EngineRequest::ProcessS2s {
            request_id: 4,
            audio: vec![0.8, -0.4, 0.2],
            sample_rate: 16_000,
            source_lang: "pt-BR".to_string(),
            target_lang: "en-US".to_string(),
            is_final: true,
        };
        match engine.handle_request(request) {
            EngineResponse::S2sResult { request_id, result } => {
                assert_eq!(request_id, 4);
                let data = result.data().unwrap();
                assert_eq!(data.audio, vec![0.4, -0.2, 0.1]);
            }
            _ => panic!("Expected S2sResult"),
        }
    }
}
