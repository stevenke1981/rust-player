use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::error::Result;
use crate::player::WorkerPerFrameStatus;
use crate::sync::AvSync;
use crate::video::{DecodedFrame, Mp4Demuxer, VideoCodec, VideoDecoder};

/// Capacity of decoded frames buffered between the decode thread and the UI thread.
const FRAME_QUEUE_CAP: usize = 32;
/// Packets to demux/decode per worker iteration before yielding.
const PACKETS_PER_BURST: usize = 16;

enum DecodeCommand {
    Seek(f64),
    Shutdown,
}

pub struct VideoDecodeWorker {
    frame_rx: Receiver<DecodedFrame>,
    cmd_tx: Sender<DecodeCommand>,
    thread: Option<JoinHandle<()>>,
    status: Arc<Mutex<WorkerPerFrameStatus>>,
}

impl VideoDecodeWorker {
    pub fn spawn(path: PathBuf, codec: VideoCodec, extradata: Vec<u8>) -> Result<Self> {
        let (frame_tx, frame_rx) = mpsc::sync_channel(FRAME_QUEUE_CAP);
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(WorkerPerFrameStatus::default()));

        let status_clone = status.clone();
        let thread = thread::Builder::new()
            .name("video-decode".into())
            .spawn(move || decode_loop(path, codec, extradata, frame_tx, cmd_rx, status_clone))?;

        Ok(Self {
            frame_rx,
            cmd_tx,
            thread: Some(thread),
            status,
        })
    }

    /// Returns a handle to the shared worker status for the player to read.
    pub fn status_handle(&self) -> Arc<Mutex<WorkerPerFrameStatus>> {
        self.status.clone()
    }

    /// Non-blocking: move all ready frames into the A/V sync queue.
    pub fn poll_frames(&self, av_sync: &mut AvSync) {
        while let Ok(frame) = self.frame_rx.try_recv() {
            av_sync.push_frame(frame);
        }
    }

    pub fn seek(&self, position_secs: f64) {
        let _ = self.cmd_tx.send(DecodeCommand::Seek(position_secs));
        while self.frame_rx.try_recv().is_ok() {}
    }
}

impl Drop for VideoDecodeWorker {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(DecodeCommand::Shutdown);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn decode_loop(
    path: PathBuf,
    _codec: VideoCodec,
    _extradata: Vec<u8>,
    frame_tx: SyncSender<DecodedFrame>,
    cmd_rx: Receiver<DecodeCommand>,
    status: Arc<Mutex<WorkerPerFrameStatus>>,
) {
    // Mark worker running and report init status.
    {
        let mut s = status.lock().unwrap();
        s.worker_running = true;
    }

    let mut demuxer = match Mp4Demuxer::open(&path) {
        Ok(d) => d,
        Err(e) => {
            let msg = format!("failed to open {}: {e}", path.display());
            log::error!("video worker: {msg}");
            let mut s = status.lock().unwrap();
            s.worker_running = false;
            return;
        }
    };

    let mut decoder = match VideoDecoder::for_codec(demuxer.video_codec(), demuxer.extradata()) {
        Ok(d) => d,
        Err(e) => {
            let msg = format!("decoder init failed: {e}");
            log::error!("video worker: {msg}");
            let mut s = status.lock().unwrap();
            s.worker_running = false;
            return;
        }
    };

    log::info!(
        "video decode thread started: codec={:?}, samples={}",
        demuxer.video_codec(),
        demuxer.sample_count()
    );

    // Track demux/decode counts locally to reduce lock contention.
    let mut local_demuxed: u64 = 0;
    let mut local_decoded: u64 = 0;

    loop {
        let cmd = drain_commands(&mut demuxer, &mut decoder, &cmd_rx);
        match cmd {
            CommandAction::Shutdown => break,
            CommandAction::Seeked => {
                local_demuxed = 0;
                local_decoded = 0;
            }
            CommandAction::None => {}
        }

        let mut decoded_any = false;
        for _ in 0..PACKETS_PER_BURST {
            let packet = match demuxer.next_packet() {
                Ok(Some(p)) => {
                    local_demuxed += 1;
                    p
                }
                Ok(None) => break,
                Err(e) => {
                    log::debug!("video worker demux: {e}");
                    let mut s = status.lock().unwrap();
                    s.demuxed_packets = local_demuxed;
                    break;
                }
            };

            let frames = match decoder.decode(&packet) {
                Ok(f) => f,
                Err(e) => {
                    log::debug!("video worker decode: {e}");
                    let mut s = status.lock().unwrap();
                    s.demuxed_packets = local_demuxed;
                    continue;
                }
            };

            for frame in frames {
                local_decoded += 1;
                if frame_tx.send(frame).is_err() {
                    let mut s = status.lock().unwrap();
                    s.demuxed_packets = local_demuxed;
                    s.decoded_frames = local_decoded;
                    s.worker_running = false;
                    return;
                }
                decoded_any = true;
            }
        }

        // Sync status to shared state periodically (every burst).
        {
            let mut s = status.lock().unwrap();
            s.demuxed_packets = local_demuxed;
            s.decoded_frames = local_decoded;
        }

        if !decoded_any {
            thread::sleep(Duration::from_millis(5));
        } else {
            thread::sleep(Duration::from_millis(1));
        }
    }

    // Worker loop ended normally.
    let mut s = status.lock().unwrap();
    s.worker_running = false;
    s.demuxed_packets = local_demuxed;
    s.decoded_frames = local_decoded;
}

enum CommandAction {
    None,
    Seeked,
    Shutdown,
}

fn drain_commands(
    demuxer: &mut Mp4Demuxer,
    decoder: &mut VideoDecoder,
    cmd_rx: &Receiver<DecodeCommand>,
) -> CommandAction {
    let mut latest_seek = None;
    while let Ok(cmd) = cmd_rx.try_recv() {
        match cmd {
            DecodeCommand::Seek(pos) => latest_seek = Some(pos),
            DecodeCommand::Shutdown => return CommandAction::Shutdown,
        }
    }

    let Some(position_secs) = latest_seek else {
        return CommandAction::None;
    };

    if let Err(e) = demuxer.seek(position_secs) {
        log::error!("video worker seek failed: {e}");
        return CommandAction::Seeked;
    }

    match VideoDecoder::for_codec(demuxer.video_codec(), demuxer.extradata()) {
        Ok(d) => {
            *decoder = d;
            log::debug!("video worker seek to {position_secs:.3}s");
        }
        Err(e) => log::error!("video worker decoder reset failed: {e}"),
    }

    CommandAction::Seeked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn worker_spawns_and_drops_cleanly() {
        let path = PathBuf::from("assets/test_av1.mp4");
        if !path.exists() {
            return;
        }
        let demuxer = Mp4Demuxer::open(&path).expect("demux");
        let worker = VideoDecodeWorker::spawn(
            path,
            demuxer.video_codec(),
            demuxer.extradata().to_vec(),
        )
        .expect("spawn");
        std::thread::sleep(Duration::from_millis(50));
        drop(worker);
    }
}