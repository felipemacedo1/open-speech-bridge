//! List engines command.

use anyhow::Result;
use osb_daemon::engine_supervisor::EngineSupervisor;
use osb_protocol::capability::{Capability, CapabilitySet};
use osb_protocol::engine::{EngineInfo, EngineType};
use serde::Serialize;

#[derive(Serialize)]
struct EngineListOutput {
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
    commercial_use: Option<bool>,
}

pub async fn run(json_output: bool) -> Result<()> {
    // For now, show the mock engine and explain the engine system
    let mut supervisor = EngineSupervisor::new();

    // Register mock engine
    let mut mock_caps = CapabilitySet::new();
    mock_caps
        .add(Capability::StreamingStt)
        .add(Capability::StreamingTts)
        .add(Capability::CpuOnly)
        .add_stt_language("en-US")
        .add_stt_language("pt-BR")
        .add_tts_language("en-US")
        .add_tts_language("pt-BR");

    let mock_info = EngineInfo::builder("mock")
        .name("Mock Engine")
        .version("0.1.0")
        .engine_type(EngineType::Mock)
        .capabilities(mock_caps)
        .license("Apache-2.0")
        .commercial_use(true)
        .build();

    supervisor.register(mock_info);

    let engines = supervisor.list_engines();

    if json_output {
        let output = EngineListOutput {
            engines: engines
                .iter()
                .map(|e| EngineOutput {
                    id: e.id.to_string(),
                    name: e.name.clone(),
                    version: e.version.clone(),
                    engine_type: e.engine_type.to_string(),
                    capabilities: e
                        .capabilities
                        .capabilities()
                        .iter()
                        .map(|c| format!("{:?}", c))
                        .collect(),
                    license: e.license.clone(),
                    commercial_use: e.commercial_use,
                })
                .collect(),
            note: "No ML engines installed. See docs/ENGINE_PROTOCOL.md for integration.".to_string(),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Available Engines:");
        println!("══════════════════");
        println!();

        for engine in engines {
            let commercial = match engine.commercial_use {
                Some(true) => "✓ commercial OK",
                Some(false) => "✗ non-commercial",
                None => "? unknown",
            };

            println!("  {} ({})", engine.name, engine.id);
            println!("    Version:    {}", engine.version);
            println!("    Type:       {}", engine.engine_type);
            println!("    License:    {}", engine.license.as_deref().unwrap_or("unknown"));
            println!("    Commercial: {}", commercial);
            println!();
        }

        println!("Note: No ML engines are installed yet.");
        println!("The mock engine provides testing without ML models.");
        println!();
        println!("To add real engines, see:");
        println!("  - docs/ENGINE_PROTOCOL.md");
        println!("  - engines/README.md");
    }

    Ok(())
}
