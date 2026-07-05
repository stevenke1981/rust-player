use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::audio::clock::PlaybackClock;
use crate::audio::output::AudioOutput;
use crate::error::{PlayerError, Result};

/// Null audio sink for environments without an output device (e.g. remote desktop).
/// Samples are discarded; the player drives the master clock from wall time.
pub struct VirtualAudioOutput {
    channels: u16,
    sample_rate: u32,
    #[allow(dead_code)]
    clock: Arc<PlaybackClock>,
    paused: AtomicBool,
    volume: AtomicU32,
}

impl VirtualAudioOutput {
    pub fn new(sample_rate: u32, channels: u16, clock: Arc<PlaybackClock>) -> Self {
        Self {
            channels,
            sample_rate,
            clock,
            paused: AtomicBool::new(false),
            volume: AtomicU32::new(1000),
        }
    }

    pub fn write(&self, samples: &[f32]) -> Result<()> {
        let _ = (self.paused.load(Ordering::Relaxed), samples.len());
        Ok(())
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::Relaxed);
    }

    pub fn clear(&self) {}

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_volume(&self, level: f32) {
        let permille = (level.clamp(0.0, 1.0) * 1000.0).round() as u32;
        self.volume.store(permille, Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        self.volume.load(Ordering::Relaxed) as f32 / 1000.0
    }
}

pub enum AudioSink {
    Device(AudioOutput),
    Virtual(VirtualAudioOutput),
}

impl AudioSink {
    pub fn try_new(sample_rate: u32, channels: u16, clock: Arc<PlaybackClock>) -> Result<Self> {
        match AudioOutput::new(sample_rate, channels, clock.clone()) {
            Ok(device) => {
                log::info!("audio output: device");
                Ok(Self::Device(device))
            }
            Err(PlayerError::AudioOutput(msg)) => {
                log::warn!("no audio device ({msg}); using virtual audio sink");
                Ok(Self::Virtual(VirtualAudioOutput::new(
                    sample_rate, channels, clock,
                )))
            }
            Err(e) => Err(e),
        }
    }

    pub fn is_virtual(&self) -> bool {
        matches!(self, Self::Virtual(_))
    }

    pub fn write(&self, samples: &[f32]) -> Result<()> {
        match self {
            Self::Device(o) => o.write(samples),
            Self::Virtual(o) => o.write(samples),
        }
    }

    pub fn pause(&self) {
        match self {
            Self::Device(o) => o.pause(),
            Self::Virtual(o) => o.pause(),
        }
    }

    pub fn resume(&self) {
        match self {
            Self::Device(o) => o.resume(),
            Self::Virtual(o) => o.resume(),
        }
    }

    pub fn clear(&self) {
        match self {
            Self::Device(o) => o.clear(),
            Self::Virtual(o) => o.clear(),
        }
    }

    pub fn channels(&self) -> u16 {
        match self {
            Self::Device(o) => o.channels(),
            Self::Virtual(o) => o.channels(),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        match self {
            Self::Device(o) => o.sample_rate(),
            Self::Virtual(o) => o.sample_rate(),
        }
    }

    pub fn set_volume(&self, level: f32) {
        match self {
            Self::Device(o) => o.set_volume(level),
            Self::Virtual(o) => o.set_volume(level),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_sink_does_not_advance_clock() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(10.0)));
        let sink = VirtualAudioOutput::new(48_000, 2, clock.clone());
        sink.write(&vec![0.0f32; 96_000]).unwrap();
        assert!((clock.position_secs()).abs() < 1e-6);
    }
}