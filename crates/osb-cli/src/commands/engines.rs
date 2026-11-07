//! List engines command.

use anyhow::Result;
use osb_daemon::engine_supervisor::EngineSupervisor;
use osb_protocol::capability::{Capability, CapabilitySet, LanguagePair};
use osb_protocol::engine::{EngineInfo, EngineType};
use osb_protocol::framing::WIRE_VERSION;
use osb_protocol::PROTOCOL_VERSION;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct EngineListOutput {
    protocol_version: String,
    wire_format_version: u16,
    engines: Vec<EngineOutput>,
    note: String,
}

#[derive(Serialize)]
struct EngineOutput {
    id: String,
    name: String,
    version: String,
    engine_type: String,
    capabilities: Vec<String>,
    license: Option<String>,
    weights_license: Option<String>,
    commercial_use: Option<bool>,
    worker_available: bool,
}

fn find_mock_worker_binary() -> Option<PathBuf> {
    let cur = std::env::current_exe().ok()?;
    let sibling = cur.parent()?.join("osb-engine-mock");
    sibling.is_file().then_some(sibling)
}

fn find_python_worker_script(filename: &str) -> Option<PathBuf> {
    let rel = Path::new("engines/sdk-python/osb_engine_sdk/workers").join(filename);
    if rel.is_file() {
        return Some(rel);
    }
    let exe = std::env::current_exe().ok()?;
    for ancestor in exe.ancestors().take(5) {
        let candidate = ancestor.join(&rel);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub async fn run(json_output: bool) -> Result<()> {
    let mut supervisor = EngineSupervisor::new();
    let mock_bin = find_mock_worker_binary();
    let whisper_script = find_python_worker_script("faster_whisper_worker.py");
    let kokoro_script = find_python_worker_script("kokoro_worker.py");

    supervisor.register(build_whisper_info());
    supervisor.register(build_kokoro_info());
    supervisor.register(build_mock_info());

    let mut engines = supervisor.list_engines();
    engines.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

    if json_output {
        let output = EngineListOutput {
            protocol_version: PROTOCOL_VERSION.to_string(),
            wire_format_version: WIRE_VERSION,
            engines: engines
                .iter()
                .map(|e| {
                    let mut caps: Vec<String> = e
                        .capabilities
                        .capabilities()
                        .iter()
                        .map(|c| format!("{c:?}"))
                        .collect();
                    caps.sort();
                    let avail = match e.id.as_str() {
                        "mock" => mock_bin.is_some(),
                        "faster-whisper" => whisper_script.is_some(),
                        "kokoro-tts" => kokoro_script.is_some(),
                        _ => false,
                    };
                    EngineOutput {
                        id: e.id.to_string(),
                        name: e.name.clone(),
                        version: e.version.clone(),
                        engine_type: e.engine_type.to_string(),
                        capabilities: caps,
                        license: e.license.clone(),
                        weights_license: e.weights_license.clone(),
                        commercial_use: e.commercial_use,
                        worker_available: avail,
                    }
                })
                .collect(),
            note: "M3 local speech-translation workers active over OSBP v1 Unix Domain Sockets."
                .to_string(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!(
            "Available Engines (Protocol v{}, Wire OSBP v{}):",
            PROTOCOL_VERSION, WIRE_VERSION
        );
        println!("══════════════════════════════════════════════════");
        println!();

        for engine in engines {
            let commercial = match engine.commercial_use {
                Some(true) => "✓ commercial OK",
                Some(false) => "✗ non-commercial",
                None => "? unknown",
            };
            println!("  {} ({})", engine.name, engine.id);
            println!("    Version:         {}", engine.version);
            println!("    Type:            {}", engine.engine_type);
            println!(
                "    Code License:    {}",
                engine.license.as_deref().unwrap_or("unknown")
            );
            println!(
                "    Weights License: {}",
                engine.weights_license.as_deref().unwrap_or("n/a")
            );
            println!("    Commercial:      {}", commercial);
            let worker_status = match engine.id.as_str() {
                "mock" => mock_bin
                    .as_ref()
                    .map(|p| format!("built ({})", p.display()))
                    .unwrap_or_else(|| "not built (run `cargo build -p osb-engine-mock`)".into()),
                "faster-whisper" => whisper_script
                    .as_ref()
                    .map(|p| format!("ready ({})", p.display()))
                    .unwrap_or_else(|| "script not found".into()),
                "kokoro-tts" => kokoro_script
                    .as_ref()
                    .map(|p| format!("ready ({})", p.display()))
                    .unwrap_or_else(|| "script not found".into()),
                _ => "external".into(),
            };
            println!("    UDS Worker:      {}", worker_status);
            println!();
        }
    }

    Ok(())
}

fn build_whisper_info() -> EngineInfo {
    let mut caps = CapabilitySet::new();
    caps.add(Capability::Stt)
        .add(Capability::SpeechTranslation)
        .add(Capability::CpuOnly)
        .add_stt_language("pt-BR")
        .add_stt_language("en-US")
        .add_language_pair(LanguagePair::pt_en());

    EngineInfo::builder("faster-whisper")
        .name("Faster-Whisper CPU Worker (small/int8)")
        .version("0.1.0")
        .engine_type(EngineType::Python)
        .capabilities(caps)
        .model("Systran/faster-whisper-small")
        .license("MIT")
        .weights_license("MIT")
        .commercial_use(true)
        .build()
}

fn build_kokoro_info() -> EngineInfo {
    let mut caps = CapabilitySet::new();
    caps.add(Capability::Tts)
        .add(Capability::CpuOnly)
        .add_tts_language("en-US")
        .add_tts_language("pt-BR");

    EngineInfo::builder("kokoro-tts")
        .name("Kokoro-82M CPU TTS Worker")
        .version("0.1.0")
        .engine_type(EngineType::Python)
        .capabilities(caps)
        .model("hexgrad/Kokoro-82M")
        .license("Apache-2.0")
        .weights_license("Apache-2.0 (verify voice pack provenance before redistribution)")
        .commercial_use(true)
        .build()
}

fn build_mock_info() -> EngineInfo {
    let mut mock_caps = CapabilitySet::new();
    mock_caps
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

    EngineInfo::builder("mock")
        .name("Mock Engine (UDS Worker)")
        .version("0.1.0")
        .engine_type(EngineType::Mock)
        .capabilities(mock_caps)
        .license("Apache-2.0")
        .weights_license("Apache-2.0")
        .commercial_use(true)
        .build()
}
