//! Offline and end-to-end WAV translation command (`openspeechbridge translate`).
//!
//! Feeds a RIFF/WAVE file through the daemon's `Pipeline` (resampling + bounded
//! VAD endpointing -> supervised UDS workers -> clause-chunked TTS -> output
//! resampling) and writes the synthesized target-language speech to an output WAV file.

use crate::wav::{read_wav_f32, write_wav_pcm16};
use anyhow::{bail, Context, Result};
use clap::Args;
use osb_audio::{resample_mono_buffer, SampleConverter, UtteranceSegment};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use osb_core::config::{Config, WorkerProcessConfig};
use osb_daemon::{
    EngineHandler, Pipeline, PipelineConfig, RestartPolicy, Runtime, RuntimeConfig, WorkerSpec,
};
use osb_engine_mock::MockEngine;
use osb_protocol::engine::{EngineInfo, EngineType};
use osb_protocol::message::{EngineRequest, EngineResponse};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// CLI arguments for `openspeechbridge translate`.
#[derive(Debug, Clone, Args)]
pub struct TranslateArgs {
    /// Input RIFF/WAVE audio file path
    #[arg(short, long)]
    pub input: PathBuf,

    /// Output RIFF/WAVE audio file path
    #[arg(short, long)]
    pub output: PathBuf,

    /// Optional TOML configuration file defining supervised UDS workers
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// Source language code
    #[arg(long, default_value = "pt-BR")]
    pub source: String,

    /// Target language code
    #[arg(long, default_value = "en-US")]
    pub target: String,

    /// Output WAV sample rate in Hz
    #[arg(long, default_value_t = 48_000)]
    pub output_rate: u32,

    /// Write mono output WAV instead of desktop stereo
    #[arg(long)]
    pub mono_output: bool,

    /// Bypass VAD segmentation and translate the entire input file as one utterance
    #[arg(long)]
    pub no_vad: bool,

    /// Disable M5 clause-chunked incremental TTS synthesis
    #[arg(long)]
    pub no_chunked_tts: bool,

    /// Use deterministic MockEngine instead of Python ML workers
    #[arg(long)]
    pub mock: bool,

    /// Python executable used to spawn default faster-whisper + Kokoro UDS workers
    #[arg(long, default_value = "python3")]
    pub python: PathBuf,

    /// Faster-Whisper model size when spawning default Python workers
    #[arg(long, default_value = "small")]
    pub whisper_model: String,

    /// Optional local cache directory for faster-whisper model weights
    #[arg(long)]
    pub whisper_cache: Option<PathBuf>,

    /// Optional HuggingFace cache directory (`HF_HOME`) for Kokoro weights
    #[arg(long)]
    pub hf_home: Option<PathBuf>,

    /// TTS voice identifier
    #[arg(long, default_value = "af_heart")]
    pub voice: String,

    /// CPU inference threads per ML worker
    #[arg(long, default_value_t = 4)]
    pub threads: usize,

    /// Explicitly print in-memory translated text (disabled by default for privacy)
    #[arg(long)]
    pub print_text: bool,
}

#[derive(Debug, Serialize)]
struct TranslateSummary {
    input_file: String,
    output_file: String,
    route: String,
    source_language: String,
    target_language: String,
    input_duration_ms: u64,
    output_duration_ms: u64,
    utterances_detected: usize,
    utterances_translated: usize,
    clauses_synthesized: usize,
    startup_ms: u64,
    speech_stage_ms: u64,
    mt_stage_ms: u64,
    tts_stage_ms: u64,
    first_audio_ms: u64,
    total_turnaround_ms: u64,
    realtime_factor: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    translated_text: Option<String>,
}

struct InProcessMock(MockEngine);

impl EngineHandler for InProcessMock {
    fn handle(&self, request: EngineRequest) -> EngineResponse {
        self.0.handle_request(request)
    }
}

