use std::fs::File;
use std::path::Path;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::{PlayerError, Result};

pub struct AudioBuffer {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub sample_rate: u32,
    pub pts_secs: f64,
}

pub struct AudioDecoder {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    sample_rate: u32,
    channels: u16,
    sample_cursor: u64,
    duration_secs: Option<f64>,
}

impl AudioDecoder {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| PlayerError::AudioDecode(e.to_string()))?;

        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| PlayerError::AudioDecode("no audio track found".into()))?;

        let track_id = track.id;
        let codec_params = &track.codec_params;
        let sample_rate = codec_params
            .sample_rate
            .ok_or_else(|| PlayerError::AudioDecode("unknown sample rate".into()))?;
        let channels = codec_params
            .channels
            .map(|c| c.count() as u16)
            .unwrap_or(2);

        let duration_secs = codec_params
            .n_frames
            .map(|frames| frames as f64 / sample_rate as f64);

        let decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| PlayerError::AudioDecode(e.to_string()))?;

        Ok(Self {
            format,
            decoder,
            track_id,
            sample_rate,
            channels,
            sample_cursor: 0,
            duration_secs,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn duration_secs(&self) -> Option<f64> {
        self.duration_secs
    }

    pub fn decode_next(&mut self) -> Result<Option<AudioBuffer>> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(SymphoniaError::ResetRequired) => {
                    return Err(PlayerError::AudioDecode("decoder reset required".into()));
                }
                Err(e) => return Err(PlayerError::AudioDecode(e.to_string())),
            };

            if packet.track_id() != self.track_id {
                continue;
            }

            let decoded = self
                .decoder
                .decode(&packet)
                .map_err(|e| PlayerError::AudioDecode(e.to_string()))?;

            let pts_secs = self.sample_cursor as f64 / self.sample_rate as f64;
            let samples = convert_to_f32(&decoded);
            let frame_samples = samples.len() / self.channels as usize;
            self.sample_cursor += frame_samples as u64;

            return Ok(Some(AudioBuffer {
                samples,
                channels: self.channels,
                sample_rate: self.sample_rate,
                pts_secs,
            }));
        }
    }

    pub fn seek(&mut self, position_secs: f64) -> Result<()> {
        use symphonia::core::formats::SeekMode;
        use symphonia::core::units::Time;

        let time = Time::from(position_secs);
        self.format
            .seek(
                SeekMode::Accurate,
                symphonia::core::formats::SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| PlayerError::AudioDecode(e.to_string()))?;

        self.decoder.reset();
        self.sample_cursor = (position_secs * self.sample_rate as f64).round() as u64;
        Ok(())
    }
}

fn convert_to_f32(decoded: &AudioBufferRef<'_>) -> Vec<f32> {
    match decoded {
        AudioBufferRef::F32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let mut out = Vec::with_capacity(frames * channels);
            for frame in 0..frames {
                for ch in 0..channels {
                    out.push(buf.chan(ch)[frame]);
                }
            }
            out
        }
        AudioBufferRef::F64(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let mut out = Vec::with_capacity(frames * channels);
            for frame in 0..frames {
                for ch in 0..channels {
                    out.push(buf.chan(ch)[frame] as f32);
                }
            }
            out
        }
        AudioBufferRef::S16(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let mut out = Vec::with_capacity(frames * channels);
            for frame in 0..frames {
                for ch in 0..channels {
                    let s = buf.chan(ch)[frame] as f32 / i16::MAX as f32;
                    out.push(s);
                }
            }
            out
        }
        AudioBufferRef::S24(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let mut out = Vec::with_capacity(frames * channels);
            for frame in 0..frames {
                for ch in 0..channels {
                    let s = buf.chan(ch)[frame].inner() as f32 / 8_388_608.0;
                    out.push(s);
                }
            }
            out
        }
        AudioBufferRef::S32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let mut out = Vec::with_capacity(frames * channels);
            for frame in 0..frames {
                for ch in 0..channels {
                    let s = buf.chan(ch)[frame] as f32 / i32::MAX as f32;
                    out.push(s);
                }
            }
            out
        }
        AudioBufferRef::U8(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let mut out = Vec::with_capacity(frames * channels);
            for frame in 0..frames {
                for ch in 0..channels {
                    let s = (buf.chan(ch)[frame] as f32 - 128.0) / 128.0;
                    out.push(s);
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

