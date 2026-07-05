use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use mp4parse::unstable::{create_sample_table, Indice};
use mp4parse::{read_mp4, CodecType, MediaContext, SampleEntry, Track, TrackType, VideoCodecSpecific};

use crate::error::{PlayerError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    Av1,
    H264,
    H265,
    Unknown,
}

pub struct VideoPacket {
    pub pts_secs: f64,
    pub dts_secs: f64,
    pub data: Vec<u8>,
    pub is_keyframe: bool,
}

enum DemuxBackend {
    Mp4parse {
        file: File,
        samples: Vec<Indice>,
        sample_index: usize,
    },
    Mp4Crate {
        reader: mp4::Mp4Reader<BufReader<File>>,
        track_id: u32,
        sample_index: usize,
        sample_count: u32,
    },
}

pub struct Mp4Demuxer {
    backend: DemuxBackend,
    timescale: u32,
    codec: VideoCodec,
    extradata: Vec<u8>,
}

impl Mp4Demuxer {
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_mp4parse(path).or_else(|_| Self::open_mp4_crate(path))
    }

    fn open_mp4parse(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let context = read_mp4(&mut file).map_err(|e| PlayerError::VideoDemux(e.to_string()))?;

        let (track, timescale, codec, extradata) = find_mp4parse_video_track(&context)
            .ok_or_else(|| PlayerError::VideoDemux("no supported video track".into()))?;

        let track_offset = track.media_time.map(|t| t.0 as i64).unwrap_or(0);
        let sample_table = create_sample_table(track, track_offset.into())
            .ok_or_else(|| PlayerError::VideoDemux("failed to build sample table".into()))?;

        let samples: Vec<Indice> = sample_table.into_iter().collect();
        if samples.is_empty() {
            return Err(PlayerError::VideoDemux("no samples in track".into()));
        }

        Ok(Self {
            backend: DemuxBackend::Mp4parse {
                file,
                samples,
                sample_index: 0,
            },
            timescale,
            codec,
            extradata,
        })
    }

    fn open_mp4_crate(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        let reader = BufReader::new(file);
        let mp4 = mp4::Mp4Reader::read_header(reader, size)
            .map_err(|e| PlayerError::VideoDemux(e.to_string()))?;

        let (track_id, codec, extradata, timescale, sample_count) =
            find_mp4_crate_video_track(&mp4)
                .ok_or_else(|| PlayerError::VideoDemux("no H.264/H.265 video track".into()))?;

        if sample_count == 0 {
            return Err(PlayerError::VideoDemux("no samples in track".into()));
        }

        Ok(Self {
            backend: DemuxBackend::Mp4Crate {
                reader: mp4,
                track_id,
                sample_index: 0,
                sample_count,
            },
            timescale,
            codec,
            extradata,
        })
    }

    pub fn video_codec(&self) -> VideoCodec {
        self.codec
    }

    pub fn extradata(&self) -> &[u8] {
        &self.extradata
    }

    pub fn timebase(&self) -> (u32, u32) {
        (1, self.timescale)
    }

    pub fn sample_count(&self) -> u32 {
        match &self.backend {
            DemuxBackend::Mp4parse { samples, .. } => samples.len() as u32,
            DemuxBackend::Mp4Crate { sample_count, .. } => *sample_count,
        }
    }

    pub fn next_packet(&mut self) -> Result<Option<VideoPacket>> {
        match &mut self.backend {
            DemuxBackend::Mp4parse {
                file,
                samples,
                sample_index,
            } => next_mp4parse_packet(file, samples, sample_index, self.timescale),
            DemuxBackend::Mp4Crate {
                reader,
                track_id,
                sample_index,
                ..
            } => next_mp4_crate_packet(reader, *track_id, sample_index, self.timescale),
        }
    }

    pub fn seek(&mut self, pts_secs: f64) -> Result<()> {
        let target = (pts_secs * self.timescale as f64) as i64;
        match &mut self.backend {
            DemuxBackend::Mp4parse { samples, sample_index, .. } => {
                *sample_index = seek_keyframe_index(samples, target);
            }
            DemuxBackend::Mp4Crate {
                reader,
                track_id,
                sample_index,
                sample_count,
            } => {
                *sample_index = seek_mp4_crate(reader, *track_id, *sample_count, target);
            }
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        match &mut self.backend {
            DemuxBackend::Mp4parse { sample_index, .. } => *sample_index = 0,
            DemuxBackend::Mp4Crate { sample_index, .. } => *sample_index = 0,
        }
    }
}

