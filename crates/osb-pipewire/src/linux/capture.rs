//! Audio capture stream implementation.

use super::context::PipeWireContext;
use crate::error::Result;
use osb_audio::buffer::{AudioRingBuffer, AudioRingProducer};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use tracing::info;

/// Audio capture stream for recording from a device.
pub struct CaptureStream {
    producer: AudioRingProducer,
    format: AudioFormat,
    device_id: String,
    is_running: bool,
}

impl CaptureStream {
    /// Create a new capture stream for the specified device.
    ///
    /// # Arguments
    ///
    /// * `ctx` - PipeWire context
    /// * `device_id` - Device identifier (node ID or name)
    /// * `buffer_frames` - Size of the ring buffer in frames
    pub fn new(
        ctx: &PipeWireContext,
        device_id: &str,
        buffer_frames: u32,
    ) -> Result<(Self, osb_audio::buffer::AudioRingConsumer)> {
        let format = AudioFormat {
            sample_rate: SampleRate::PRO_48K,
            sample_format: SampleFormat::F32,
            channels: ChannelLayout::Stereo,
        };

        let (producer, consumer) = AudioRingBuffer::new(buffer_frames, format.channels.channels());

        let stream = Self {
            producer,
            format,
            device_id: device_id.to_string(),
            is_running: false,
        };

        Ok((stream, consumer))
    }

    /// Get the audio format of this stream.
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Get the device ID.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Check if the stream is currently capturing.
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Start capturing audio.
    ///
    /// This begins the audio capture. Samples will be pushed to the
    /// ring buffer and can be read from the consumer returned by `new()`.
    pub fn start(&mut self) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        // In a full implementation, we would:
        // 1. Create a PipeWire stream with the appropriate format
        // 2. Connect to the target device
        // 3. Set up the process callback to push samples to the ring buffer
        //
        // For now, this is a placeholder that demonstrates the structure.

        info!(device = %self.device_id, "starting capture stream");
        self.is_running = true;
        Ok(())
    }

    /// Stop capturing audio.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }

        info!(device = %self.device_id, "stopping capture stream");
        self.is_running = false;
        Ok(())
    }

    /// Push samples directly (for testing or manual feeding).
    pub fn push_samples(&mut self, samples: &[f32]) -> usize {
        self.producer.push(samples)
    }
}

impl Drop for CaptureStream {
    fn drop(&mut self) {
        if self.is_running {
            let _ = self.stop();
        }
    }
}

/// Builder for CaptureStream with configuration options.
pub struct CaptureStreamBuilder {
    device_id: String,
    buffer_frames: u32,
    sample_rate: SampleRate,
    channels: ChannelLayout,
}

impl CaptureStreamBuilder {
    /// Create a new builder for the specified device.
    pub fn new(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            buffer_frames: 4096,
            sample_rate: SampleRate::PRO_48K,
            channels: ChannelLayout::Stereo,
        }
    }

    /// Set the ring buffer size in frames.
    pub fn buffer_frames(mut self, frames: u32) -> Self {
        self.buffer_frames = frames;
        self
    }

    /// Set the sample rate.
    pub fn sample_rate(mut self, rate: SampleRate) -> Self {
        self.sample_rate = rate;
        self
    }

    /// Set the channel layout.
    pub fn channels(mut self, channels: ChannelLayout) -> Self {
        self.channels = channels;
        self
    }

    /// Build the capture stream.
    pub fn build(
        self,
        ctx: &PipeWireContext,
    ) -> Result<(CaptureStream, osb_audio::buffer::AudioRingConsumer)> {
        CaptureStream::new(ctx, &self.device_id, self.buffer_frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require PipeWire to be running
    // They are marked as ignored by default

    #[test]
    #[ignore]
    fn test_capture_stream_creation() {
        let ctx = PipeWireContext::new().unwrap();
        let (stream, _consumer) = CaptureStream::new(&ctx, "default", 4096).unwrap();
        assert!(!stream.is_running());
    }
}
