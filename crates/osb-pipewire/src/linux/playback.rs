//! Audio playback stream implementation.

use super::context::PipeWireContext;
use crate::error::Result;
use osb_audio::buffer::{AudioRingBuffer, AudioRingConsumer};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use tracing::info;

/// Audio playback stream for outputting to a device.
pub struct PlaybackStream {
    consumer: AudioRingConsumer,
    format: AudioFormat,
    device_id: String,
    is_running: bool,
}

impl PlaybackStream {
    /// Create a new playback stream for the specified device.
    ///
    /// # Arguments
    ///
    /// * `ctx` - PipeWire context
    /// * `device_id` - Device identifier (node ID or name)
    /// * `buffer_frames` - Size of the ring buffer in frames
    ///
    /// # Returns
    ///
    /// A tuple of (stream, producer). Write samples to the producer,
    /// and the stream will play them.
    pub fn new(
        _ctx: &PipeWireContext,
        device_id: &str,
        buffer_frames: u32,
    ) -> Result<(Self, osb_audio::buffer::AudioRingProducer)> {
        let format = AudioFormat {
            sample_rate: SampleRate::PRO_48K,
            sample_format: SampleFormat::F32,
            channels: ChannelLayout::Stereo,
        };

        let (producer, consumer) = AudioRingBuffer::new(buffer_frames, format.channels.channels());

        let stream = Self {
            consumer,
            format,
            device_id: device_id.to_string(),
            is_running: false,
        };

        Ok((stream, producer))
    }

    /// Get the audio format of this stream.
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Get the device ID.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Check if the stream is currently playing.
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Start playback.
    pub fn start(&mut self) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        info!(device = %self.device_id, "starting playback stream");
        self.is_running = true;
        Ok(())
    }

    /// Stop playback.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }

        info!(device = %self.device_id, "stopping playback stream");
        self.is_running = false;
        Ok(())
    }

    /// Read samples for playback (called by audio callback).
    pub fn read_samples(&mut self, output: &mut [f32]) -> usize {
        self.consumer.pop_or_silence(output)
    }

    /// Get current buffer occupancy.
    pub fn buffer_occupancy(&self) -> f32 {
        self.consumer.occupancy().fill_ratio
    }
}

impl Drop for PlaybackStream {
    fn drop(&mut self) {
        if self.is_running {
            let _ = self.stop();
        }
    }
}

/// Builder for PlaybackStream.
pub struct PlaybackStreamBuilder {
    device_id: String,
    buffer_frames: u32,
    sample_rate: SampleRate,
    channels: ChannelLayout,
}

impl PlaybackStreamBuilder {
    /// Create a new builder.
    pub fn new(device_id: impl Into<String>) -> Self {
        Self {
            device_id: device_id.into(),
            buffer_frames: 4096,
            sample_rate: SampleRate::PRO_48K,
            channels: ChannelLayout::Stereo,
        }
    }

    /// Set buffer size.
    pub fn buffer_frames(mut self, frames: u32) -> Self {
        self.buffer_frames = frames;
        self
    }

    /// Set sample rate.
    pub fn sample_rate(mut self, rate: SampleRate) -> Self {
        self.sample_rate = rate;
        self
    }

    /// Set channels.
    pub fn channels(mut self, channels: ChannelLayout) -> Self {
        self.channels = channels;
        self
    }

    /// Build the stream.
    pub fn build(
        self,
        ctx: &PipeWireContext,
    ) -> Result<(PlaybackStream, osb_audio::buffer::AudioRingProducer)> {
        PlaybackStream::new(ctx, &self.device_id, self.buffer_frames)
    }
}
