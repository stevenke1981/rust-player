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
}

impl AvSync {
    pub fn new(clock: Arc<PlaybackClock>) -> Self {
        Self {
            clock,
            frame_queue: VecDeque::new(),
            sync_threshold_secs: 0.040,
            max_queue_frames: 12,
            dropped_late: 0,
            dropped_overflow: 0,
            waited_early: 0,
        }
    }

    pub fn with_threshold(mut self, threshold_ms: f64) -> Self {
        self.sync_threshold_secs = threshold_ms / 1000.0;
        self
    }

    pub fn push_frame(&mut self, frame: DecodedFrame) {
        let audio_pts = self.clock.position_secs();

        if frame.pts_secs < audio_pts - self.sync_threshold_secs {
            self.dropped_late += 1;
            return;
        }

        while self.frame_queue.len() >= self.max_queue_frames {
            self.frame_queue.pop_front();
            self.dropped_overflow += 1;
        }

        self.frame_queue.push_back(frame);
    }

    pub fn pop_frame_for_display(&mut self) -> Option<DecodedFrame> {
        let audio_pts = self.clock.position_secs();

        while let Some(frame) = self.frame_queue.front() {
            if frame.pts_secs > audio_pts + self.sync_threshold_secs {
                self.waited_early += 1;
                return None;
            }

            let frame = self.frame_queue.pop_front().unwrap();
            if frame.pts_secs < audio_pts - self.sync_threshold_secs {
                self.dropped_late += 1;
                continue;
            }
            return Some(frame);
        }

        None
    }

    pub fn clear(&mut self) {
        self.frame_queue.clear();
    }

    pub fn queue_len(&self) -> usize {
        self.frame_queue.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_drops_late_frame() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(10.0)));
        clock.on_samples_played(480_000); // 10s
        let mut sync = AvSync::new(clock);

        sync.push_frame(make_frame(1.0));
        assert_eq!(sync.dropped_late, 1);
    }

    #[test]
    fn sync_waits_early_frame() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(10.0)));
        let mut sync = AvSync::new(clock);

        sync.push_frame(make_frame(5.0));
        assert!(sync.pop_frame_for_display().is_none());
        assert_eq!(sync.waited_early, 1);
    }

    #[test]
    fn sync_queue_overflow() {
        let clock = Arc::new(PlaybackClock::new(48_000, Some(100.0)));
        let mut sync = AvSync::new(clock).with_threshold(1000.0);
        sync.max_queue_frames = 2;

        sync.push_frame(make_frame(0.0));
        sync.push_frame(make_frame(0.1));
        sync.push_frame(make_frame(0.2));
        assert_eq!(sync.dropped_overflow, 1);
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