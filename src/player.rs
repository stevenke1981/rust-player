use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egui_wgpu::Renderer as EguiRenderer;
use egui_winit::State as EguiWinitState;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, Modifiers, WindowEvent};
use winit::keyboard::{Key, NamedKey};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

use crate::audio::{AudioDecoder, AudioOutput, PlaybackClock};
use crate::error::{PlayerError, Result};
use crate::i18n::{Language, Locale};
use crate::render::RenderPipeline;
use crate::sync::AvSync;
use crate::ui::{apply_theme, draw_player_ui, PlayerUiState, PlayerView};
use crate::video::{Av1Decoder, DecodedFrame, Mp4Demuxer};

const MEDIA_EXTENSIONS: &[&str] = &["mp4", "m4a", "mp3"];

pub struct MediaPlayer {
    audio_decoder: Option<AudioDecoder>,
    video_demuxer: Option<Mp4Demuxer>,
    video_decoder: Av1Decoder,
    audio_output: AudioOutput,
    clock: Arc<PlaybackClock>,
    av_sync: AvSync,
    pending_audio: Vec<f32>,
    has_video: bool,
}

impl MediaPlayer {
    pub fn open(path: &Path) -> Result<Self> {
        let video_demuxer = Mp4Demuxer::open(path).ok();
        let has_video = video_demuxer.is_some();

        let audio_decoder = AudioDecoder::open(path).ok();
        let (sample_rate, channels, duration) = if let Some(ref dec) = audio_decoder {
            (dec.sample_rate(), dec.channels(), dec.duration_secs())
        } else if has_video {
            (48_000, 2, Some(10.0))
        } else {
            return Err(crate::error::PlayerError::AudioDecode(
                "no audio or video track".into(),
            ));
        };

        let clock = Arc::new(PlaybackClock::new(sample_rate, duration));
        if let Some(d) = duration {
            clock.set_duration_secs(d);
        }

        let audio_output = AudioOutput::new(sample_rate, channels, clock.clone())?;
        let video_decoder = Av1Decoder::new()?;
        let av_sync = AvSync::new(clock.clone());

        Ok(Self {
            audio_decoder,
            video_demuxer,
            video_decoder,
            audio_output,
            clock,
            av_sync,
            pending_audio: Vec::new(),
            has_video,
        })
    }

    pub fn clock(&self) -> Arc<PlaybackClock> {
        self.clock.clone()
    }

    pub fn tick(&mut self, playing: bool) -> Option<DecodedFrame> {
        if playing {
            self.audio_output.resume();
        } else {
            self.audio_output.pause();
            return None;
        }

        if let Some(dec) = &mut self.audio_decoder {
            if self.pending_audio.len() < self.audio_output.sample_rate() as usize * 2 {
                if let Ok(Some(buf)) = dec.decode_next() {
                    self.pending_audio.extend(buf.samples);
                }
            }
        }
        if !self.pending_audio.is_empty() {
            let chunk = self.pending_audio.len().min(4096);
            let samples: Vec<f32> = self.pending_audio.drain(..chunk).collect();
            let _ = self.audio_output.write(&samples);
        }

        if self.has_video {
            if let Some(demuxer) = &mut self.video_demuxer {
                if let Ok(Some(packet)) = demuxer.next_packet() {
                    if let Ok(frames) = self.video_decoder.decode(&packet) {
                        for frame in frames {
                            self.av_sync.push_frame(frame);
                        }
                    }
                }
            }
        }

        self.av_sync.pop_frame_for_display()
    }

    pub fn set_volume(&self, level: f32) {
        self.audio_output.set_volume(level);
    }

    pub fn handle_ui(&mut self, state: &mut PlayerUiState) {
        self.set_volume(state.volume);
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
        self.audio_output.clear();
        self.pending_audio.clear();
        self.av_sync.clear();
        if let Some(dec) = &mut self.audio_decoder {
            dec.seek(position_secs)?;
        }
        self.clock.seek(position_secs);
        if let Some(demuxer) = &mut self.video_demuxer {
            demuxer.seek(position_secs)?;
        }
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
    match (path, no_ui) {
        (Some(p), true) => run_headless(p),
        (_, true) => {
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
    theme_applied: bool,
    modifiers: Modifiers,
    locale: Locale,
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
            theme_applied: false,
            modifiers: Modifiers::default(),
            locale: Locale::new(lang),
        }
    }

    fn load_media(&mut self, path: PathBuf) {
        if !is_media_file(&path) {
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_else(|| self.locale.unknown_extension().to_string());
            self.load_error = Some(self.locale.unsupported_file_type(&ext));
            return;
        }

        match MediaPlayer::open(&path) {
            Ok(player) => {
                self.player = Some(player);
                self.path = Some(path.clone());
                self.ui_state.is_playing = true;
                self.ui_state.seek_preview = None;
                self.load_error = None;
                self.set_window_title(&path);
                log::info!("loaded media: {}", path.display());
            }
            Err(e) => {
                self.load_error = Some(e.to_string());
                log::error!("failed to open {}: {e}", path.display());
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
        let _ = window.set_title(&self.locale.window_title(&name));
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
                        .with_title(&self.locale.window_title_default())
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
                            }
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            self.ui_state.skip_backward = true;
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            self.ui_state.skip_forward = true;
                        }
                        Key::Character(ref s) if s.eq_ignore_ascii_case("o") => {
                            if self.modifiers.state().control_key() {
                                self.pick_file_dialog();
                            }
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
            window.request_redraw();
        }
    }
}

impl PlayerApp {
    fn redraw(&mut self, window: &Window) {
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
        let view = PlayerView {
            clock: clock.as_ref().map(|c| c.as_ref()),
            filename: filename.as_deref(),
            has_media: self.player.is_some(),
            drag_active: self.file_hover,
            error: self.load_error.as_deref(),
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

        let output = render
            .surface
            .get_current_texture()
            .expect("surface texture");
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
        decoder: Av1Decoder,
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
            let decoder = Av1Decoder::new().expect("decoder");
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
                                        let _ = w.set_title(&format!(
                                            "PTS={:.3}s {}x{}",
                                            frame.pts_secs, frame.width, frame.height
                                        ));
                                    }
                                }
                            }
                        }
                        let _ = render.render();
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
        decoder: Av1Decoder::new()?,
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