fn next_mp4parse_packet(
    file: &mut File,
    samples: &[Indice],
    sample_index: &mut usize,
    timescale: u32,
) -> Result<Option<VideoPacket>> {
    if *sample_index >= samples.len() {
        return Ok(None);
    }

    let sample = &samples[*sample_index];
    *sample_index += 1;

    let start = sample.start_offset.0;
    let end = sample.end_offset.0;
    let size = (end - start) as usize;
    let mut data = vec![0u8; size];
    file.seek(SeekFrom::Start(start))?;
    file.read_exact(&mut data)?;

    let pts_secs = sample.start_composition.0 as f64 / timescale as f64;
    let dts_secs = sample.start_decode.0 as f64 / timescale as f64;

    Ok(Some(VideoPacket {
        pts_secs,
        dts_secs,
        data,
        is_keyframe: sample.sync,
    }))
}

fn next_mp4_crate_packet(
    reader: &mut mp4::Mp4Reader<BufReader<File>>,
    track_id: u32,
    sample_index: &mut usize,
    timescale: u32,
) -> Result<Option<VideoPacket>> {
    let sample_count = reader
        .sample_count(track_id)
        .map_err(|e| PlayerError::VideoDemux(e.to_string()))? as usize;
    if *sample_index >= sample_count {
        return Ok(None);
    }

    let sample_id = (*sample_index + 1) as u32;
    *sample_index += 1;

    let sample = reader
        .read_sample(track_id, sample_id)
        .map_err(|e| PlayerError::VideoDemux(e.to_string()))?
        .ok_or_else(|| PlayerError::VideoDemux("missing sample".into()))?;

    let pts_secs = sample.start_time as f64 / timescale as f64;
    let dts_secs = pts_secs;

    Ok(Some(VideoPacket {
        pts_secs,
        dts_secs,
        data: sample.bytes.to_vec(),
        is_keyframe: sample.is_sync,
    }))
}

fn find_mp4parse_video_track(
    context: &MediaContext,
) -> Option<(&Track, u32, VideoCodec, Vec<u8>)> {
    for track in &context.tracks {
        if track.track_type != TrackType::Video {
            continue;
        }
        let stsd = track.stsd.as_ref()?;
        let entry = stsd.descriptions.first()?;
        if let SampleEntry::Video(v) = entry {
            let timescale = track.timescale.map(|t| t.0 as u32).unwrap_or(1);
            match v.codec_type {
                CodecType::AV1 => {
                    return Some((track, timescale, VideoCodec::Av1, Vec::new()));
                }
                CodecType::H264 => {
                    let extradata = match &v.codec_specific {
                        VideoCodecSpecific::AVCConfig(data) => data.to_vec(),
                        _ => Vec::new(),
                    };
                    return Some((track, timescale, VideoCodec::H264, extradata));
                }
                _ => {}
            }
        }
    }
    None
}

