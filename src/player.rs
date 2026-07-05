use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui_wgpu::Renderer as EguiRenderer;
use egui_winit::State as EguiWinitState;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Modifiers, WindowEvent};
use winit::keyboard::{Key, NamedKey};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use crate::audio::{AudioDecoder, AudioSink, PlaybackClock};
use crate::error::{PlayerError, Result};
use crate::i18n::{Language, Locale};
use crate::render::RenderPipeline;
use crate::stream::{is_stream_url, resolve_media_source};
use crate::subtitle::SubtitleTrack;
use crate::sync::AvSync;
use crate::ui::{apply_theme, draw_player_ui, setup_fonts, PlayerUiState, PlayerView};
use crate::video::{DecodedFrame, Mp4Demuxer, VideoDecodeWorker, VideoDecoder};

const MEDIA_EXTENSIONS: &[&str] = &["mp4", "m4a", "mp3", "ts"];

const AUDIO_PREFILL_SAMPLES: usize = 4096;

/// Per-frame diagnostic status shared between the decode worker and the player.
#[derive(Clone, Debug, Default)]
pub struct WorkerPerFrameStatus {
    pub demuxed_packets: u64,
    pub decoded_frames: u64,
    pub last_frame_pts: f64,
    pub worker_running: bool,
}

/// Aggregate playback status for UI consumption.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum WaitingReason {
    #[default]
    None,
    WaitingForFirstFrame,
    Decoding,
    SeekPending,
    CodecUnsupported(String),
    DemuxError(String),
    DecodeError(String),
    NoVideoTrack,
}

#[derive(Clone, Debug)]
pub struct PlaybackStatus {
    pub demuxed_packets: u64,
    pub decoded_frames: u64,
    pub uploaded_frames: u64,
    pub last_frame_pts: f64,
    pub last_error: Option<String>,
    pub has_render_frame: bool,
    pub waiting_reason: WaitingReason,
    pub worker_running: bool,
    pub is_seeking: bool,
    pub player_active: bool,
}

impl Default for PlaybackStatus {
    fn default() -> Self {
        Self {
            demuxed_packets: 0,
            decoded_frames: 0,
            uploaded_frames: 0,
            last_frame_pts: 0.0,
            last_error: None,
            has_render_frame: false,
            waiting_reason: WaitingReason::None,
            worker_running: false,
            is_seeking: false,
            player_active: false,
        }
    }
}

pub struct MediaPlayer {
    audio_decoder: Option<AudioDecoder>,
    video_worker: Option<VideoDecodeWorker>,
    audio_sink: AudioSink,
    clock: Arc<PlaybackClock>,
    av_sync: AvSync,
    subtitles: Option<SubtitleTrack>,
    pending_audio: Vec<f32>,
    output_playing: bool,
    last_volume: f32,
    wall_clock_drive: bool,
    wall_anchor_secs: f64,
    wall_anchor_time: Option<Instant>,
    last_video_frame: Option<Arc<DecodedFrame>>,
    /// Shared status from the decode worker thread.
    worker_status: Arc<Mutex<WorkerPerFrameStatus>>,
    /// Track upload count for the UI.
    uploaded_frames: Arc<AtomicU64>,
    /// Accumulated errors or waiting reason.
    waiting_reason: WaitingReason,
}

impl MediaPlayer {
    pub fn open(path: &Path) -> Result<Self> {
        let demuxer = Mp4Demuxer::open(path).ok();
        let video_info = demuxer.as_ref().map(|d| {
            (
                d.video_codec(),
                d.extradata().to_vec(),
                d.duration_secs(),
            )
        });
        let has_video = video_info.is_some();

        let audio_decoder = AudioDecoder::open(path).ok();
        let (sample_rate, channels, duration) = if let Some(ref dec) = audio_decoder {
            (dec.sample_rate(), dec.channels(), dec.duration_secs())
        } else if let Some((_, _, dur)) = video_info {
            (48_000, 2, dur)
        } else {
            return Err(PlayerError::AudioDecode("no audio or video track".into()));
        };

        let clock = Arc::new(PlaybackClock::new(sample_rate, duration));
        if let Some(d) = duration {
            clock.set_duration_secs(d);
        }

        let audio_sink = AudioSink::try_new(sample_rate, channels, clock.clone())?;
        let video_worker = if let Some((codec, extradata, _)) = video_info {
            Some(VideoDecodeWorker::spawn(
                path.to_path_buf(),
                codec,
                extradata,
            )?)
        } else {
            None
        };
        let av_sync = AvSync::new(clock.clone());
        let subtitles = SubtitleTrack::load_sidecar(path);
        let wall_clock_drive =
            audio_sink.is_virtual() || (audio_decoder.is_none() && has_video);

        let worker_status = video_worker
            .as_ref()
            .map(|w| w.status_handle())
            .unwrap_or_default();

        Ok(Self {
            audio_decoder,
            video_worker,
            audio_sink,
            clock,
            av_sync,
            subtitles,
            pending_audio: Vec::new(),
            output_playing: false,
            last_volume: 1.0,
            wall_clock_drive,
            wall_anchor_secs: 0.0,
            wall_anchor_time: None,
            last_video_frame: None,
            worker_status,
            uploaded_frames: Arc::new(AtomicU64::new(0)),
            waiting_reason: WaitingReason::WaitingForFirstFrame,
        })
    }

