use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Audio master clock driven by samples actually delivered to the output device.
pub struct PlaybackClock {
    samples_played: AtomicU64,
    sample_rate: u32,
    paused_offset: AtomicU64,
    is_paused: AtomicBool,
    duration_samples: AtomicU64,
}

impl PlaybackClock {
    pub fn new(sample_rate: u32, duration_secs: Option<f64>) -> Self {
        let duration_samples = duration_secs
            .map(|d| (d * sample_rate as f64) as u64)
            .unwrap_or(0);
        Self {
            samples_played: AtomicU64::new(0),
            sample_rate,
            paused_offset: AtomicU64::new(0),
            is_paused: AtomicBool::new(false),
            duration_samples: AtomicU64::new(duration_samples),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn position_secs(&self) -> f64 {
        let samples = if self.is_paused.load(Ordering::Relaxed) {
            self.paused_offset.load(Ordering::Relaxed)
        } else {
            self.samples_played.load(Ordering::Relaxed)
        };
        samples as f64 / self.sample_rate as f64
    }

    pub fn on_samples_played(&self, count: u64) {
        if !self.is_paused.load(Ordering::Relaxed) {
            self.samples_played.fetch_add(count, Ordering::Relaxed);
        }
    }

    pub fn pause(&self) {
        if !self.is_paused.swap(true, Ordering::Relaxed) {
            self.paused_offset
                .store(self.samples_played.load(Ordering::Relaxed), Ordering::Relaxed);
        }
    }

    pub fn resume(&self) {
        if self.is_paused.swap(false, Ordering::Relaxed) {
            self.samples_played
                .store(self.paused_offset.load(Ordering::Relaxed), Ordering::Relaxed);
        }
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Relaxed)
    }

    pub fn seek(&self, position_secs: f64) {
        let samples = (position_secs * self.sample_rate as f64).round() as u64;
        self.samples_played.store(samples, Ordering::Relaxed);
        self.paused_offset.store(samples, Ordering::Relaxed);
    }

    pub fn duration_secs(&self) -> Option<f64> {
        let total = self.duration_samples.load(Ordering::Relaxed);
        if total == 0 {
            None
        } else {
            Some(total as f64 / self.sample_rate as f64)
        }
    }

    pub fn set_duration_secs(&self, duration_secs: f64) {
        let samples = (duration_secs * self.sample_rate as f64) as u64;
        self.duration_samples.store(samples, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_position_increases() {
        let clock = PlaybackClock::new(48_000, Some(10.0));
        clock.on_samples_played(48_000);
        assert!((clock.position_secs() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn clock_pause_freezes() {
        let clock = PlaybackClock::new(48_000, Some(10.0));
        clock.on_samples_played(96_000);
        clock.pause();
        clock.on_samples_played(48_000);
        assert!((clock.position_secs() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn clock_seek_jumps() {
        let clock = PlaybackClock::new(48_000, Some(10.0));
        clock.seek(5.0);
        assert!((clock.position_secs() - 5.0).abs() < 1e-3);
    }
}