pub async fn run(args: TranslateArgs, json_output: bool) -> Result<()> {
    let input_wav = read_wav_f32(&args.input)?;
    let input_duration_ms = input_wav.duration_ms();

    let runtime = build_translation_runtime(&args).await?;
    let startup_started = Instant::now();
    runtime
        .start()
        .await
        .context("failed to start translation runtime and engine workers")?;
    let startup_ms = startup_started.elapsed().as_millis() as u64;

    let output_format = AudioFormat {
        sample_rate: SampleRate::new(args.output_rate),
        sample_format: SampleFormat::F32,
        channels: if args.mono_output {
            ChannelLayout::Mono
        } else {
            ChannelLayout::Stereo
        },
    };

    let cfg_engine = &runtime.config().config.engine;
    let cfg_vad = runtime.config().config.audio.vad;
    let chunked_tts = if args.no_chunked_tts {
        false
    } else {
        cfg_engine.chunked_tts
    };

    let mut pipeline = Pipeline::new(
        PipelineConfig {
            input_format: input_wav.format,
            output_format,
            engine_format: AudioFormat::SPEECH_RECOGNITION,
            source_language: args.source.clone(),
            target_language: args.target.clone(),
            voice_id: Some(args.voice.clone()),
            chunked_tts,
            min_clause_words: cfg_engine.min_clause_words,
            ..PipelineConfig::default()
        }
        .with_vad_params(cfg_vad),
    );
    pipeline.start()?;

    let route = {
        let sup = runtime.supervisor().lock().await;
        pipeline.negotiate_route(&sup, Some(&runtime.config().config.engine))?
    };

    let turnaround_started = Instant::now();
    let segments = collect_segments(
        &mut pipeline,
        &input_wav.samples,
        input_wav.format,
        args.no_vad,
    )?;
    let utterances_detected = segments.len();

    let mut combined_output: Vec<f32> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    let mut speech_stage_ms = 0u64;
    let mut mt_stage_ms = 0u64;
    let mut tts_stage_ms = 0u64;
    let mut first_audio_ms = 0u64;
    let mut clauses_synthesized = 0usize;
    let mut utterances_translated = 0usize;

    for seg in &segments {
        let translated = {
            let mut sup = runtime.supervisor().lock().await;
            pipeline.process_utterance(&mut sup, &route, seg).await?
        };
        if let Some(utt) = translated {
            if utterances_translated == 0 {
                first_audio_ms = utt.first_audio_ms;
            }
            utterances_translated += 1;
            clauses_synthesized += utt.clauses_synthesized;
            speech_stage_ms += utt.speech_stage_ms;
            mt_stage_ms += utt.mt_stage_ms;
            tts_stage_ms += utt.tts_stage_ms;
            combined_output.extend_from_slice(&utt.output_samples);
            if args.print_text {
                if let Some(t) = utt.translated_text {
                    texts.push(t);
                }
            }
        }
    }

    let total_turnaround_ms = turnaround_started.elapsed().as_millis() as u64;
    if first_audio_ms == 0 {
        first_audio_ms = total_turnaround_ms;
    }
    let _ = pipeline.stop();
    let _ = runtime.stop().await;

    write_wav_pcm16(&args.output, &combined_output, output_format)?;

    let out_channels = output_format.channels.channels() as u64;
    let out_frames = (combined_output.len() as u64) / out_channels.max(1);
    let output_duration_ms =
        (out_frames * 1_000) / u64::from(output_format.sample_rate.hz().max(1));
    let rtf = if input_duration_ms > 0 {
        (total_turnaround_ms as f64) / (input_duration_ms as f64)
    } else {
        0.0
    };

    let summary = TranslateSummary {
        input_file: args.input.display().to_string(),
        output_file: args.output.display().to_string(),
        route: format_route_label(&route),
        source_language: args.source,
        target_language: args.target,
        input_duration_ms,
        output_duration_ms,
        utterances_detected,
        utterances_translated,
        clauses_synthesized,
        startup_ms,
        speech_stage_ms,
        mt_stage_ms,
        tts_stage_ms,
        first_audio_ms,
        total_turnaround_ms,
        realtime_factor: (rtf * 1000.0).round() / 1000.0,
        translated_text: args.print_text.then(|| texts.join(" ")),
    };

    emit_summary(&summary, json_output)?;
    Ok(())
}