    fn start_wall_clock(&mut self) {
        self.wall_anchor_secs = self.clock.position_secs();
        self.wall_anchor_time = Some(Instant::now());
    }

    fn stop_wall_clock(&mut self) {
        if let Some(start) = self.wall_anchor_time {
            self.wall_anchor_secs += start.elapsed().as_secs_f64();
        }
        self.wall_anchor_time = None;
    }

    fn advance_wall_clock(&mut self) {
        if let Some(start) = self.wall_anchor_time {
            let pos = self.wall_anchor_secs + start.elapsed().as_secs_f64();
            self.clock.seek(pos);
        }
    }

    pub fn uses_virtual_audio(&self) -> bool {
        self.audio_sink.is_virtual()
    }

    pub fn subtitle_at(&self, time_secs: f64) -> Option<&str> {
        self.subtitles
            .as_ref()
            .and_then(|t| t.cue_at(time_secs))
            .map(|c| c.text.as_str())
    }

    pub fn clock(&self) -> Arc<PlaybackClock> {
        self.clock.clone()
    }

    pub fn tick(&mut self, playing: bool) -> Option<Arc<DecodedFrame>> {
        if playing {
            if !self.output_playing {
                self.audio_sink.resume();
                self.output_playing = true;
                if self.wall_clock_drive {
                    self.start_wall_clock();
                }
            }
        } else {
            if self.output_playing {
                self.audio_sink.pause();
                if self.wall_clock_drive {
                    self.stop_wall_clock();
                }
                self.output_playing = false;
            }
            return self.last_video_frame.clone();
        }

        if self.wall_clock_drive {
            self.advance_wall_clock();
        }

        let channels = self.audio_sink.channels() as usize;
        let prefill_threshold = self.audio_sink.sample_rate() as usize * channels / 5;

        if let Some(dec) = &mut self.audio_decoder {
            while self.pending_audio.len() < prefill_threshold {
                match dec.decode_next() {
                    Ok(Some(buf)) => self.pending_audio.extend(buf.samples),
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }
        if !self.pending_audio.is_empty() {
            let chunk = self.pending_audio.len().min(AUDIO_PREFILL_SAMPLES);
            let _ = self.audio_sink.write(&self.pending_audio[..chunk]);
            self.pending_audio.drain(..chunk);
        }

        if let Some(worker) = &self.video_worker {
            worker.poll_frames(&mut self.av_sync);
            // Read worker diagnostic counters
            if let Ok(s) = self.worker_status.lock() {
                if s.worker_running && s.decoded_frames == 0 && self.clock.position_secs() > 1.0 {
                    self.waiting_reason = WaitingReason::Decoding;
                }
            }
        }

        if let Some(frame) = self.av_sync.pop_frame_for_display() {
            let frame = Arc::new(frame);
            self.last_video_frame = Some(frame.clone());
            self.waiting_reason = WaitingReason::None;
            Some(frame)
        } else {
            if self.waiting_reason == WaitingReason::None
                && self.video_worker.is_some()
                && self.last_video_frame.is_none()
            {
                self.waiting_reason = WaitingReason::WaitingForFirstFrame;
            }
            self.last_video_frame.clone()
        }
    }

    /// Returns the aggregate playback status for UI.
    pub fn playback_status(&self) -> PlaybackStatus {
        let (worker_demuxed, worker_decoded, worker_pts, worker_running) = self
            .worker_status
            .lock()
            .map(|s| (s.demuxed_packets, s.decoded_frames, s.last_frame_pts, s.worker_running))
            .unwrap_or((0, 0, 0.0, false));

        let has_render = self.last_video_frame.is_some();
        let uploaded = self.uploaded_frames.load(Ordering::Relaxed);

        PlaybackStatus {
            demuxed_packets: worker_demuxed,
            decoded_frames: worker_decoded,
            uploaded_frames: uploaded,
            last_frame_pts: worker_pts,
            last_error: match &self.waiting_reason {
                WaitingReason::CodecUnsupported(m) => Some(m.clone()),
                WaitingReason::DemuxError(m) => Some(m.clone()),
                WaitingReason::DecodeError(m) => Some(m.clone()),
                _ => None,
            },
            has_render_frame: has_render,
            waiting_reason: self.waiting_reason.clone(),
            worker_running,
            is_seeking: self.av_sync.is_seeking(),
            player_active: self.output_playing,
        }
    }

    pub fn set_volume(&self, level: f32) {
        self.audio_sink.set_volume(level);
    }

    pub fn handle_ui(&mut self, state: &mut PlayerUiState) {
        if (state.volume - self.last_volume).abs() > f32::EPSILON {
            self.set_volume(state.volume);
            self.last_volume = state.volume;
        }
        if state.stop_playback {
            let _ = self.seek(0.0);
            state.is_playing = false;
            state.stop_playback = false;
        }
        if state.skip_forward {
            let pos = (self.clock.position_secs() + 10.0)
                .min(self.clock.duration_secs().unwrap_or(f64::MAX));
            let _ = self.seek(pos);
            state.skip_forward = false;
        }
        if state.skip_backward {
            let pos = (self.clock.position_secs() - 10.0).max(0.0);
            let _ = self.seek(pos);
            state.skip_backward = false;
        }
        if let Some(target) = state.seek_target.take() {
            let _ = self.seek(target);
        }
    }

    pub fn seek(&mut self, position_secs: f64) -> Result<()> {
        self.audio_sink.clear();
        self.pending_audio.clear();
        self.av_sync.clear();
        // Do NOT clear last_video_frame — keep it displayed until new frames arrive.
        if let Some(dec) = &mut self.audio_decoder {
            dec.seek(position_secs)?;
        }
        self.clock.seek(position_secs);
        self.wall_anchor_secs = position_secs;
        if self.output_playing && self.wall_clock_drive {
            self.wall_anchor_time = Some(Instant::now());
        } else {
            self.wall_anchor_time = None;
        }
        if let Some(worker) = &self.video_worker {
            worker.seek(position_secs);
        }
        self.av_sync.set_seeking(true);
        self.waiting_reason = WaitingReason::SeekPending;
        Ok(())
    }

    pub fn sync_stats(&self) -> (u64, u64, u64) {
        (
            self.av_sync.dropped_late,
            self.av_sync.dropped_overflow,
            self.av_sync.waited_early,
        )
    }
}

pub fn run_player(path: Option<&Path>, no_ui: bool, lang: Language) -> Result<()> {
    let resolved = path
        .map(|p| {
            let s = p.to_string_lossy();
            if is_stream_url(&s) {
                resolve_media_source(&s)
            } else {
                Ok(p.to_path_buf())
            }
        })
        .transpose()?;

    match (resolved.as_deref(), no_ui) {
        (Some(p), true) => run_headless(p),
        (None, true) => {
            let loc = Locale::new(lang);
            Err(PlayerError::Unsupported(loc.headless_requires_path().into()))
        }
        (path, false) => run_windowed(path, lang),
    }
}

fn run_headless(path: &Path) -> Result<()> {
    let mut player = MediaPlayer::open(path)?;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        player.tick(true);
        if player
            .clock()
            .duration_secs()
            .is_some_and(|d| player.clock().position_secs() >= d)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    let (late, overflow, early) = player.sync_stats();
    log::info!("sync stats: dropped_late={late} overflow={overflow} waited_early={early}");
    Ok(())
}

struct PlayerApp {
    window: Option<Arc<Window>>,
    render: Option<RenderPipeline>,
    egui_ctx: egui::Context,
    egui_winit: Option<EguiWinitState>,
    egui_renderer: Option<EguiRenderer>,
    player: Option<MediaPlayer>,
    ui_state: PlayerUiState,
    path: Option<PathBuf>,
    file_hover: bool,
    load_error: Option<String>,
    load_warning: Option<String>,
    theme_applied: bool,
    fonts_applied: bool,
    modifiers: Modifiers,
    locale: Locale,
    stream_temp: Option<PathBuf>,
}

impl PlayerApp {
    fn new(path: Option<PathBuf>, lang: Language) -> Self {
        Self {
            window: None,
            render: None,
            egui_ctx: egui::Context::default(),
            egui_winit: None,
            egui_renderer: None,
            player: None,
            ui_state: PlayerUiState::default(),
            path,
            file_hover: false,
            load_error: None,
            load_warning: None,
            theme_applied: false,
            fonts_applied: false,
            modifiers: Modifiers::default(),
            locale: Locale::new(lang),
            stream_temp: None,
        }
    }

    fn load_media(&mut self, path: PathBuf) {
        let path_str = path.to_string_lossy();
        let resolved = if is_stream_url(&path_str) {
            match resolve_media_source(&path_str) {
                Ok(p) => {
                    self.stream_temp = Some(p.clone());
                    p
                }
                Err(e) => {
                    self.load_error = Some(e.to_string());
                    return;
                }
            }
        } else {
            path
        };

        if !is_media_file(&resolved) {
            let ext = resolved
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_else(|| self.locale.unknown_extension().to_string());
            self.load_error = Some(self.locale.unsupported_file_type(&ext));
            return;
        }

        match MediaPlayer::open(&resolved) {
            Ok(player) => {
                self.load_warning = if player.uses_virtual_audio() {
                    Some(self.locale.virtual_audio_warning().to_string())
                } else {
                    None
                };
                self.player = Some(player);
                self.path = Some(resolved.clone());
                self.ui_state.is_playing = true;
                self.ui_state.seek_preview = None;
                self.load_error = None;
                self.set_window_title(&resolved);
                log::info!("loaded media: {}", resolved.display());
            }
            Err(e) => {
                self.load_error = Some(e.to_string());
                log::error!("failed to open {}: {e}", resolved.display());
            }
        }
    }

    fn set_window_title(&self, path: &Path) {
        let Some(window) = &self.window else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.locale.window_title_default());
        window.set_title(&self.locale.window_title(&name));
    }

    fn pick_file_dialog(&mut self) {
        let loc = self.locale;
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(loc.file_dialog_filter(), MEDIA_EXTENSIONS)
            .set_title(loc.file_dialog_title())
            .pick_file()
        {
            self.load_media(path);
        }
    }
}

fn is_media_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lower = e.to_ascii_lowercase();
            MEDIA_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

impl ApplicationHandler for PlayerApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(self.locale.window_title_default())
                        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0)),
                )
                .expect("create window"),
        );

        let render = RenderPipeline::new(window.clone()).expect("render pipeline");
        let egui_winit = EguiWinitState::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            event_loop,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let egui_renderer =
            EguiRenderer::new(render.device(), render.format(), None, 1, false);

        if let Some(path) = self.path.clone() {
            self.load_media(path);
        }

        window.request_redraw();
        self.window = Some(window);
        self.render = Some(render);
        self.egui_winit = Some(egui_winit);
        self.egui_renderer = Some(egui_renderer);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let Some(egui_winit) = self.egui_winit.as_mut() else {
            return;
        };

        match &event {
            WindowEvent::DroppedFile(path) => {
                self.file_hover = false;
                self.load_media(path.clone());
                window.request_redraw();
                return;
            }
            WindowEvent::HoveredFile(_) => {
                self.file_hover = true;
                window.request_redraw();
            }
            WindowEvent::HoveredFileCancelled => {
                self.file_hover = false;
                window.request_redraw();
            }
            _ => {}
        }

        let response = egui_winit.on_window_event(&window, &event);
        if response.consumed {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    match event.logical_key {
                        Key::Named(NamedKey::Space) => {
                            if self.player.is_some() {
                                self.ui_state.is_playing = !self.ui_state.is_playing;
                                window.request_redraw();
                            }
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            self.ui_state.skip_backward = true;
                            window.request_redraw();
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            self.ui_state.skip_forward = true;
                            window.request_redraw();
                        }
                        Key::Character(ref s)
                            if s.eq_ignore_ascii_case("o")
                                && self.modifiers.state().control_key() =>
                        {
                            self.pick_file_dialog();
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(render) = &mut self.render {
                    render.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw(&window);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            let needs_redraw = self.ui_state.is_playing
                || self.file_hover
                || self.load_error.is_some()
                || self.ui_state.seek_drag
                || self.player.is_none();
            if needs_redraw {
                window.request_redraw();
            }
        }
    }
}

impl PlayerApp {
    fn redraw(&mut self, window: &Window) {
        if !self.fonts_applied {
            setup_fonts(&self.egui_ctx);
            self.fonts_applied = true;
        }
        if !self.theme_applied {
            apply_theme(&self.egui_ctx);
            self.theme_applied = true;
        }

        {
            let render = self.render.as_mut().unwrap();
            if let Some(player) = self.player.as_mut() {
                player.handle_ui(&mut self.ui_state);
                if let Some(frame) = player.tick(self.ui_state.is_playing) {
                    render.upload_frame(&frame);
                    player.uploaded_frames.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        let raw_input = self
            .egui_winit
            .as_mut()
            .unwrap()
            .take_egui_input(window);

        let filename = self.path.as_ref().and_then(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
        });
        let clock = self.player.as_ref().map(|p| p.clock());
        let subtitle = self
            .player
            .as_ref()
            .and_then(|p| p.subtitle_at(p.clock().position_secs()));
        let playback_status = self
            .player
            .as_ref()
            .map(|p| p.playback_status());
        let view = PlayerView {
            clock: clock.as_ref().map(|c| c.as_ref()),
            filename: filename.as_deref(),
            has_media: self.player.is_some(),
            drag_active: self.file_hover,
            error: self.load_error.as_deref(),
            warning: self.load_warning.as_deref(),
            subtitle,
            playback_status: playback_status.as_ref(),
            render_has_frame: self.render.as_ref().map(|r| r.has_frame()).unwrap_or(false),
            render_uploaded: self.render.as_ref().map(|r| r.uploaded_frame_count()).unwrap_or(0),
        };

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            draw_player_ui(ctx, &view, &mut self.ui_state, &mut self.locale);
        });

        let open_file = self.ui_state.open_file;
        if open_file {
            self.ui_state.open_file = false;
        }

        self.egui_winit
            .as_mut()
            .unwrap()
            .handle_platform_output(window, full_output.platform_output);

        if open_file {
            self.pick_file_dialog();
        }

        let render = self.render.as_mut().unwrap();
        let egui_renderer = self.egui_renderer.as_mut().unwrap();

        // Handle surface errors gracefully — don't panic on Lost/Outdated.
        let output = match render.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost) => {
                log::warn!("surface lost, reconfiguring");
                render.reconfigure_surface();
                window.request_redraw();
                return;
            }
            Err(wgpu::SurfaceError::Outdated) => {
                log::debug!("surface outdated, skipping frame");
                window.request_redraw();
                return;
            }
            Err(e) => {
                log::error!("surface error: {e}");
                window.request_redraw();
                return;
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = render
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("player_encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            if let Some(bind_group) = &render.bind_group {
                pass.set_pipeline(&render.pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }

        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [render.config.width, render.config.height],
            pixels_per_point: window.scale_factor() as f32,
        };

        let primitives = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, delta) in &full_output.textures_delta.set {
            egui_renderer.update_texture(&render.device, &render.queue, *id, delta);
        }
        egui_renderer.update_buffers(
            &render.device,
            &render.queue,
            &mut encoder,
            &primitives,
            &screen_desc,
        );

        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    occlusion_query_set: None,
                    timestamp_writes: None,
                })
                .forget_lifetime();
            egui_renderer.render(&mut pass, &primitives, &screen_desc);
        }

        render.queue.submit(std::iter::once(encoder.finish()));
        for id in &full_output.textures_delta.free {
            egui_renderer.free_texture(id);
        }
        output.present();
    }
}

