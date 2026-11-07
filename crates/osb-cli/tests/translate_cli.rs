//! CLI integration tests for `openspeechbridge translate` and `engines`.

use assert_cmd::Command;
use std::fs;
use std::path::Path;

fn write_test_speech_wav(path: &Path) {
    let sample_rate: u32 = 16_000;
    let channels: u16 = 1;
    // 100 ms silence + 350 ms 440 Hz tone + 450 ms silence
    let mut samples_i16: Vec<i16> = vec![0; 1_600];
    for i in 0..5_600 {
        let s = ((i as f32 / sample_rate as f32) * 440.0 * 2.0 * std::f32::consts::PI).sin();
        samples_i16.push((s * 14_000.0) as i16);
    }
    samples_i16.resize(samples_i16.len() + 7_200, 0i16);

    let data_bytes = (samples_i16.len() * 2) as u32;
    let riff_size = 36u32 + data_bytes;

    let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&riff_size.to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_bytes.to_le_bytes());
    for s in samples_i16 {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    fs::write(path, bytes).unwrap();
}

#[test]
fn test_cli_translate_mock_end_to_end_json_privacy() {
    let dir = std::env::temp_dir().join(format!("osb-cli-translate-test-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let in_wav = dir.join("in.wav");
    let out_wav = dir.join("out.wav");
    write_test_speech_wav(&in_wav);

    let output = Command::cargo_bin("openspeechbridge")
        .unwrap()
        .args([
            "--format",
            "json",
            "translate",
            "--mock",
            "--input",
            in_wav.to_str().unwrap(),
            "--output",
            out_wav.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let parsed: serde_json::Value = serde_json::from_slice(&output).expect("clean JSON stdout");
    assert_eq!(parsed["utterances_translated"], 1);
    assert!(parsed["output_duration_ms"].as_u64().unwrap_or(0) > 0);
    // Privacy invariant: translated_text is omitted unless --print-text is passed.
    assert!(parsed.get("translated_text").is_none());

    let out_bytes = fs::read(&out_wav).expect("output WAV written");
    assert!(out_bytes.len() > 1_000);
    assert_eq!(&out_bytes[0..4], b"RIFF");
    assert_eq!(&out_bytes[8..12], b"WAVE");

    let _ = fs::remove_dir_all(&dir);
}
