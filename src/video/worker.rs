use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::error::Result;
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
}

impl VideoDecodeWorker {
    pub fn spawn(path: PathBuf, codec: VideoCodec, extradata: Vec<u8>) -> Result<Self> {
        let (frame_tx, frame_rx) = mpsc::sync_channel(FRAME_QUEUE_CAP);
        let (cmd_tx, cmd_rx) = mpsc::channel();

        let thread = thread::Builder::new()
            .name("video-decode".into())
            .spawn(move || decode_loop(path, codec, extradata, frame_tx, cmd_rx))?;

        Ok(Self {
            frame_rx,
            cmd_tx,
            thread: Some(thread),
        })
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
) {
    let mut demuxer = match Mp4Demuxer::open(&path) {
        Ok(d) => d,
        Err(e) => {
            log::error!("video worker: failed to open {}: {e}", path.display());
            return;
        }
    };

    let mut decoder = match VideoDecoder::for_codec(demuxer.video_codec(), demuxer.extradata()) {
        Ok(d) => d,
        Err(e) => {
            log::error!("video worker: decoder init failed: {e}");
            return;
        }
    };

    log::info!(
        "video decode thread started: codec={:?}, samples={}",
        demuxer.video_codec(),
        demuxer.sample_count()
    );

    loop {
        match drain_commands(&mut demuxer, &mut decoder, &cmd_rx) {
            CommandAction::Shutdown => break,
            CommandAction::Seeked | CommandAction::None => {}
        }

        let mut decoded_any = false;
        for _ in 0..PACKETS_PER_BURST {
            let packet = match demuxer.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => break,
                Err(e) => {
                    log::debug!("video worker demux: {e}");
                    break;
                }
            };

            let Ok(frames) = decoder.decode(&packet) else {
                continue;
            };

            for frame in frames {
                if frame_tx.send(frame).is_err() {
                    return;
                }
                decoded_any = true;
            }
        }

        if !decoded_any {
            thread::sleep(Duration::from_millis(5));
        } else {
            thread::sleep(Duration::from_millis(1));
        }
    }
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