pub fn run_render_only(path: &Path) -> Result<()> {
    struct RenderApp {
        window: Option<Arc<Window>>,
        render: Option<RenderPipeline>,
        demuxer: Option<Mp4Demuxer>,
        decoder: VideoDecoder,
        path: PathBuf,
    }

    impl ApplicationHandler for RenderApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }
            let window = Arc::new(
                event_loop
                    .create_window(
                        Window::default_attributes()
                            .with_title("rust-player render")
                            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0)),
                    )
                    .expect("create window"),
            );
            let render = RenderPipeline::new(window.clone()).expect("render");
            let demuxer = Mp4Demuxer::open(&self.path).ok();
            let decoder = demuxer
                .as_ref()
                .map(|d| VideoDecoder::for_codec(d.video_codec(), d.extradata()))
                .transpose()
                .expect("decoder")
                .unwrap_or_else(|| VideoDecoder::for_codec(crate::video::VideoCodec::Av1, &[]).expect("decoder"));
            window.request_redraw();
            self.window = Some(window);
            self.render = Some(render);
            self.demuxer = demuxer;
            self.decoder = decoder;
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Resized(size) => {
                    if let Some(r) = &mut self.render {
                        r.resize(size.width, size.height);
                    }
                }
                WindowEvent::RedrawRequested => {
                    if let (Some(render), Some(demuxer)) =
                        (self.render.as_mut(), self.demuxer.as_mut())
                    {
                        if let Ok(Some(packet)) = demuxer.next_packet() {
                            if let Ok(frames) = self.decoder.decode(&packet) {
                                if let Some(frame) = frames.last() {
                                    render.upload_frame(frame);
                                    if let Some(w) = &self.window {
                                        w.set_title(&format!(
                                            "PTS={:.3}s {}x{}",
                                            frame.pts_secs, frame.width, frame.height
                                        ));
                                    }
                                }
                            }
                        }
                        // Handle surface errors gracefully (same pattern as PlayerApp::redraw).
                        let output = match render.surface.get_current_texture() {
                            Ok(t) => t,
                            Err(wgpu::SurfaceError::Lost) => {
                                log::warn!("render-only surface lost, reconfiguring");
                                render.reconfigure_surface();
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                                return;
                            }
                            Err(wgpu::SurfaceError::Outdated) => {
                                log::debug!("render-only surface outdated, skipping frame");
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                                return;
                            }
                            Err(e) => {
                                log::error!("render-only surface error: {e}");
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                                return;
                            }
                        };
                        let view = output
                            .texture
                            .create_view(&wgpu::TextureViewDescriptor::default());
                        let mut encoder = render
                            .device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("render_only_encoder"),
                            });
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("render_only_pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &view,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: None,
                                occlusion_query_set: None,
                                timestamp_writes: None,
                            });
                            if let Some(bind_group) = &render.bind_group {
                                pass.set_pipeline(&render.pipeline);
                                pass.set_bind_group(0, bind_group, &[]);
                                pass.draw(0..6, 0..1);
                            }
                        }
                        render.queue.submit(std::iter::once(encoder.finish()));
                        output.present();
                    }
                    if let Some(w) = &self.window {
                        w.request_redraw();
                    }
                }
                _ => {}
            }
        }

        fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    let event_loop = EventLoop::new().map_err(|e| PlayerError::Render(e.to_string()))?;
    let mut app = RenderApp {
        window: None,
        render: None,
        demuxer: None,
        decoder: VideoDecoder::for_codec(crate::video::VideoCodec::Av1, &[])?,
        path: path.to_path_buf(),
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| PlayerError::Render(e.to_string()))?;
    Ok(())
}

