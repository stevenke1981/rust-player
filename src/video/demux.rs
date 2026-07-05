use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use mp4parse::unstable::{create_sample_table, Indice};
use mp4parse::{read_mp4, CodecType, MediaContext, Track, TrackType};

use crate::error::{PlayerError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    Av1,
    Unknown,
}

pub struct VideoPacket {
    pub pts_secs: f64,
    pub dts_secs: f64,
    pub data: Vec<u8>,
    pub is_keyframe: bool,
}

pub struct Mp4Demuxer {
    file: File,
    samples: Vec<Indice>,
    timescale: u32,
    sample_index: usize,
    codec: VideoCodec,
}

impl Mp4Demuxer {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let context = read_mp4(&mut file).map_err(|e| PlayerError::VideoDemux(e.to_string()))?;

        let (track, timescale) = find_av1_video_track(&context)
            .ok_or_else(|| PlayerError::VideoDemux("no AV1 video track".into()))?;

        let track_offset = track
            .media_time
            .map(|t| t.0 as i64)
            .unwrap_or(0);

        let sample_table = create_sample_table(track, track_offset.into())
            .ok_or_else(|| PlayerError::VideoDemux("failed to build sample table".into()))?;

        let samples: Vec<Indice> = sample_table.into_iter().collect();
        if samples.is_empty() {
            return Err(PlayerError::VideoDemux("no samples in track".into()));
        }

        Ok(Self {
            file,
            samples,
            timescale,
            sample_index: 0,
            codec: VideoCodec::Av1,
        })
    }

    pub fn video_codec(&self) -> VideoCodec {
        self.codec
    }

    pub fn timebase(&self) -> (u32, u32) {
        (1, self.timescale)
    }

    pub fn sample_count(&self) -> u32 {
        self.samples.len() as u32
    }

    pub fn next_packet(&mut self) -> Result<Option<VideoPacket>> {
        if self.sample_index >= self.samples.len() {
            return Ok(None);
        }

        let sample = &self.samples[self.sample_index];
        self.sample_index += 1;

        let start = sample.start_offset.0;
        let end = sample.end_offset.0;
        let size = (end - start) as usize;
        let mut data = vec![0u8; size];
        self.file.seek(SeekFrom::Start(start))?;
        self.file.read_exact(&mut data)?;

        let pts_secs = sample.start_composition.0 as f64 / self.timescale as f64;
        let dts_secs = sample.start_decode.0 as f64 / self.timescale as f64;

        Ok(Some(VideoPacket {
            pts_secs,
            dts_secs,
            data,
            is_keyframe: sample.sync,
        }))
    }

    pub fn seek(&mut self, pts_secs: f64) -> Result<()> {
        let target = (pts_secs * self.timescale as f64) as i64;
        let mut best = 0usize;
        for (i, sample) in self.samples.iter().enumerate() {
            if sample.start_composition.0 <= target {
                best = i;
            }
            if sample.start_composition.0 >= target && sample.sync {
                best = i;
                break;
            }
        }
        self.sample_index = best;
        Ok(())
    }

    pub fn reset(&mut self) {
        self.sample_index = 0;
    }
}

fn find_av1_video_track(context: &MediaContext) -> Option<(&Track, u32)> {
    for track in &context.tracks {
        if track.track_type != TrackType::Video {
            continue;
        }
        let stsd = track.stsd.as_ref()?;
        let entry = stsd.descriptions.first()?;
        if let mp4parse::SampleEntry::Video(v) = entry {
            if v.codec_type == CodecType::AV1 {
                let timescale = track.timescale.map(|t| t.0 as u32).unwrap_or(1);
                return Some((track, timescale));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn pts_conversion() {
        let timescale = 90000u32;
        let start_time = 90000u32;
        let pts = start_time as f64 / timescale as f64;
        assert!((pts - 1.0).abs() < 1e-6);
    }
}