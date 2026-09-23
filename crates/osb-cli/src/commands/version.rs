//! Version information command.

use anyhow::Result;
use serde::Serialize;

#[derive(Serialize)]
struct VersionInfo {
    name: &'static str,
    version: &'static str,
    target: &'static str,
    rustc: &'static str,
    profile: &'static str,
    features: Vec<&'static str>,
}

pub fn run(json_output: bool) -> Result<()> {
    let info = VersionInfo {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        target: env!("TARGET"),
        rustc: env!("RUSTC_VERSION"),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        features: vec![],
    };

    if json_output {
        println!("{}", serde_json::to_string_pretty(&info)?);
    } else {
        println!("OpenSpeechBridge {}", info.version);
        println!();
        println!("Target:  {}", info.target);
        println!("Rustc:   {}", info.rustc);
        println!("Profile: {}", info.profile);
        println!();
        println!("Repository: https://github.com/user/open-speech-bridge");
        println!("License:    Apache-2.0");
    }

    Ok(())
}
