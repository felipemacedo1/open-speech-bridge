//! Virtual audio device implementation.
//!
//! Virtual devices allow OpenSpeechBridge to inject audio into applications
//! (virtual microphone) or capture audio from applications (virtual sink).

use super::context::PipeWireContext;
use crate::error::Result;
use osb_audio::buffer::{AudioRingBuffer, AudioRingConsumer, AudioRingProducer};
use osb_core::audio::{AudioFormat, ChannelLayout, SampleFormat, SampleRate};
use tracing::info;

/// A virtual microphone that appears as an audio input device.
///
/// Applications like Discord, Zoom, or Google Meet can select this
/// virtual device as their microphone input. Audio written to this
/// device will be "heard" by those applications.
pub struct VirtualMicrophone {
    name: String,
    consumer: AudioRingConsumer,
    format: AudioFormat,
    is_running: bool,
    node_id: Option<u32>,
}

impl VirtualMicrophone {
    /// Create a new virtual microphone.
    ///
    /// # Arguments
    ///
    /// * `ctx` - PipeWire context
    /// * `name` - Name that will appear in application device lists
    /// * `buffer_frames` - Ring buffer size in frames
    ///
    /// # Returns
    ///
    /// A tuple of (virtual mic, producer). Write translated audio to the
    /// producer, and applications will receive it from the virtual mic.
    pub fn new(
        _ctx: &PipeWireContext,
        name: &str,
        buffer_frames: u32,
    ) -> Result<(Self, AudioRingProducer)> {
        let format = AudioFormat {
            sample_rate: SampleRate::PRO_48K,
            sample_format: SampleFormat::F32,
            channels: ChannelLayout::Stereo,
        };

        let (producer, consumer) = AudioRingBuffer::new(buffer_frames, format.channels.channels());

        info!(name = %name, "creating virtual microphone");

        let vmic = Self {
            name: name.to_string(),
            consumer,
            format,
            is_running: false,
            node_id: None,
        };

        Ok((vmic, producer))
    }

    /// Get the name of this virtual microphone.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the audio format.
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Get the PipeWire node ID (if registered).
    pub fn node_id(&self) -> Option<u32> {
        self.node_id
    }

    /// Check if the virtual microphone is active.
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Start the virtual microphone.
    ///
    /// This registers the device with PipeWire and makes it visible
    /// to applications.
    pub fn start(&mut self) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        // In a full implementation, we would:
        // 1. Create a PipeWire node with media.class = "Audio/Source/Virtual"
        // 2. Register appropriate metadata so it appears correctly in apps
        // 3. Set up the process callback to serve audio from our buffer

        info!(name = %self.name, "starting virtual microphone");
        self.is_running = true;
        Ok(())
    }

    /// Stop the virtual microphone.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }

        info!(name = %self.name, "stopping virtual microphone");
        self.is_running = false;
        self.node_id = None;
        Ok(())
    }

    /// Read samples to provide to applications (called by audio callback).
    pub fn read_samples(&mut self, output: &mut [f32]) -> usize {
        self.consumer.pop_or_silence(output)
    }

    /// Get buffer occupancy.
    pub fn buffer_occupancy(&self) -> f32 {
        self.consumer.occupancy().fill_ratio
    }
}

impl Drop for VirtualMicrophone {
    fn drop(&mut self) {
        if self.is_running {
            let _ = self.stop();
        }
    }
}

/// A virtual audio sink that captures audio from applications.
///
/// This allows capturing the audio output of a specific application
/// or the entire desktop audio for translation of incoming speech.
pub struct VirtualSink {
    name: String,
    producer: AudioRingProducer,
    format: AudioFormat,
    is_running: bool,
    node_id: Option<u32>,
}

impl VirtualSink {
    /// Create a new virtual sink.
    ///
    /// # Arguments
    ///
    /// * `ctx` - PipeWire context
    /// * `name` - Name for the virtual sink
    /// * `buffer_frames` - Ring buffer size
    ///
    /// # Returns
    ///
    /// A tuple of (sink, consumer). Audio from applications will be
    /// available from the consumer for processing.
    pub fn new(
        _ctx: &PipeWireContext,
        name: &str,
        buffer_frames: u32,
    ) -> Result<(Self, AudioRingConsumer)> {
        let format = AudioFormat {
            sample_rate: SampleRate::PRO_48K,
            sample_format: SampleFormat::F32,
            channels: ChannelLayout::Stereo,
        };

        let (producer, consumer) = AudioRingBuffer::new(buffer_frames, format.channels.channels());

        info!(name = %name, "creating virtual sink");

        let sink = Self {
            name: name.to_string(),
            producer,
            format,
            is_running: false,
            node_id: None,
        };

        Ok((sink, consumer))
    }

    /// Get the name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the format.
    pub fn format(&self) -> AudioFormat {
        self.format
    }

    /// Get node ID.
    pub fn node_id(&self) -> Option<u32> {
        self.node_id
    }

    /// Check if running.
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Start the virtual sink.
    pub fn start(&mut self) -> Result<()> {
        if self.is_running {
            return Ok(());
        }

        info!(name = %self.name, "starting virtual sink");
        self.is_running = true;
        Ok(())
    }

    /// Stop the virtual sink.
    pub fn stop(&mut self) -> Result<()> {
        if !self.is_running {
            return Ok(());
        }

        info!(name = %self.name, "stopping virtual sink");
        self.is_running = false;
        self.node_id = None;
        Ok(())
    }

    /// Write samples received from applications (called by audio callback).
    pub fn write_samples(&mut self, samples: &[f32]) -> usize {
        self.producer.push(samples)
    }
}

impl Drop for VirtualSink {
    fn drop(&mut self) {
        if self.is_running {
            let _ = self.stop();
        }
    }
}

/// Default name for the OpenSpeechBridge virtual microphone.
pub const DEFAULT_VIRTUAL_MIC_NAME: &str = "OpenSpeechBridge Virtual Microphone";

/// Default name for the OpenSpeechBridge virtual sink.
pub const DEFAULT_VIRTUAL_SINK_NAME: &str = "OpenSpeechBridge Virtual Sink";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires PipeWire
    fn test_virtual_mic_creation() {
        let ctx = PipeWireContext::new().unwrap();
        let (vmic, _producer) = VirtualMicrophone::new(&ctx, "Test Mic", 4096).unwrap();
        assert_eq!(vmic.name(), "Test Mic");
        assert!(!vmic.is_running());
    }
}
