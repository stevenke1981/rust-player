use std::io;

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("IO: {0}")]
    Io(#[from] io::Error),
    #[error("Audio decode: {0}")]
    AudioDecode(String),
    #[error("Video demux: {0}")]
    VideoDemux(String),
    #[error("Video decode: {0}")]
    VideoDecode(String),
    #[error("Render: {0}")]
    Render(String),
    #[error("Unsupported format: {0}")]
    Unsupported(String),
    #[error("Audio output: {0}")]
    AudioOutput(String),
}

pub type Result<T> = std::result::Result<T, PlayerError>;