fn find_mp4_crate_video_track(
    mp4: &mp4::Mp4Reader<BufReader<File>>,
) -> Option<(u32, VideoCodec, Vec<u8>, u32, u32)> {
    for (track_id, track) in mp4.tracks() {
        let Ok(mp4::TrackType::Video) = track.track_type() else {
            continue;
        };
        let Ok(media_type) = track.media_type() else {
            continue;
        };

        let timescale = track.timescale();
        let sample_count = track.sample_count();

        match media_type {
            mp4::MediaType::H264 => {
                let extradata = avcc_from_mp4_track(track);
                return Some((
                    *track_id,
                    VideoCodec::H264,
                    extradata,
                    timescale,
                    sample_count,
                ));
            }
            mp4::MediaType::H265 => {
                return Some((
                    *track_id,
                    VideoCodec::H265,
                    Vec::new(),
                    timescale,
                    sample_count,
                ));
            }
            _ => {}
        }
    }
    None
}

fn avcc_from_mp4_track(track: &mp4::Mp4Track) -> Vec<u8> {
    let Ok(sps) = track.sequence_parameter_set() else {
        return Vec::new();
    };
    let Ok(pps) = track.picture_parameter_set() else {
        return Vec::new();
    };
    if sps.len() < 4 {
        return Vec::new();
    }

    let mut out = Vec::new();
    out.push(1);
    out.push(sps[1]);
    out.push(sps[2]);
    out.push(sps[3]);
    out.push(0xff);
    out.push(0xe0 | 1);
    out.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    out.extend_from_slice(sps);
    out.push(1);
    out.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    out.extend_from_slice(pps);
    out
}

fn seek_mp4_crate(
    reader: &mut mp4::Mp4Reader<BufReader<File>>,
    track_id: u32,
    sample_count: u32,
    target: i64,
) -> usize {
    if sample_count == 0 {
        return 0;
    }

    let target = target as u64;
    let mut lo = 1u32;
    let mut hi = sample_count + 1;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let time = reader
            .read_sample(track_id, mid)
            .ok()
            .flatten()
            .map(|s| s.start_time)
            .unwrap_or(0);
        if time <= target {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }

    let at_or_before = lo.saturating_sub(1).max(1);
    for sid in at_or_before..=sample_count {
        if let Ok(Some(sample)) = reader.read_sample(track_id, sid) {
            if sample.is_sync {
                return sid as usize - 1;
            }
        }
    }

    (at_or_before - 1) as usize
}

/// Binary search for the nearest keyframe at or before `target`.
fn seek_keyframe_index(samples: &[Indice], target: i64) -> usize {
    if samples.is_empty() {
        return 0;
    }

    let mut lo = 0usize;
    let mut hi = samples.len();
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if samples[mid].start_composition.0 <= target {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }

    let at_or_before = lo.saturating_sub(1);

    for (i, sample) in samples.iter().enumerate().skip(at_or_before) {
        if sample.sync {
            return i;
        }
    }

    for i in (0..=at_or_before).rev() {
        if samples[i].sync {
            return i;
        }
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp4parse::unstable::Indice;

    fn make_sample(pts: i64, sync: bool) -> Indice {
        use mp4parse::unstable::CheckedInteger;
        Indice {
            start_composition: CheckedInteger(pts),
            start_decode: CheckedInteger(pts),
            sync,
            ..Default::default()
        }
    }

    #[test]
    fn pts_conversion() {
        let timescale = 90000u32;
        let start_time = 90000u32;
        let pts = start_time as f64 / timescale as f64;
        assert!((pts - 1.0).abs() < 1e-6);
    }

    #[test]
    fn seek_keyframe_picks_nearest_sync() {
        let samples = vec![
            make_sample(0, true),
            make_sample(100, false),
            make_sample(200, false),
            make_sample(300, true),
            make_sample(400, false),
        ];
        assert_eq!(seek_keyframe_index(&samples, 250), 3);
        assert_eq!(seek_keyframe_index(&samples, 0), 0);
        assert_eq!(seek_keyframe_index(&samples, 500), 3);
    }

    #[test]
    fn video_codec_variants_distinct() {
        assert_ne!(VideoCodec::H264, VideoCodec::H265);
        assert_ne!(VideoCodec::Av1, VideoCodec::H264);
    }
}