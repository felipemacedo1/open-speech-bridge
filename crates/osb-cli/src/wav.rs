//! Minimal dependency-free RIFF/WAVE PCM reader and writer for CLI audio testing.

use anyhow::{bail, Context, Result};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use std::fs;
use std::path::Path;

const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Decoded WAV audio buffer converted to normalized interleaved `f32` samples.
#[derive(Debug, Clone)]
pub struct WavData {
    /// Interleaved `f32` samples in `[-1.0, 1.0]`
    pub samples: Vec<f32>,
    /// Sample rate and channel layout (sample_format set to `F32`)
    pub format: AudioFormat,
}

impl WavData {
    /// Duration of the decoded audio in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        let ch = self.format.channels.channels() as u64;
        let hz = self.format.sample_rate.hz() as u64;
        if ch == 0 || hz == 0 {
            return 0;
        }
        let frames = (self.samples.len() as u64) / ch;
        (frames * 1_000) / hz
    }
}

/// Read a RIFF/WAVE file (`PCM_16`, `PCM_24`, `PCM_32`, or `IEEE_FLOAT` `f32`) into `f32` samples.
pub fn read_wav_f32(path: &Path) -> Result<WavData> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read WAV file {}", path.display()))?;
    parse_wav_bytes(&bytes)
}

/// Parse RIFF/WAVE bytes into normalized interleaved `f32` samples.
pub fn parse_wav_bytes(bytes: &[u8]) -> Result<WavData> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("invalid RIFF/WAVE file header");
    }

    let mut offset = 12usize;
    let mut fmt_info: Option<(u16, u16, u32, u16)> = None;
    let mut data_slice: Option<&[u8]> = None;

    while offset + 8 <= bytes.len() {
        let chunk_id = &bytes[offset..offset + 4];
        let chunk_len = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let body_start = offset + 8;
        let body_end = body_start.saturating_add(chunk_len).min(bytes.len());
        let body = &bytes[body_start..body_end];

        if chunk_id == b"fmt " {
            fmt_info = Some(parse_fmt_chunk(body)?);
        } else if chunk_id == b"data" {
            data_slice = Some(body);
            if fmt_info.is_some() {
                break;
            }
        }

        // RIFF chunks are word-aligned (padded to even byte boundary).
        let padded = chunk_len + (chunk_len % 2);
        offset = match body_start.checked_add(padded) {
            Some(next) => next,
            None => break,
        };
    }

    let (audio_fmt, channels, sample_rate, bits_per_sample) =
        fmt_info.context("WAV file missing 'fmt ' chunk")?;
    let raw_data = data_slice.context("WAV file missing 'data' chunk")?;

    let layout = match channels {
        1 => ChannelLayout::Mono,
        2 => ChannelLayout::Stereo,
        other => bail!("unsupported WAV channel count: {other} (expected mono or stereo)"),
    };

    let samples = decode_pcm_samples(raw_data, audio_fmt, bits_per_sample)?;
    Ok(WavData {
        samples,
        format: AudioFormat {
            sample_rate: SampleRate::new(sample_rate),
            sample_format: SampleFormat::F32,
            channels: layout,
        },
    })
}

fn parse_fmt_chunk(body: &[u8]) -> Result<(u16, u16, u32, u16)> {
    if body.len() < 16 {
        bail!("WAV 'fmt ' chunk too short ({} bytes)", body.len());
    }
    let mut audio_fmt = u16::from_le_bytes([body[0], body[1]]);
    let channels = u16::from_le_bytes([body[2], body[3]]);
    let sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
    let bits_per_sample = u16::from_le_bytes([body[14], body[15]]);

    if audio_fmt == WAVE_FORMAT_EXTENSIBLE && body.len() >= 26 {
        audio_fmt = u16::from_le_bytes([body[24], body[25]]);
    }
    Ok((audio_fmt, channels, sample_rate, bits_per_sample))
}

fn decode_pcm_samples(raw: &[u8], audio_fmt: u16, bits_per_sample: u16) -> Result<Vec<f32>> {
    match (audio_fmt, bits_per_sample) {
        (WAVE_FORMAT_PCM, 16) => Ok(raw
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32_768.0)
            .collect()),
        (WAVE_FORMAT_PCM, 24) => Ok(raw
            .chunks_exact(3)
            .map(|b| {
                let sign = if b[2] & 0x80 != 0 { 0xFF } else { 0x00 };
                let v = i32::from_le_bytes([b[0], b[1], b[2], sign]);
                v as f32 / 8_388_608.0
            })
            .collect()),
        (WAVE_FORMAT_PCM, 32) => Ok(raw
            .chunks_exact(4)
            .map(|b| (i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f64 / 2_147_483_648.0) as f32)
            .collect()),
        (WAVE_FORMAT_IEEE_FLOAT, 32) => Ok(raw
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()),
        _ => bail!(
            "unsupported WAV sample encoding (format=0x{audio_fmt:04x}, bits={bits_per_sample})"
        ),
    }
}

/// Write interleaved `f32` samples as a standard 16-bit PCM (`PCM_16`) RIFF/WAVE file.
pub fn write_wav_pcm16(path: &Path, samples: &[f32], format: AudioFormat) -> Result<()> {
    let channels = u16::from(format.channels.channels());
    let sample_rate = format.sample_rate.hz();
    let bytes_per_sample: u16 = 2;
    let block_align = channels * bytes_per_sample;
    let byte_rate = sample_rate * u32::from(block_align);
    let data_bytes_len = u32::try_from(samples.len().saturating_mul(2))
        .context("WAV output exceeds 4 GiB RIFF size limit")?;
    let riff_chunk_size = 36u32.saturating_add(data_bytes_len);

    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_chunk_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");

    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());

    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes_len.to_le_bytes());
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let pcm16 = (clamped * 32_767.0).round() as i16;
        out.extend_from_slice(&pcm16.to_le_bytes());
    }

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(path, out).with_context(|| format!("failed to write WAV file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wav_pcm16_write_and_read_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("osb-wav-test-{}.wav", std::process::id()));
        let fmt = AudioFormat {
            sample_rate: SampleRate::SPEECH_16K,
            sample_format: SampleFormat::F32,
            channels: ChannelLayout::Mono,
        };
        let input = vec![0.0f32, 0.25, -0.25, 0.5, -0.5, 0.9];
        write_wav_pcm16(&tmp, &input, fmt).unwrap();

        let decoded = read_wav_f32(&tmp).unwrap();
        let _ = fs::remove_file(&tmp);

        assert_eq!(decoded.format.sample_rate, SampleRate::SPEECH_16K);
        assert_eq!(decoded.format.channels, ChannelLayout::Mono);
        assert_eq!(decoded.samples.len(), input.len());
        for (a, b) in input.iter().zip(decoded.samples.iter()) {
            assert!((a - b).abs() < 0.001, "expected {a} ~= {b}");
        }
    }
}
