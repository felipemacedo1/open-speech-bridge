//! Milestone 3 integration tests: end-to-end audio resampling + VAD segmentation +
//! capability-composed pipeline across Rust `EngineSupervisor` and Python SDK UDS workers.

#![cfg(unix)]

use osb_daemon::{
    EngineHandler, EngineSupervisor, Pipeline, PipelineConfig, PipelineRoute, RestartPolicy,
    Runtime, WorkerSpec,
};
use osb_engine_mock::MockEngine;
use osb_protocol::capability::{Capability, CapabilitySet, LanguagePair};
use osb_protocol::engine::{EngineId, EngineInfo, EngineType};
use osb_protocol::message::{EngineRequest, EngineResponse};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn synthesize_48k_stereo_speech_utterance() -> Vec<f32> {
    let mut pcm = vec![0.0f32; 4_800 * 2];
    // 320 ms 440 Hz tone
    for i in 0..15_360 {
        let s = ((i as f32 / 48_000.0) * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.4;
        pcm.push(s);
        pcm.push(s);
    }
    // 420 ms trailing silence to trigger VAD hangover endpoint
    pcm.resize(pcm.len() + 20_160 * 2, 0.0f32);
    pcm
}

struct MockInProcess(MockEngine);

impl EngineHandler for MockInProcess {
    fn handle(&self, request: EngineRequest) -> EngineResponse {
        self.0.handle_request(request)
    }
}

#[tokio::test]
async fn test_m3_pipeline_with_python_sdk_uds_workers() {
    // Verify python3 is present in test environment
    if tokio::process::Command::new("python3")
        .arg("--version")
        .output()
        .await
        .is_err()
    {
        return;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sdk_root = manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("engines/sdk-python");
    assert!(
        sdk_root.is_dir(),
        "expected Python SDK at {}",
        sdk_root.display()
    );

    let inline_py_worker = r#"
import argparse, math, sys
from osb_engine_sdk import BaseEngineWorker, pcm_bytes_to_floats, pcm_floats_to_bytes

class TestPyWorker(BaseEngineWorker):
    def __init__(self, role):
        super().__init__()
        self.role = role

    def engine_info(self):
        if self.role == "st":
            return {
                "id": "py-whisper-test",
                "name": "Python Test Speech Translator",
                "version": "0.1.0",
                "engine_type": "python",
                "capabilities": {
                    "capabilities": ["speech_translation", "cpu_only"],
                    "language_pairs": [{"source": "pt-BR", "target": "en-US"}],
                    "stt_languages": [],
                    "tts_languages": [],
                },
                "model": "test",
                "license": "MIT",
                "weights_license": "MIT",
                "commercial_use": True,
            }
        return {
            "id": "py-kokoro-test",
            "name": "Python Test 24kHz TTS",
            "version": "0.1.0",
            "engine_type": "python",
            "capabilities": {
                "capabilities": ["tts", "cpu_only"],
                "language_pairs": [],
                "stt_languages": [],
                "tts_languages": ["en-US"],
            },
            "model": "test",
            "license": "Apache-2.0",
            "weights_license": "Apache-2.0",
            "commercial_use": True,
        }

    def process_speech_translation(self, req, pcm_bytes, flags):
        samples = pcm_bytes_to_floats(pcm_bytes)
        assert len(samples) > 1000
        return {
            "type": "speech_translation_result",
            "request_id": int(req["request_id"]),
            "result": {
                "status": "success",
                "processing_ms": 3,
                "data": {
                    "text": "Good morning from OpenSpeechBridge.",
                    "detected_source": "pt",
                    "target_lang": req["target_lang"],
                    "confidence": 0.98,
                },
            },
        }

    def process_tts(self, req):
        sr = 24000
        tone = [math.sin(2.0 * math.pi * 330.0 * i / sr) * 0.3 for i in range(sr // 5)]
        return (
            {
                "type": "tts_result",
                "request_id": int(req["request_id"]),
                "result": {
                    "status": "success",
                    "processing_ms": 2,
                    "data": {"audio": [], "sample_rate": sr, "duration_ms": 200},
                },
            },
            pcm_floats_to_bytes(tone),
        )

p = argparse.ArgumentParser()
BaseEngineWorker.add_common_cli_args(p)
p.add_argument("--role", required=True)
a = p.parse_args()
TestPyWorker(a.role).run_from_parsed_args(a)
"#;

    let runtime = Runtime::with_defaults();
    {
        let mut sup = runtime.supervisor().lock().await;
        let st_spec = WorkerSpec::spawn("python3", ["-c", inline_py_worker, "--role", "st"])
            .with_env("PYTHONPATH", sdk_root.display().to_string())
            .with_env("PYTHONDONTWRITEBYTECODE", "1")
            .with_startup_timeout(Duration::from_secs(5));
        let tts_spec = WorkerSpec::spawn("python3", ["-c", inline_py_worker, "--role", "tts"])
            .with_env("PYTHONPATH", sdk_root.display().to_string())
            .with_env("PYTHONDONTWRITEBYTECODE", "1")
            .with_startup_timeout(Duration::from_secs(5));

        sup.register_worker(
            EngineInfo::builder("py-whisper-test")
                .name("Py ST")
                .engine_type(EngineType::Python)
                .build(),
            st_spec,
            RestartPolicy::default(),
        );
        sup.register_worker(
            EngineInfo::builder("py-kokoro-test")
                .name("Py TTS")
                .engine_type(EngineType::Python)
                .build(),
            tts_spec,
            RestartPolicy::default(),
        );
    }

    runtime.start().await.unwrap();

    let mut pipeline = Pipeline::new(PipelineConfig::default());
    pipeline.start().unwrap();

    let route = {
        let sup = runtime.supervisor().lock().await;
        pipeline.negotiate_route(&sup, None).unwrap()
    };
    assert_eq!(
        route,
        PipelineRoute::DirectSpeechTranslation {
            speech_translation_engine: EngineId::new("py-whisper-test"),
            tts_engine: EngineId::new("py-kokoro-test"),
        }
    );

    let input_48k_stereo = synthesize_48k_stereo_speech_utterance();
    let mut segments = Vec::new();
    for chunk in input_48k_stereo.chunks(1_920) {
        segments.extend(pipeline.ingest_interleaved(chunk).unwrap());
    }
    segments.extend(pipeline.flush_input().unwrap());
    assert_eq!(segments.len(), 1);

    let translated = {
        let mut sup = runtime.supervisor().lock().await;
        pipeline
            .process_utterance(&mut sup, &route, &segments[0])
            .await
            .unwrap()
            .expect("translated utterance")
    };

    // 200 ms @ 24 kHz mono resampled to 48 kHz stereo => ~9,600 stereo frames (~19,200 f32 samples)
    assert!(translated.output_samples.len() >= 17_000);
    assert!(translated.output_samples.iter().any(|s| s.abs() > 0.05));
    assert_eq!(
        translated.translated_text.as_deref(),
        Some("Good morning from OpenSpeechBridge.")
    );

    runtime.stop().await.unwrap();
}

#[tokio::test]
async fn test_m3_pipeline_cascaded_stt_mt_tts_fallback_route() {
    let mut sup = EngineSupervisor::new();
    let mock = MockEngine::new();

    // Register separate STT-only, MT-only, and TTS-only handlers to verify 3-stage cascade negotiation.
    let mut stt_caps = CapabilitySet::new();
    stt_caps.add(Capability::Stt).add_stt_language("pt-BR");
    sup.register_in_process(
        EngineInfo::builder("only-stt")
            .name("STT")
            .capabilities(stt_caps)
            .build(),
        Arc::new(MockInProcess(mock.clone())),
    );

    let mut mt_caps = CapabilitySet::new();
    mt_caps
        .add(Capability::TextTranslation)
        .add_language_pair(LanguagePair::pt_en());
    sup.register_in_process(
        EngineInfo::builder("only-mt")
            .name("MT")
            .capabilities(mt_caps)
            .build(),
        Arc::new(MockInProcess(mock.clone())),
    );

    let mut tts_caps = CapabilitySet::new();
    tts_caps.add(Capability::Tts).add_tts_language("en-US");
    sup.register_in_process(
        EngineInfo::builder("only-tts")
            .name("TTS")
            .capabilities(tts_caps)
            .build(),
        Arc::new(MockInProcess(mock)),
    );

    sup.start_all().await.unwrap();

    let mut pipeline = Pipeline::default();
    pipeline.start().unwrap();
    let route = pipeline.negotiate_route(&sup, None).unwrap();
    assert_eq!(
        route,
        PipelineRoute::CascadedSttMtTts {
            stt_engine: EngineId::new("only-stt"),
            mt_engine: EngineId::new("only-mt"),
            tts_engine: EngineId::new("only-tts"),
        }
    );

    let input = synthesize_48k_stereo_speech_utterance();
    let mut segs = pipeline.ingest_interleaved(&input).unwrap();
    segs.extend(pipeline.flush_input().unwrap());
    assert_eq!(segs.len(), 1);

    let utt = pipeline
        .process_utterance(&mut sup, &route, &segs[0])
        .await
        .unwrap()
        .expect("cascaded output");
    assert!(!utt.output_samples.is_empty());
}

#[tokio::test]
async fn test_m4_full_duplex_bidirectional_and_m5_chunked_tts() {
    use osb_audio::AudioRingBuffer;
    use osb_protocol::message::{ProcessingResult, SpeechTranslationOutput};
    use tokio::sync::{broadcast, Mutex};

    struct ChunkedMockHandler(MockEngine);
    impl EngineHandler for ChunkedMockHandler {
        fn handle(&self, request: EngineRequest) -> EngineResponse {
            match request {
                EngineRequest::ProcessSpeechTranslation {
                    request_id,
                    target_lang,
                    ..
                } => EngineResponse::SpeechTranslationResult {
                    request_id,
                    result: ProcessingResult::Success {
                        data: SpeechTranslationOutput {
                            text:
                                "Hello team from OpenSpeechBridge, we are testing full duplex now."
                                    .to_string(),
                            detected_source: Some("pt-BR".to_string()),
                            target_lang,
                            confidence: Some(0.95),
                        },
                        processing_ms: 5,
                    },
                },
                EngineRequest::ProcessTts { .. } => {
                    std::thread::sleep(Duration::from_millis(8));
                    self.0.handle_request(request)
                }
                other => self.0.handle_request(other),
            }
        }
    }

    let mut sup = EngineSupervisor::new();
    let mock = MockEngine::new();
    sup.register_in_process(mock.info().clone(), Arc::new(ChunkedMockHandler(mock)));
    sup.start_all().await.unwrap();

    let shared_sup = Arc::new(Mutex::new(sup));
    let (shutdown_tx, shutdown_rx_out) = broadcast::channel(1);
    let shutdown_rx_in = shutdown_tx.subscribe();

    let (mut mic_tx, mic_rx) = AudioRingBuffer::new(48_000 * 2, 2);
    let (vmic_tx, mut vmic_rx) = AudioRingBuffer::new(48_000 * 4, 2);

    let (mut vsink_tx, vsink_rx) = AudioRingBuffer::new(48_000 * 2, 2);
    let (play_tx, mut play_rx) = AudioRingBuffer::new(48_000 * 4, 2);

    let sup_out = Arc::clone(&shared_sup);
    let out_task = tokio::spawn(async move {
        let mut p = Pipeline::new(PipelineConfig {
            source_language: "pt-BR".to_string(),
            target_language: "en-US".to_string(),
            chunked_tts: true,
            min_clause_words: 3,
            ..PipelineConfig::default()
        });
        p.run_ring_bridge(sup_out, mic_rx, vmic_tx, shutdown_rx_out)
            .await
            .unwrap();
        p.metrics()
    });

    let sup_in = Arc::clone(&shared_sup);
    let in_task = tokio::spawn(async move {
        let mut p = Pipeline::new(PipelineConfig {
            enable_outgoing: false,
            enable_incoming: true,
            source_language: "en-US".to_string(),
            target_language: "pt-BR".to_string(),
            chunked_tts: true,
            min_clause_words: 3,
            ..PipelineConfig::default()
        });
        p.run_ring_bridge(sup_in, vsink_rx, play_tx, shutdown_rx_in)
            .await
            .unwrap();
        p.metrics()
    });

    let speech = synthesize_48k_stereo_speech_utterance();
    for chunk in speech.chunks(1_920) {
        mic_tx.push(chunk);
        vsink_tx.push(chunk);
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    tokio::time::sleep(Duration::from_millis(220)).await;
    let _ = shutdown_tx.send(());

    let out_metrics = out_task.await.unwrap();
    let in_metrics = in_task.await.unwrap();

    assert_eq!(out_metrics.utterances_processed, 1);
    assert_eq!(in_metrics.utterances_processed, 1);
    assert!(out_metrics.output_frames_emitted > 0);
    assert!(in_metrics.output_frames_emitted > 0);
    // M5 chunked TTS: first clause audio is emitted before the second clause finishes TTS.
    assert!(out_metrics.last_first_audio_ms < out_metrics.last_total_ms);

    let mut buf = vec![0.0f32; 4_096];
    assert!(vmic_rx.pop(&mut buf) > 0);
    assert!(play_rx.pop(&mut buf) > 0);
}
