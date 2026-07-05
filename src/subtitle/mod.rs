use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct SubtitleCue {
    pub start_secs: f64,
    pub end_secs: f64,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct SubtitleTrack {
    cues: Vec<SubtitleCue>,
}

impl SubtitleTrack {
    pub fn from_srt(content: &str) -> Result<Self> {
        let mut cues = Vec::new();
        let blocks: Vec<&str> = content
            .split("\n\n")
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .collect();

        for block in blocks {
            let lines: Vec<&str> = block.lines().collect();
            if lines.len() < 2 {
                continue;
            }
            let time_line = if lines[0].chars().all(|c| c.is_ascii_digit()) && lines.len() >= 3 {
                lines[1]
            } else {
                lines[0]
            };
            let text_start = if lines[0].chars().all(|c| c.is_ascii_digit()) && lines.len() >= 3 {
                2
            } else {
                1
            };

            let Some((start, end)) = parse_srt_time_range(time_line) else {
                continue;
            };
            let text: String = lines[text_start..]
                .join("\n")
                .replace("<i>", "")
                .replace("</i>", "")
                .replace("<b>", "")
                .replace("</b>", "");
            cues.push(SubtitleCue {
                start_secs: start,
                end_secs: end,
                text,
            });
        }

        cues.sort_by(|a, b| {
            a.start_secs
                .partial_cmp(&b.start_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(Self { cues })
    }

    pub fn load_sidecar(video_path: &Path) -> Option<Self> {
        let srt_path = video_path.with_extension("srt");
        if !srt_path.exists() {
            return None;
        }
        match fs::read_to_string(&srt_path) {
            Ok(content) => match Self::from_srt(&content) {
                Ok(track) => {
                    log::info!(
                        "loaded {} subtitle cues from {}",
                        track.cues.len(),
                        srt_path.display()
                    );
                    Some(track)
                }
                Err(e) => {
                    log::warn!("failed to parse {}: {e}", srt_path.display());
                    None
                }
            },
            Err(e) => {
                log::warn!("failed to read {}: {e}", srt_path.display());
                None
            }
        }
    }

    pub fn cue_at(&self, time_secs: f64) -> Option<&SubtitleCue> {
        self.cues.iter().find(|c| time_secs >= c.start_secs && time_secs < c.end_secs)
    }

    pub fn is_empty(&self) -> bool {
        self.cues.is_empty()
    }

    pub fn path_for_video(video_path: &Path) -> PathBuf {
        video_path.with_extension("srt")
    }
}

fn parse_srt_time_range(line: &str) -> Option<(f64, f64)> {
    let parts: Vec<&str> = line.split("-->").map(str::trim).collect();
    if parts.len() != 2 {
        return None;
    }
    Some((parse_srt_timestamp(parts[0])?, parse_srt_timestamp(parts[1])?))
}

fn parse_srt_timestamp(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let segments: Vec<&str> = s.split(':').collect();
    match segments.len() {
        3 => {
            let h: f64 = segments[0].parse().ok()?;
            let m: f64 = segments[1].parse().ok()?;
            let sec: f64 = segments[2].parse().ok()?;
            Some(h * 3600.0 + m * 60.0 + sec)
        }
        2 => {
            let m: f64 = segments[0].parse().ok()?;
            let sec: f64 = segments[1].parse().ok()?;
            Some(m * 60.0 + sec)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_srt_basic() {
        let srt = "1\n00:00:01,000 --> 00:00:03,500\nHello world\n";
        let track = SubtitleTrack::from_srt(srt).unwrap();
        assert_eq!(track.cues.len(), 1);
        assert!((track.cues[0].start_secs - 1.0).abs() < 1e-3);
        assert!((track.cues[0].end_secs - 3.5).abs() < 1e-3);
        assert_eq!(track.cues[0].text, "Hello world");
    }

    #[test]
    fn cue_at_finds_active() {
        let srt = "1\n00:00:00,000 --> 00:00:02,000\nA\n\n2\n00:00:02,000 --> 00:00:04,000\nB\n";
        let track = SubtitleTrack::from_srt(srt).unwrap();
        assert_eq!(track.cue_at(1.0).unwrap().text, "A");
        assert_eq!(track.cue_at(2.5).unwrap().text, "B");
        assert!(track.cue_at(5.0).is_none());
    }

    #[test]
    fn parse_timestamp_formats() {
        assert!((parse_srt_timestamp("01:02:03,456").unwrap() - 3723.456).abs() < 1e-3);
        assert!((parse_srt_timestamp("02:30.000").unwrap() - 150.0).abs() < 1e-3);
    }
}