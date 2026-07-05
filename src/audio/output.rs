use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;

use crate::audio::clock::PlaybackClock;
use crate::error::{PlayerError, Result};

struct RingBuffer {
    data: Vec<f32>,
    read_pos: usize,
    write_pos: usize,
    capacity: usize,
}

impl RingBuffer {
    fn new(capacity_samples: usize) -> Self {
        Self {
            data: vec![0.0; capacity_samples],
            read_pos: 0,
            write_pos: 0,
            capacity: capacity_samples,
        }
    }

    fn available_read(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            self.capacity - self.read_pos + self.write_pos
        }
    }

    fn available_write(&self) -> usize {
        self.capacity - self.available_read() - 1
    }

    fn write(&mut self, samples: &[f32]) -> usize {
        let mut written = 0;
        let mut src_idx = 0;
        while src_idx < samples.len() && self.available_write() > 0 {
            let contiguous = if self.write_pos >= self.read_pos {
                self.capacity - self.write_pos
            } else {
                self.read_pos - self.write_pos - 1
            };
            let avail = self.available_write();
            let to_write = (samples.len() - src_idx).min(contiguous).min(avail);
            let end = self.write_pos + to_write;
            self.data[self.write_pos..end].copy_from_slice(&samples[src_idx..src_idx + to_write]);
            self.write_pos = end % self.capacity;
            src_idx += to_write;
            written += to_write;
        }
        written
    }

    fn read(&mut self, out: &mut [f32], volume: f32) -> usize {
        let mut read = 0;
        for sample in out.iter_mut() {
            if self.available_read() == 0 {
                *sample = 0.0;
            } else {
                *sample = self.data[self.read_pos] * volume;
                self.read_pos = (self.read_pos + 1) % self.capacity;
                read += 1;
            }
        }
        read
    }

    fn clear(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
    }
}

pub struct AudioOutput {
    _stream: cpal::Stream,
    ring: Arc<Mutex<RingBuffer>>,
    channels: u16,
    sample_rate: u32,
    clock: Arc<PlaybackClock>,
    paused: Arc<std::sync::atomic::AtomicBool>,
    volume: Arc<std::sync::atomic::AtomicU32>,
}

impl AudioOutput {
    pub fn new(sample_rate: u32, channels: u16, clock: Arc<PlaybackClock>) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| PlayerError::AudioOutput("no output device".into()))?;

        let config = device
            .default_output_config()
            .map_err(|e| PlayerError::AudioOutput(e.to_string()))?;

        let channels_usize = channels as usize;
        // ~500ms of interleaved samples to absorb decode jitter.
        let ring = Arc::new(Mutex::new(RingBuffer::new(
            sample_rate as usize * channels_usize / 2,
        )));
        let ring_cb = ring.clone();
        let clock_cb = clock.clone();
        let paused = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let paused_cb = paused.clone();
        let volume = Arc::new(std::sync::atomic::AtomicU32::new(1000));
        let volume_cb = volume.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => {
                let stream_config: cpal::StreamConfig = config.clone().into();
                device
                    .build_output_stream(
                        &stream_config,
                        move |data: &mut [f32], _| {
                            if paused_cb.load(std::sync::atomic::Ordering::Relaxed) {
                                data.fill(0.0);
                                return;
                            }
                            let vol =
                                volume_cb.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
                            let read = ring_cb.lock().read(data, vol);
                            if read > 0 {
                                let frames = read / channels_usize;
                                clock_cb.on_samples_played(frames as u64);
                            }
                        },
                        |_| {},
                        None,
                    )
                    .map_err(|e| PlayerError::AudioOutput(e.to_string()))?
            }
            other => {
                return Err(PlayerError::AudioOutput(format!(
                    "unsupported sample format: {other:?}"
                )));
            }
        };

        stream
            .play()
            .map_err(|e| PlayerError::AudioOutput(e.to_string()))?;

        Ok(Self {
            _stream: stream,
            ring,
            channels,
            sample_rate,
            clock,
            paused,
            volume,
        })
    }

    pub fn write(&self, samples: &[f32]) -> Result<()> {
        self.ring.lock().write(samples);
        Ok(())
    }

    pub fn pause(&self) {
        self.paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.clock.pause();
    }

    pub fn resume(&self) {
        self.paused
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.clock.resume();
    }

    pub fn clear(&self) {
        self.ring.lock().clear();
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_volume(&self, level: f32) {
        let permille = (level.clamp(0.0, 1.0) * 1000.0).round() as u32;
        self.volume
            .store(permille, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        self.volume.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_batch_write_read() {
        let mut ring = RingBuffer::new(16);
        let samples: Vec<f32> = (0..10).map(|i| i as f32).collect();
        assert_eq!(ring.write(&samples), 10);
        assert_eq!(ring.available_read(), 10);

        let mut out = vec![0.0; 10];
        assert_eq!(ring.read(&mut out, 1.0), 10);
        assert_eq!(out, samples);
    }

    #[test]
    fn ring_buffer_wraps_around() {
        let mut ring = RingBuffer::new(8);
        assert_eq!(ring.write(&[1.0, 2.0, 3.0, 4.0, 5.0]), 5);
        let mut partial = [0.0; 3];
        assert_eq!(ring.read(&mut partial, 1.0), 3);
        assert_eq!(partial, [1.0, 2.0, 3.0]);
        assert_eq!(ring.write(&[6.0, 7.0, 8.0, 9.0, 10.0]), 5);
        let mut rest = [0.0; 7];
        assert_eq!(ring.read(&mut rest, 1.0), 7);
        assert_eq!(rest, [4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]);
    }
}