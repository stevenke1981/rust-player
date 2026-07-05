use std::path::{Path, PathBuf};

use crate::error::{PlayerError, Result};

pub fn is_stream_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Resolve a local path or remote URL into a playable local file.
pub fn resolve_media_source(path_or_url: &str) -> Result<PathBuf> {
    if is_stream_url(path_or_url) {
        if path_or_url.contains(".m3u8") {
            download_hls(path_or_url)
        } else {
            download_http_media(path_or_url)
        }
    } else {
        Ok(PathBuf::from(path_or_url))
    }
}

fn http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| PlayerError::Unsupported(format!("HTTP client: {e}")))
}

fn download_http_media(url: &str) -> Result<PathBuf> {
    log::info!("downloading stream: {url}");
    let client = http_client()?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| PlayerError::Unsupported(format!("HTTP GET failed: {e}")))?;

    if !response.status().is_success() {
        return Err(PlayerError::Unsupported(format!(
            "HTTP {} for {url}",
            response.status()
        )));
    }

    let ext = guess_extension(url, response.headers().get(reqwest::header::CONTENT_TYPE));
    let mut temp = tempfile::Builder::new()
        .prefix("rust-player-stream-")
        .suffix(&format!(".{ext}"))
        .tempfile()
        .map_err(PlayerError::Io)?;
    response
        .copy_to(&mut temp)
        .map_err(|e| PlayerError::Unsupported(format!("download failed: {e}")))?;

    let written = temp.as_file().metadata().map(|m| m.len()).unwrap_or(0);
    let (_file, path) = temp.keep().map_err(|e| PlayerError::Io(e.error))?;
    log::info!("downloaded {written} bytes to {}", path.display());
    Ok(path)
}

fn download_hls(url: &str) -> Result<PathBuf> {
    log::info!("fetching HLS playlist: {url}");
    let client = http_client()?;
    let playlist = client
        .get(url)
        .send()
        .map_err(|e| PlayerError::Unsupported(format!("HLS fetch failed: {e}")))?
        .text()
        .map_err(|e| PlayerError::Unsupported(format!("HLS read failed: {e}")))?;

    let media_url = resolve_hls_media_url(url, &playlist, &client)?;
    let segment_urls = parse_hls_segments(&media_url, &client)?;
    if segment_urls.is_empty() {
        return Err(PlayerError::Unsupported("HLS playlist has no segments".into()));
    }

    let mut temp = tempfile::Builder::new()
        .prefix("rust-player-hls-")
        .suffix(".ts")
        .tempfile()
        .map_err(PlayerError::Io)?;

    for (i, seg_url) in segment_urls.iter().enumerate() {
        log::debug!("HLS segment {}/{}: {seg_url}", i + 1, segment_urls.len());
        let mut resp = client
            .get(seg_url)
            .send()
            .map_err(|e| PlayerError::Unsupported(format!("segment fetch: {e}")))?;
        resp.copy_to(&mut temp)
            .map_err(|e| PlayerError::Unsupported(format!("segment write: {e}")))?;
    }

    let (_file, path) = temp.keep().map_err(|e| PlayerError::Io(e.error))?;
    log::info!(
        "HLS: concatenated {} segments to {}",
        segment_urls.len(),
        path.display()
    );
    Ok(path)
}

fn resolve_hls_media_url(
    master_url: &str,
    playlist: &str,
    client: &reqwest::blocking::Client,
) -> Result<String> {
    if playlist.contains("#EXTINF:") {
        return Ok(master_url.to_string());
    }

    let base = url_dir(master_url);
    let lines: Vec<&str> = playlist.lines().collect();
    let mut best_bw = 0u64;
    let mut best_uri = None;

    for i in 0..lines.len() {
        let line = lines[i].trim();
        if line.starts_with("#EXT-X-STREAM-INF:") {
            let bw = parse_bandwidth(line);
            if let Some(next) = lines.get(i + 1) {
                let uri = next.trim();
                if !uri.starts_with('#') && bw >= best_bw {
                    best_bw = bw;
                    best_uri = Some(resolve_url(&base, uri));
                }
            }
        }
    }

    let variant_url = best_uri
        .ok_or_else(|| PlayerError::Unsupported("no playable HLS variant found".into()))?;
    log::info!("HLS variant selected: bandwidth={best_bw}");

    let sub = client
        .get(&variant_url)
        .send()
        .map_err(|e| PlayerError::Unsupported(e.to_string()))?
        .text()
        .map_err(|e| PlayerError::Unsupported(e.to_string()))?;

    if sub.contains("#EXTINF:") {
        Ok(variant_url)
    } else {
        resolve_hls_media_url(&variant_url, &sub, client)
    }
}

fn parse_hls_segments(
    playlist_url: &str,
    client: &reqwest::blocking::Client,
) -> Result<Vec<String>> {
    let playlist = client
        .get(playlist_url)
        .send()
        .map_err(|e| PlayerError::Unsupported(e.to_string()))?
        .text()
        .map_err(|e| PlayerError::Unsupported(e.to_string()))?;

    let base = url_dir(playlist_url);
    let mut urls = Vec::new();
    for line in playlist.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        urls.push(resolve_url(&base, line));
    }
    Ok(urls)
}

fn parse_bandwidth(line: &str) -> u64 {
    let Some(idx) = line.find("BANDWIDTH=") else {
        return 0;
    };
    let rest = &line[idx + "BANDWIDTH=".len()..];
    let digits: String = rest
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().unwrap_or(0)
}

fn url_dir(url: &str) -> String {
    match url.rfind('/') {
        Some(i) => url[..=i].to_string(),
        None => String::new(),
    }
}

fn resolve_url(base: &str, relative: &str) -> String {
    if is_stream_url(relative) {
        return relative.to_string();
    }
    if relative.starts_with('/') {
        if let Some(scheme_end) = base.find("://") {
            if let Some(host_end) = base[scheme_end + 3..].find('/') {
                let origin = &base[..scheme_end + 3 + host_end];
                return format!("{origin}{relative}");
            }
        }
    }
    format!("{base}{relative}")
}

fn guess_extension(url: &str, content_type: Option<&reqwest::header::HeaderValue>) -> String {
    if let Some(ct) = content_type.and_then(|v| v.to_str().ok()) {
        if ct.contains("mp4") {
            return "mp4".into();
        }
        if ct.contains("mpeg") || ct.contains("mp3") {
            return "mp3".into();
        }
    }
    Path::new(url)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "mp4".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_stream_urls() {
        assert!(is_stream_url("https://example.com/v.mp4"));
        assert!(is_stream_url("http://localhost/test.m3u8"));
        assert!(!is_stream_url("/local/file.mp4"));
        assert!(!is_stream_url("file.mp4"));
    }

    #[test]
    fn resolve_relative_hls_url() {
        let base = "https://cdn.example.com/live/";
        assert_eq!(
            resolve_url(base, "seg001.ts"),
            "https://cdn.example.com/live/seg001.ts"
        );
    }

    #[test]
    fn parse_bandwidth_tag() {
        assert_eq!(
            parse_bandwidth("#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=854x480"),
            1_280_000
        );
    }
}