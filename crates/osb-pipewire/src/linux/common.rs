//! Common utilities for PipeWire stream implementations.
//!
//! This module provides shared functionality used by capture, playback,
//! and virtual device implementations to avoid code duplication.
//!
//! # Real-time Safety
//!
//! Functions in this module that are called from audio callbacks must be
//! real-time safe: no allocations, no locks, no blocking I/O.

use crate::error::{PipeWireError, Result};
use osb_core::audio::{AudioFormat, SampleFormat};
use pipewire as pw;
use pw::spa;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Stack buffer size for batch processing in real-time callbacks.
/// 512 samples provides a good balance between efficiency and stack usage.
pub const RT_BATCH_SIZE: usize = 512;

/// Convert F32LE bytes to f32 samples in batches.
///
/// # Real-time Safety
///
/// This function is real-time safe:
/// - No allocations (writes to provided output buffer)
/// - No locks
/// - No I/O
///
/// # Arguments
///
/// * `audio_bytes` - Input bytes in F32LE format
/// * `output` - Output buffer for f32 samples (must be at least `audio_bytes.len() / 4`)
///
/// # Returns
///
/// Number of samples converted.
#[inline]
pub fn bytes_to_f32_samples(audio_bytes: &[u8], output: &mut [f32]) -> usize {
    let num_samples = audio_bytes.len() / 4;
    let samples_to_convert = num_samples.min(output.len());

    for (i, sample_out) in output.iter_mut().take(samples_to_convert).enumerate() {
        let idx = i * 4;
        if idx + 4 <= audio_bytes.len() {
            let bytes: [u8; 4] = [
                audio_bytes[idx],
                audio_bytes[idx + 1],
                audio_bytes[idx + 2],
                audio_bytes[idx + 3],
            ];
            *sample_out = f32::from_le_bytes(bytes);
        }
    }

    samples_to_convert
}

/// Convert f32 samples to F32LE bytes.
///
/// # Real-time Safety
///
/// This function is real-time safe:
/// - No allocations (writes to provided output buffer)
/// - No locks
/// - No I/O
///
/// # Arguments
///
/// * `samples` - Input f32 samples
/// * `output` - Output byte buffer (must be at least `samples.len() * 4`)
///
/// # Returns
///
/// Number of bytes written.
#[inline]
pub fn f32_samples_to_bytes(samples: &[f32], output: &mut [u8]) -> usize {
    let bytes_available = output.len();
    let samples_to_convert = (bytes_available / 4).min(samples.len());

    for (i, &sample) in samples.iter().take(samples_to_convert).enumerate() {
        let bytes = sample.to_le_bytes();
        let idx = i * 4;
        output[idx] = bytes[0];
        output[idx + 1] = bytes[1];
        output[idx + 2] = bytes[2];
        output[idx + 3] = bytes[3];
    }

    samples_to_convert * 4
}

/// Build SPA audio format parameters for stream connection.
///
/// # Arguments
///
/// * `format` - Audio format specification
///
/// # Returns
///
/// Serialized POD bytes for the audio format.
pub fn build_audio_format_pod(format: AudioFormat) -> Result<Vec<u8>> {
    let spa_format = match format.sample_format {
        SampleFormat::F32 => spa::param::audio::AudioFormat::F32LE,
        SampleFormat::I16 => spa::param::audio::AudioFormat::S16LE,
        SampleFormat::I32 => spa::param::audio::AudioFormat::S32LE,
        _ => spa::param::audio::AudioFormat::F32LE,
    };

    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa_format);
    audio_info.set_rate(format.sample_rate.hz());
    audio_info.set_channels(format.channels.channels() as u32);

    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: pw::spa::sys::SPA_TYPE_OBJECT_Format,
            id: pw::spa::sys::SPA_PARAM_EnumFormat,
            properties: audio_info.into(),
        }),
    )
    .map_err(|e| PipeWireError::Internal(format!("failed to serialize audio format: {:?}", e)))?
    .0
    .into_inner();

    Ok(values)
}

/// Set up a periodic timer to check the is_running flag and quit the main loop.
///
/// # Arguments
///
/// * `mainloop` - The PipeWire main loop
/// * `is_running` - Shared flag to check for stop signal
/// * `interval_ms` - Timer interval in milliseconds (default 100)
///
/// # Returns
///
/// The timer source (must be kept alive while the main loop runs).
pub fn setup_stop_timer(
    mainloop: &pw::main_loop::MainLoop,
    is_running: Arc<AtomicBool>,
    interval_ms: u64,
) -> Result<pw::loop_::TimerSource<'_>> {
    let mainloop_weak = mainloop.downgrade();
    let timer = mainloop.loop_().add_timer(move |_| {
        if !is_running.load(Ordering::Acquire) {
            if let Some(ml) = mainloop_weak.upgrade() {
                ml.quit();
            }
        }
    });

    let duration = std::time::Duration::from_millis(interval_ms);
    timer
        .update_timer(Some(duration), Some(duration))
        .into_result()
        .map_err(|e| PipeWireError::Internal(format!("failed to set timer: {:?}", e)))?;

    Ok(timer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_to_f32_samples() {
        // Create F32LE bytes for [1.0, -1.0, 0.5]
        let bytes: Vec<u8> = [1.0f32, -1.0f32, 0.5f32]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();

        let mut output = [0.0f32; 4];
        let converted = bytes_to_f32_samples(&bytes, &mut output);

        assert_eq!(converted, 3);
        assert!((output[0] - 1.0).abs() < f32::EPSILON);
        assert!((output[1] - (-1.0)).abs() < f32::EPSILON);
        assert!((output[2] - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_bytes_to_f32_samples_partial() {
        // 2 complete samples + incomplete
        let bytes: Vec<u8> = [1.0f32, 2.0f32]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .chain([0u8, 0u8]) // Incomplete sample
            .collect();

        let mut output = [0.0f32; 3];
        let converted = bytes_to_f32_samples(&bytes, &mut output);

        assert_eq!(converted, 2);
        assert!((output[0] - 1.0).abs() < f32::EPSILON);
        assert!((output[1] - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_f32_samples_to_bytes() {
        let samples = [1.0f32, -1.0f32, 0.5f32];
        let mut output = [0u8; 12];

        let written = f32_samples_to_bytes(&samples, &mut output);

        assert_eq!(written, 12);

        // Verify by converting back
        let mut verify = [0.0f32; 3];
        bytes_to_f32_samples(&output, &mut verify);
        assert!((verify[0] - 1.0).abs() < f32::EPSILON);
        assert!((verify[1] - (-1.0)).abs() < f32::EPSILON);
        assert!((verify[2] - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_f32_samples_to_bytes_limited_output() {
        let samples = [1.0f32, 2.0f32, 3.0f32];
        let mut output = [0u8; 8]; // Only room for 2 samples

        let written = f32_samples_to_bytes(&samples, &mut output);

        assert_eq!(written, 8);
    }

    #[test]
    fn test_roundtrip_conversion() {
        let original = [0.1f32, -0.5f32, 0.9f32, -0.9f32];
        let mut bytes = [0u8; 16];
        let mut result = [0.0f32; 4];

        f32_samples_to_bytes(&original, &mut bytes);
        bytes_to_f32_samples(&bytes, &mut result);

        for (o, r) in original.iter().zip(result.iter()) {
            assert!((o - r).abs() < f32::EPSILON);
        }
    }
}
