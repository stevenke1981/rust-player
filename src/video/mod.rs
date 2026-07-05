mod decoder;
mod demux;
mod frame;
mod h264;
mod h265;
mod nal;
mod obu;
mod worker;

pub use decoder::{Av1Decoder, DecodedFrame, VideoDecoder};
pub use demux::{Mp4Demuxer, VideoCodec, VideoPacket};
pub use worker::VideoDecodeWorker;

use std::path::Path;

use crate::error::Result;

pub fn decode_file(path: &Path, max_frames: usize) -> Result<()> {
    let mut demuxer = Mp4Demuxer::open(path)?;
    let mut decoder = VideoDecoder::for_codec(demuxer.video_codec(), demuxer.extradata())?;

    log::info!(
        "opened video: codec={:?}, samples={}",
        demuxer.video_codec(),
        demuxer.sample_count()
    );

    let mut decoded = 0usize;
    let mut packets_tried = 0usize;
    while decoded < max_frames && packets_tried < 200 {
        let Some(packet) = demuxer.next_packet()? else {
            break;
        };
        packets_tried += 1;

        if packet.data.len() < 4 {
            continue;
        }

        let frames = decoder.decode(&packet)?;
        for frame in frames {
            log::info!(
                "PTS={:.3}s size={}x{} Y={} U={} V={}",
                frame.pts_secs,
                frame.width,
                frame.height,
                frame.y_plane.len(),
                frame.u_plane.len(),
                frame.v_plane.len()
            );
            decoded += 1;
            if decoded >= max_frames {
                break;
            }
        }
    }

    if decoded < max_frames {
        if let Ok(frames) = decoder.flush() {
            for frame in frames {
                log::info!(
                    "PTS={:.3}s size={}x{} Y={} U={} V={}",
                    frame.pts_secs,
                    frame.width,
                    frame.height,
                    frame.y_plane.len(),
                    frame.u_plane.len(),
                    frame.v_plane.len()
                );
                decoded += 1;
                if decoded >= max_frames {
                    break;
                }
            }
        }
    }

    log::info!("decoded {decoded} frames");
    Ok(())
}