fn collect_segments(
    pipeline: &mut Pipeline,
    samples: &[f32],
    input_format: AudioFormat,
    no_vad: bool,
) -> Result<Vec<UtteranceSegment>> {
    if no_vad {
        let mono = match input_format.channels {
            ChannelLayout::Mono => samples.to_vec(),
            ChannelLayout::Stereo => {
                let mut m = vec![0.0f32; samples.len() / 2];
                SampleConverter::stereo_to_mono(samples, &mut m);
                m
            }
        };
        let mono_16k =
            resample_mono_buffer(&mono, input_format.sample_rate, SampleRate::SPEECH_16K)?;
        if mono_16k.is_empty() {
            return Ok(Vec::new());
        }
        let end_us = (mono_16k.len() as u64 * 1_000_000) / u64::from(SampleRate::SPEECH_16K.hz());
        return Ok(vec![UtteranceSegment {
            utterance_id: 1,
            samples: mono_16k,
            sample_rate: SampleRate::SPEECH_16K.hz(),
            start_timestamp_us: 0,
            end_timestamp_us: end_us,
            forced_cutoff: true,
        }]);
    }

    let step = (input_format.sample_rate.samples_for_ms(20) as usize
        * input_format.channels.channels() as usize)
        .max(320);
    let mut segments = Vec::new();
    for chunk in samples.chunks(step) {
        segments.extend(pipeline.ingest_interleaved(chunk)?);
    }
    segments.extend(pipeline.flush_input()?);
    Ok(segments)
}

async fn build_translation_runtime(args: &TranslateArgs) -> Result<Runtime> {
    if let Some(ref cfg_path) = args.config {
        return Runtime::from_config_file(cfg_path)
            .with_context(|| format!("failed to load config from {}", cfg_path.display()));
    }

    if args.mock {
        let runtime = Runtime::with_defaults();
        let mut sup = runtime.supervisor().lock().await;
        if let Some(mock_bin) = find_sibling_binary("osb-engine-mock") {
            let info = EngineInfo::builder("mock")
                .name("Mock Engine (UDS Worker)")
                .version("0.1.0")
                .engine_type(EngineType::Mock)
                .build();
            let spec = WorkerSpec::spawn(mock_bin, std::iter::empty::<&str>())
                .with_startup_timeout(Duration::from_secs(5))
                .with_processing_timeout(Duration::from_secs(5));
            sup.register_worker(info, spec, RestartPolicy::default());
        } else {
            let mock = MockEngine::new();
            sup.register_in_process(mock.info().clone(), Arc::new(InProcessMock(mock)));
        }
        drop(sup);
        return Ok(runtime);
    }

    let sdk_workers_dir = locate_python_workers_dir()?;
    let whisper_script = sdk_workers_dir.join("faster_whisper_worker.py");
    let kokoro_script = sdk_workers_dir.join("kokoro_worker.py");
    let mt_script = sdk_workers_dir.join("text_translation_worker.py");

    let mut whisper_args = vec![
        whisper_script.display().to_string(),
        "--engine-id".to_string(),
        "faster-whisper".to_string(),
        "--model".to_string(),
        args.whisper_model.clone(),
        "--threads".to_string(),
        args.threads.to_string(),
    ];
    if let Some(ref cache_dir) = args.whisper_cache {
        whisper_args.push("--download-root".to_string());
        whisper_args.push(cache_dir.display().to_string());
    }

    let mut kokoro_args = vec![
        kokoro_script.display().to_string(),
        "--engine-id".to_string(),
        "kokoro-tts".to_string(),
        "--voice".to_string(),
        args.voice.clone(),
        "--threads".to_string(),
        args.threads.to_string(),
    ];
    if let Some(ref hf_dir) = args.hf_home {
        kokoro_args.push("--hf-home".to_string());
        kokoro_args.push(hf_dir.display().to_string());
    }

    let mut workers = vec![
        WorkerProcessConfig {
            id: "faster-whisper".to_string(),
            name: "Faster-Whisper CPU Worker".to_string(),
            program: args.python.display().to_string(),
            args: whisper_args,
            env: BTreeMap::new(),
            max_restarts: 2,
        },
        WorkerProcessConfig {
            id: "kokoro-tts".to_string(),
            name: "Kokoro-82M CPU TTS Worker".to_string(),
            program: args.python.display().to_string(),
            args: kokoro_args,
            env: BTreeMap::new(),
            max_restarts: 2,
        },
    ];

    if !args.target.to_lowercase().starts_with("en") && mt_script.is_file() {
        workers.push(WorkerProcessConfig {
            id: "text-translator".to_string(),
            name: "Local Text Translation Worker".to_string(),
            program: args.python.display().to_string(),
            args: vec![
                mt_script.display().to_string(),
                "--engine-id".to_string(),
                "text-translator".to_string(),
                "--threads".to_string(),
                args.threads.to_string(),
            ],
            env: BTreeMap::new(),
            max_restarts: 2,
        });
    }

    let mut cfg = Config::default();
    cfg.engine.source_language = args.source.clone();
    cfg.engine.target_language = args.target.clone();
    cfg.engine.speech_translation_engine = Some("faster-whisper".to_string());
    cfg.engine.stt_engine = Some("faster-whisper".to_string());
    cfg.engine.translation_engine = Some("text-translator".to_string());
    cfg.engine.tts_engine = Some("kokoro-tts".to_string());
    cfg.engine.startup_timeout_ms = 60_000;
    cfg.engine.processing_timeout_ms = 30_000;
    cfg.engine.workers = workers;

    Ok(Runtime::new(RuntimeConfig {
        config: cfg,
        ..RuntimeConfig::default()
    }))
}

