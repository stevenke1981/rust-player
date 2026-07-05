use std::collections::VecDeque;
use std::sync::Arc;

use crate::audio::PlaybackClock;
use crate::video::DecodedFrame;

pub struct AvSync {
    clock: Arc<PlaybackClock>,
    frame_queue: VecDeque<DecodedFrame>,
    sync_threshold_secs: f64,
    max_queue_frames: usize,
    pub dropped_late: u64,
    pub dropped_overflow: u64,
    pub waited_early: u64,
    /// When true, the first eligible frame is shown immediately (seek bootstrap).
    seeking: bool,
    /// When true and queue is non-empty, show the first frame (startup bootstrap).
    startup_bootstrap: bool,
}

impl AvSync {
    pub fn new(clock: Arc<PlaybackClock>) -> Self {
        Self {
            clock,
            frame_queue: VecDeque::new(),
            sync_threshold_secs: 0.040,
            max_queue_frames: 64,
            dropped_late: 0,
            dropped_overflow: 0,
            waited_early: 0,
            seeking: false,
            startup_bootstrap: true,
        }
    }

    pub fn with_threshold(mut self, threshold_ms: f64) -> Self {
        self.sync_threshold_secs = threshold_ms / 1000.0;
        self
    }

    pub fn push_frame(&mut self, frame: DecodedFrame) {
        while self.frame_queue.len() >= self.max_queue_frames {
            self.frame_queue.pop_front();
            self.dropped_overflow += 1;
        }

        self.frame_queue.push_back(frame);
    }

    pub fn pop_frame_for_display(&mut self) -> Option<DecodedFrame> {
        if self.frame_queue.is_empty() {
            return None;
        }

        // --- Bootstrap mode (startup) ---
        // Show the first queued frame immediately to avoid initial black screen.
        if self.startup_bootstrap {
            self.startup_bootstrap = false;
            return self.frame_queue.pop_front();
        }

        // --- Seeking mode ---
        // Show the first queued frame immediately, then return to normal sync.
        if self.seeking {
            self.seeking = false;
            return self.frame_queue.pop_front();
        }

        let audio_pts = self.clock.position_secs();
        let mut best_idx = None;

        for (i, frame) in self.frame_queue.iter().enumerate() {
            if frame.pts_secs <= audio_pts + self.sync_threshold_secs {
                best_idx = Some(i);
            }
        }

        if let Some(idx) = best_idx {
            for _ in 0..idx {
                self.frame_queue.pop_front();
                self.dropped_late += 1;
            }
            return self.frame_queue.pop_front();
        }

        if self
            .frame_queue
            .front()
            .is_some_and(|f| f.pts_secs > audio_pts + self.sync_threshold_secs)
        {
            self.waited_early += 1;
            if audio_pts <= self.sync_threshold_secs
                && self
                    .frame_queue
                    .front()
                    .is_some_and(|f| f.pts_secs <= self.sync_threshold_secs)
            {
                return self.frame_queue.pop_front();
            }
            return None;
        }

        let late_count = self.frame_queue.len().saturating_sub(1) as u64;
        self.dropped_late += late_count;
        self.frame_queue.pop_back()
    }

    pub fn clear(&mut self) {
        self.frame_queue.clear();
    }

    /// Reset startup bootstrap flag (used when loading new media).
    pub fn reset_startup(&mut self) {
        self.startup_bootstrap = true;
        self.seeking = false;
    }

    /// Mark that a seek is in progress — next pop will return the first frame immediately.
    pub fn set_seeking(&mut self, seeking: bool) {
        self.seeking = seeking;
    }

    pub fn is_seeking(&self) -> bool {
        self.seeking
    }

    pub fn queue_len(&self) -> usize {
        self.frame_queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_queues_all_incoming_frames() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(10.0)));
        clock.on_samples_played(480_000);
        let mut sync = AvSync::new(clock);

        sync.push_frame(make_frame(1.0));
        sync.push_frame(make_frame(2.0));
        assert_eq!(sync.queue_len(), 2);
    }

    /// Helper to create a sync instance with startup bootstrap already satisfied.
    fn sync_no_bootstrap(clock: Arc<PlaybackClock>) -> AvSync {
        let mut sync = AvSync::new(clock);
        sync.startup_bootstrap = false;
        sync
    }

    #[test]
    fn sync_waits_early_frame() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(10.0)));
        let mut sync = sync_no_bootstrap(clock);

        sync.push_frame(make_frame(5.0));
        assert!(sync.pop_frame_for_display().is_none());
        assert_eq!(sync.waited_early, 1);
    }

    #[test]
    fn sync_bootstraps_start_of_stream() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(10.0)));
        let mut sync = AvSync::new(clock);

        sync.push_frame(make_frame(0.0));
        let frame = sync.pop_frame_for_display().expect("bootstrap frame");
        assert!((frame.pts_secs - 0.0).abs() < 1e-6);
    }

    #[test]
    fn sync_catches_up_when_video_lags() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(10.0)));
        clock.seek(2.0);
        let mut sync = sync_no_bootstrap(clock);

        sync.push_frame(make_frame(0.0));
        sync.push_frame(make_frame(0.5));
        sync.push_frame(make_frame(1.0));
        let frame = sync.pop_frame_for_display().expect("catch-up frame");
        assert!((frame.pts_secs - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sync_queue_overflow() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(100.0)));
        let mut sync = sync_no_bootstrap(clock).with_threshold(1000.0);
        sync.max_queue_frames = 2;

        sync.push_frame(make_frame(0.0));
        sync.push_frame(make_frame(0.1));
        sync.push_frame(make_frame(0.2));
        assert_eq!(sync.dropped_overflow, 1);
    }

    #[test]
    fn sync_seeking_bootstrap_returns_immediately() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(10.0)));
        let mut sync = sync_no_bootstrap(clock);
        sync.set_seeking(true);

        sync.push_frame(make_frame(3.0));
        let frame = sync.pop_frame_for_display().expect("seek bootstrap");
        assert!((frame.pts_secs - 3.0).abs() < 1e-6);
        assert!(!sync.is_seeking(), "seeking should be cleared after pop");
    }

    #[test]
    fn sync_startup_bootstrap_works_only_once() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(10.0)));
        let mut sync = AvSync::new(clock);

        sync.push_frame(make_frame(0.0));
        let _first = sync.pop_frame_for_display().expect("first frame");
        // Second call should not have startup_bootstrap; clock at 0, next frame at 10s should wait.
        sync.push_frame(make_frame(10.0));
        assert!(sync.pop_frame_for_display().is_none(), "second frame should wait");
    }

    fn make_frame(pts: f64) -> DecodedFrame {
        DecodedFrame {
            pts_secs: pts,
            width: 2,
            height: 2,
            y_plane: vec![128; 4],
            u_plane: vec![128; 1],
            v_plane: vec![128; 1],
            y_stride: 2,
            uv_stride: 1,
        }
    }
}