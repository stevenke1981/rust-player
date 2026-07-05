mod clock;
mod decoder;
mod output;

pub use clock::PlaybackClock;
pub use decoder::{AudioBuffer, AudioDecoder};
pub use output::AudioOutput;

use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::error::Result;

pub struct AudioPlayer {
    decoder: AudioDecoder,
    output: AudioOutput,
    clock: Arc<PlaybackClock>,
}

impl AudioPlayer {
    pub fn open(path: &Path) -> Result<Self> {
        let decoder = AudioDecoder::open(path)?;
        let sample_rate = decoder.sample_rate();
        let channels = decoder.channels();
        let duration = decoder.duration_secs();
        let clock = Arc::new(PlaybackClock::new(sample_rate, duration));
        if let Some(d) = duration {
            clock.set_duration_secs(d);
        }
        let output = AudioOutput::new(sample_rate, channels, clock.clone())?;
        Ok(Self {
            decoder,
            output,
            clock,
        })
    }

    pub fn clock(&self) -> Arc<PlaybackClock> {
        self.clock.clone()
    }

    pub fn play_blocking(&mut self) -> Result<()> {
        while let Some(buffer) = self.decoder.decode_next()? {
            self.output.write(&buffer.samples)?;
            thread::sleep(Duration::from_millis(1));
        }
        thread::sleep(Duration::from_millis(500));
        Ok(())
    }

    pub fn play_with_progress(&mut self, interval: Duration) -> Result<()> {
        let mut last_report = std::time::Instant::now();
        while let Some(buffer) = self.decoder.decode_next()? {
            self.output.write(&buffer.samples)?;
            if last_report.elapsed() >= interval {
                let pos = self.clock.position_secs();
                let dur = self
                    .clock
                    .duration_secs()
                    .map(|d| format!("{d:.2}"))
                    .unwrap_or_else(|| "?".into());
                log::info!("progress: {pos:.3} / {dur} s");
                last_report = std::time::Instant::now();
            }
        }
        thread::sleep(Duration::from_millis(500));
        Ok(())
    }

    pub fn seek(&mut self, position_secs: f64) -> Result<()> {
        self.output.clear();
        self.decoder.seek(position_secs)?;
        self.clock.seek(position_secs);
        Ok(())
    }

    pub fn pause(&self) {
        self.output.pause();
    }

    pub fn resume(&self) {
        self.output.resume();
    }
}