fn find_sibling_binary(name: &str) -> Option<PathBuf> {
    let cur = std::env::current_exe().ok()?;
    let candidate = cur.parent()?.join(name);
    candidate.is_file().then_some(candidate)
}

fn locate_python_workers_dir() -> Result<PathBuf> {
    let rel = Path::new("engines/sdk-python/osb_engine_sdk/workers");
    if rel.is_dir() {
        return Ok(rel.to_path_buf());
    }
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().take(5) {
            let candidate = ancestor.join(rel);
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
    }
    bail!(
        "could not locate Python SDK workers directory ({}); pass --config <toml> or run from workspace root",
        rel.display()
    )
}

fn format_route_label(route: &osb_daemon::PipelineRoute) -> String {
    match route {
        osb_daemon::PipelineRoute::DirectSpeechTranslation {
            speech_translation_engine,
            tts_engine,
        } => format!("DirectSpeechTranslation({speech_translation_engine} -> {tts_engine})"),
        osb_daemon::PipelineRoute::CascadedSttMtTts {
            stt_engine,
            mt_engine,
            tts_engine,
        } => format!("CascadedSttMtTts({stt_engine} -> {mt_engine} -> {tts_engine})"),
        osb_daemon::PipelineRoute::EndToEndS2s { s2s_engine } => {
            format!("EndToEndS2s({s2s_engine})")
        }
    }
}

fn emit_summary(summary: &TranslateSummary, json_output: bool) -> Result<()> {
    if json_output {
        println!("{}", serde_json::to_string_pretty(summary)?);
        return Ok(());
    }

    println!("OpenSpeechBridge Local Translation Complete");
    println!("═══════════════════════════════════════════");
    println!(
        "  Input:               {} ({} ms)",
        summary.input_file, summary.input_duration_ms
    );
    println!(
        "  Output:              {} ({} ms)",
        summary.output_file, summary.output_duration_ms
    );
    println!(
        "  Languages:           {} -> {}",
        summary.source_language, summary.target_language
    );
    println!("  Route:               {}", summary.route);
    println!(
        "  Utterances:          {} translated / {} detected ({} TTS clauses)",
        summary.utterances_translated, summary.utterances_detected, summary.clauses_synthesized
    );
    println!("  Worker startup:      {} ms", summary.startup_ms);
    println!("  Speech stage:        {} ms", summary.speech_stage_ms);
    if summary.mt_stage_ms > 0 {
        println!("  MT stage:            {} ms", summary.mt_stage_ms);
    }
    println!("  TTS stage:           {} ms", summary.tts_stage_ms);
    println!("  First audio (TTFA):  {} ms", summary.first_audio_ms);
    println!(
        "  Total turnaround:    {} ms (RTF {:.3}x)",
        summary.total_turnaround_ms, summary.realtime_factor
    );
    if let Some(ref text) = summary.translated_text {
        println!("  Translated text:     {}", text);
    }
    Ok(())
}