fn run_windowed(path: Option<&Path>, lang: Language) -> Result<()> {
    let event_loop = EventLoop::new().map_err(|e| PlayerError::Render(e.to_string()))?;
    let mut app = PlayerApp::new(path.map(Path::to_path_buf), lang);
    event_loop
        .run_app(&mut app)
        .map_err(|e| PlayerError::Render(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_av1_path() -> Option<PathBuf> {
        let path = PathBuf::from("assets/test_av1.mp4");
        path.exists().then_some(path)
    }

    #[test]
    fn seek_stress_no_panic() {
        let Some(path) = test_av1_path() else {
            return;
        };
        let mut player = MediaPlayer::open(&path).expect("open");
        for i in 0..10 {
            let pos = (i as f64) * 0.3;
            player.seek(pos).expect("seek");
            let _ = player.tick(false);
        }
    }

    #[test]
    fn pause_play_stress_no_panic() {
        let Some(path) = test_av1_path() else {
            return;
        };
        let mut player = MediaPlayer::open(&path).expect("open");
        for playing in [true, false].into_iter().cycle().take(20) {
            let _ = player.tick(playing);
        }
    }

    #[test]
    fn media_extension_filter() {
        assert!(is_media_file(Path::new("video.mp4")));
        assert!(is_media_file(Path::new("audio.MP3")));
        assert!(!is_media_file(Path::new("readme.txt")));
    }
}