use std::env;
use std::path::PathBuf;
use std::process;
use std::time::Duration;

use rust_player::audio::AudioPlayer;
use rust_player::i18n::{Language, Locale};
use rust_player::player;
use rust_player::video;

struct CliOptions {
    lang: Language,
    no_ui: bool,
    path_args: Vec<String>,
}

impl CliOptions {
    fn from_args(args: &[String]) -> Self {
        let mut lang = Language::default();
        let mut no_ui = false;
        let mut path_args = Vec::new();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--lang" if i + 1 < args.len() => {
                    if let Some(l) = Language::parse(&args[i + 1]) {
                        lang = l;
                    }
                    i += 2;
                }
                "--no-ui" => {
                    no_ui = true;
                    i += 1;
                }
                other => {
                    path_args.push(other.to_string());
                    i += 1;
                }
            }
        }
        Self {
            lang,
            no_ui,
            path_args,
        }
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        if let Err(e) = player::run_player(None, false, Language::default()) {
            eprintln!("error: {e}");
            process::exit(1);
        }
        return;
    }

    let opts = CliOptions::from_args(&args[1..]);

    let result = match opts.path_args.first().map(String::as_str) {
        None => player::run_player(None, opts.no_ui, opts.lang),
        Some("audio") => run_audio(&opts.path_args[1..]),
        Some("decode") => run_decode(&opts.path_args[1..]),
        Some("render") => run_render(&opts.path_args[1..]),
        Some("help") | Some("-h") | Some("--help") => {
            print_usage(opts.lang);
            Ok(())
        }
        Some(path) => player::run_player(
            Some(PathBuf::from(path).as_path()),
            opts.no_ui,
            opts.lang,
        ),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        process::exit(1);
    }
}

fn run_audio(args: &[String]) -> rust_player::Result<()> {
    if args.is_empty() {
        eprintln!("usage: rust-player audio <file> [--progress]");
        process::exit(1);
    }
    let path = PathBuf::from(&args[0]);
    let progress = args.iter().any(|a| a == "--progress");
    let mut player = AudioPlayer::open(&path)?;
    if progress {
        player.play_with_progress(Duration::from_millis(500))?;
    } else {
        player.play_blocking()?;
    }
    Ok(())
}

fn run_decode(args: &[String]) -> rust_player::Result<()> {
    if args.is_empty() {
        eprintln!("usage: rust-player decode <file> [--frames N]");
        process::exit(1);
    }
    let path = PathBuf::from(&args[0]);
    let mut frames = 30usize;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--frames" && i + 1 < args.len() {
            frames = args[i + 1].parse().unwrap_or(30);
            i += 2;
        } else {
            i += 1;
        }
    }
    video::decode_file(&path, frames)
}

fn run_render(args: &[String]) -> rust_player::Result<()> {
    if args.is_empty() {
        eprintln!("usage: rust-player render <file>");
        process::exit(1);
    }
    let path = PathBuf::from(&args[0]);
    player::run_render_only(&path)
}

fn print_usage(lang: Language) {
    let loc = Locale::new(lang);
    eprintln!("{}", loc.usage());
}