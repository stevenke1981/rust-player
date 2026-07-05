mod fonts;
mod theme;

use egui::{Align2, Color32, FontId, Frame, Order, RichText, Stroke};

use crate::audio::PlaybackClock;
use crate::i18n::{Language, Locale};
use crate::player::{PlaybackStatus, WaitingReason};

pub use fonts::setup_fonts;
pub use theme::apply_theme;

pub struct PlayerUiState {
    pub is_playing: bool,
    pub seek_target: Option<f64>,
    pub seek_drag: bool,
    pub seek_preview: Option<f64>,
    pub skip_forward: bool,
    pub skip_backward: bool,
    pub volume: f32,
    pub open_file: bool,
    pub stop_playback: bool,
}

impl Default for PlayerUiState {
    fn default() -> Self {
        Self {
            is_playing: true,
            seek_target: None,
            seek_drag: false,
            seek_preview: None,
            skip_forward: false,
            skip_backward: false,
            volume: 1.0,
            open_file: false,
            stop_playback: false,
        }
    }
}

pub struct PlayerView<'a> {
    pub clock: Option<&'a PlaybackClock>,
    pub filename: Option<&'a str>,
    pub has_media: bool,
    pub drag_active: bool,
    pub error: Option<&'a str>,
    pub warning: Option<&'a str>,
    pub subtitle: Option<&'a str>,
    pub playback_status: Option<&'a PlaybackStatus>,
    pub render_has_frame: bool,
    pub render_uploaded: u64,
}

pub fn draw_player_ui(
    ctx: &egui::Context,
    view: &PlayerView<'_>,
    state: &mut PlayerUiState,
    locale: &mut Locale,
) {
    draw_title_bar(ctx, view, state, locale);
    draw_control_bar(ctx, view, state, locale);
    draw_drop_overlay(ctx, view, locale);
    draw_subtitle_overlay(ctx, view.subtitle);
    draw_warning_toast(ctx, view.warning);
    draw_error_toast(ctx, view.error);
    draw_status_overlay(ctx, view);
}

fn draw_language_selector(ui: &mut egui::Ui, locale: &mut Locale) {
    let mut current = locale.language();
    ui.label(
        RichText::new(locale.language_label())
            .size(11.0)
            .color(theme::TEXT_DIM),
    );
    egui::ComboBox::from_id_salt("language")
        .selected_text(current.native_name())
        .width(108.0)
        .show_ui(ui, |ui| {
            for lang in Language::ALL {
                ui.selectable_value(&mut current, lang, lang.native_name());
            }
        });
    locale.set_language(current);
}

fn draw_title_bar(
    ctx: &egui::Context,
    view: &PlayerView<'_>,
    state: &mut PlayerUiState,
    locale: &mut Locale,
) {
    let loc = *locale;
    egui::TopBottomPanel::top("title_bar")
        .frame(
            Frame::new()
                .fill(theme::BAR_BG)
                .inner_margin(egui::Margin::symmetric(16, 10))
                .stroke(Stroke::new(1.0, Color32::from_rgb(40, 40, 46))),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(loc.app_name())
                        .strong()
                        .size(16.0)
                        .color(theme::VLC_ORANGE),
                );
                ui.separator();
                if let Some(name) = view.filename {
                    ui.label(RichText::new(name).size(14.0));
                } else {
                    ui.label(
                        RichText::new(loc.no_media_open())
                            .italics()
                            .color(theme::TEXT_DIM),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    draw_language_selector(ui, locale);
                    ui.separator();
                    if theme::transport_button(ui, loc.open_file(), false).clicked() {
                        state.open_file = true;
                    }
                    ui.label(
                        RichText::new(loc.supported_formats())
                            .size(11.0)
                            .color(theme::TEXT_DIM),
                    );
                });
            });
        });
}

fn draw_control_bar(
    ctx: &egui::Context,
    view: &PlayerView<'_>,
    state: &mut PlayerUiState,
    locale: &Locale,
) {
    let enabled = view.has_media;

    egui::TopBottomPanel::bottom("controls")
        .frame(
            Frame::new()
                .fill(theme::BAR_BG)
                .inner_margin(egui::Margin::symmetric(16, 10))
                .stroke(Stroke::new(1.0, Color32::from_rgb(40, 40, 46))),
        )
        .show(ctx, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.vertical(|ui| {
                    let (pos, dur) = view
                        .clock
                        .map(|c| (c.position_secs(), c.duration_secs().unwrap_or(0.0)))
                        .unwrap_or((0.0, 0.0));

                    let display_pos = state.seek_preview.unwrap_or(pos);
                    let fraction = if dur > 0.0 {
                        (display_pos / dur).clamp(0.0, 1.0) as f32
                    } else {
                        0.0
                    };

                    let (seek_resp, frac) = theme::paint_seek_bar(ui, fraction, state.seek_drag);
                    if seek_resp.drag_started() || seek_resp.clicked() {
                        state.seek_drag = true;
                    }
                    if state.seek_drag {
                        state.seek_preview = Some((frac as f64) * dur);
                        if seek_resp.drag_stopped()
                            || ui.input(|i| i.pointer.any_released())
                        {
                            if let Some(preview) = state.seek_preview.take() {
                                state.seek_target = Some(preview);
                            }
                            state.seek_drag = false;
                        }
                    }

                    ui.add_space(6.0);

                    ui.horizontal(|ui| {
                        let play_label = if state.is_playing { "⏸" } else { "▶" };
                        if theme::transport_button(ui, play_label, true).clicked() {
                            state.is_playing = !state.is_playing;
                        }
                        if theme::transport_button(ui, "⏹", false).clicked() {
                            state.stop_playback = true;
                        }
                        if theme::transport_button(ui, "⏪", false).clicked() {
                            state.skip_backward = true;
                        }
                        if theme::transport_button(ui, "⏩", false).clicked() {
                            state.skip_forward = true;
                        }

                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!(
                                "{} / {}",
                                format_time(display_pos),
                                format_time(dur)
                            ))
                            .monospace()
                            .size(13.0)
                            .color(theme::TEXT_MAIN),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let vol_pct = (state.volume * 100.0).round() as i32;
                            ui.label(
                                RichText::new(format!("{vol_pct}%"))
                                    .size(12.0)
                                    .color(theme::TEXT_DIM),
                            );
                            ui.add(
                                egui::Slider::new(&mut state.volume, 0.0..=1.0)
                                    .show_value(false)
                                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                            );
                            ui.label(RichText::new("🔊").size(14.0));
                        });
                    });
                });
            });

            if !enabled {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(locale.idle_hint())
                        .size(12.0)
                        .color(theme::TEXT_DIM),
                );
            }
        });
}

fn draw_drop_overlay(ctx: &egui::Context, view: &PlayerView<'_>, locale: &Locale) {
    let show = view.drag_active || !view.has_media;
    if !show {
        return;
    }

    let screen = ctx.input(|i| i.screen_rect());
    let active = view.drag_active;

    let title = if active {
        locale.drop_title_active()
    } else {
        locale.drop_title_idle()
    };
    let subtitle = if active {
        locale.drop_subtitle_active()
    } else {
        locale.drop_subtitle_idle()
    };

    egui::Area::new(egui::Id::new("drop_overlay"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.set_width(screen.width());
            ui.set_height(screen.height());

            if view.has_media && active {
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(140));
            }

            let card_size = egui::vec2(420.0, 220.0);
            let card_rect = Align2::CENTER_CENTER.align_size_within_rect(card_size, screen);

            let border_color = if active {
                theme::VLC_ORANGE
            } else {
                Color32::from_rgb(80, 80, 90)
            };
            let fill = if active {
                theme::VLC_ORANGE.gamma_multiply(0.12)
            } else {
                Color32::from_black_alpha(100)
            };

            let painter = ui.painter();
            painter.rect_filled(card_rect, 14.0, fill);
            painter.rect_stroke(
                card_rect,
                14.0,
                Stroke::new(if active { 3.0 } else { 2.0 }, border_color),
                egui::StrokeKind::Middle,
            );

            let center = card_rect.center();
            painter.text(
                center + egui::vec2(0.0, -36.0),
                Align2::CENTER_CENTER,
                "🎬",
                FontId::proportional(42.0),
                theme::TEXT_MAIN,
            );
            painter.text(
                center + egui::vec2(0.0, 8.0),
                Align2::CENTER_CENTER,
                title,
                FontId::proportional(20.0),
                if active {
                    theme::VLC_ORANGE
                } else {
                    theme::TEXT_MAIN
                },
            );
            painter.text(
                center + egui::vec2(0.0, 36.0),
                Align2::CENTER_CENTER,
                subtitle,
                FontId::proportional(13.0),
                theme::TEXT_DIM,
            );
        });
}

/// Draw a diagnostic overlay when media is loaded but no video frame is visible yet.
/// This replaces the silent black screen with informative status.
fn draw_status_overlay(ctx: &egui::Context, view: &PlayerView<'_>) {
    let Some(status) = &view.playback_status else {
        return;
    };

    // Only show when media is loaded but no render frame AND not actively showing video.
    if view.render_has_frame && status.waiting_reason == WaitingReason::None && !status.is_seeking {
        return;
    }

    let screen = ctx.input(|i| i.screen_rect());

    let (message, detail) = match &status.waiting_reason {
        WaitingReason::None if view.render_has_frame && status.is_seeking => {
            ("⏳ Seeking...", "Waiting for new video frame")
        }
        WaitingReason::None if !view.render_has_frame && status.decoded_frames > 0 => {
            ("🎬 Frame decoded", "Rendering display...")
        }
        WaitingReason::None if status.worker_running && status.decoded_frames == 0 => {
            ("⏳ Decoding", "Waiting for first video frame")
        }
        WaitingReason::None if !status.worker_running && status.decoded_frames == 0 => {
            ("⚠ Worker not running", "Check codec support or file format")
        }
        WaitingReason::WaitingForFirstFrame => {
            ("⏳ Waiting for first frame", "Decoder starting...")
        }
        WaitingReason::Decoding => {
            ("⏳ Decoding in progress", "Video frames are being decoded")
        }
        WaitingReason::SeekPending => {
            ("⏳ Seeking", "Loading new position...")
        }
        WaitingReason::CodecUnsupported(msg) => {
            ("✗ Unsupported codec", msg.as_str())
        }
        WaitingReason::DemuxError(msg) => {
            ("✗ Demux error", msg.as_str())
        }
        WaitingReason::DecodeError(msg) => {
            ("✗ Decode error", msg.as_str())
        }
        WaitingReason::NoVideoTrack => {
            ("ℹ Audio only", "No video track in this file")
        }
        WaitingReason::None => {
            if view.render_has_frame {
                ("▶ Playing", "Video frame displayed")
            } else if status.decoded_frames > 0 {
                ("🎬 Decoded", "Waiting for render pipeline")
            } else {
                ("⏳ Initializing", "Decoder starting...")
            }
        }
    };

    egui::Area::new(egui::Id::new("status_overlay"))
        .order(Order::Foreground)
        .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, -40.0))
        .show(ctx, |ui| {
            let card_width = 360.0;
            let card_height = 80.0;
            let card_rect = egui::Rect::from_min_size(
                egui::pos2(screen.center().x - card_width / 2.0, screen.center().y - card_height / 2.0 - 40.0),
                egui::vec2(card_width, card_height),
            );

            let painter = ui.painter();
            painter.rect_filled(card_rect, 10.0, Color32::from_black_alpha(180));
            painter.rect_stroke(
                card_rect,
                10.0,
                Stroke::new(1.0, Color32::from_rgb(100, 100, 120)),
                egui::StrokeKind::Middle,
            );

            painter.text(
                card_rect.center() + egui::vec2(0.0, -14.0),
                Align2::CENTER_CENTER,
                message,
                FontId::proportional(18.0),
                Color32::WHITE,
            );
            painter.text(
                card_rect.center() + egui::vec2(0.0, 12.0),
                Align2::CENTER_CENTER,
                detail,
                FontId::proportional(13.0),
                Color32::from_rgb(180, 180, 190),
            );

            // Show diagnostic counters below the card.
            let stats = format!(
                "demux: {}  decode: {}  upload: {}",
                status.demuxed_packets, status.decoded_frames, status.uploaded_frames,
            );
            painter.text(
                egui::pos2(card_rect.center().x, card_rect.bottom() + 16.0),
                Align2::CENTER_CENTER,
                stats,
                FontId::proportional(11.0),
                Color32::from_rgb(120, 120, 130),
            );
        });
}

fn draw_subtitle_overlay(ctx: &egui::Context, text: Option<&str>) {
    let Some(text) = text else {
        return;
    };
    if text.is_empty() {
        return;
    }

    egui::Area::new(egui::Id::new("subtitle_overlay"))
        .order(Order::Foreground)
        .anchor(Align2::CENTER_BOTTOM, egui::vec2(0.0, -72.0))
        .show(ctx, |ui| {
            Frame::new()
                .fill(Color32::from_black_alpha(160))
                .inner_margin(egui::Margin::symmetric(16, 8))
                .corner_radius(egui::CornerRadius::same(4))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(text)
                            .size(18.0)
                            .color(Color32::WHITE),
                    );
                });
        });
}

fn draw_warning_toast(ctx: &egui::Context, warning: Option<&str>) {
    let Some(msg) = warning else {
        return;
    };

    egui::Area::new(egui::Id::new("warning_toast"))
        .order(Order::Tooltip)
        .anchor(Align2::LEFT_TOP, egui::vec2(16.0, 52.0))
        .show(ctx, |ui| {
            Frame::new()
                .fill(Color32::from_rgb(60, 48, 20))
                .stroke(Stroke::new(1.0, Color32::from_rgb(200, 160, 60)))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("⚠ {msg}"))
                            .size(12.0)
                            .color(Color32::from_rgb(255, 220, 160)),
                    );
                });
        });
}

fn draw_error_toast(ctx: &egui::Context, error: Option<&str>) {
    let Some(msg) = error else {
        return;
    };

    egui::Area::new(egui::Id::new("error_toast"))
        .order(Order::Tooltip)
        .anchor(Align2::RIGHT_TOP, egui::vec2(-16.0, 52.0))
        .show(ctx, |ui| {
            Frame::new()
                .fill(Color32::from_rgb(80, 28, 28))
                .stroke(Stroke::new(1.0, Color32::from_rgb(200, 80, 80)))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("⚠ {msg}"))
                            .size(12.0)
                            .color(Color32::from_rgb(255, 200, 200)),
                    );
                });
        });
}

pub fn format_time(secs: f64) -> String {
    let total_ms = (secs * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let m = (total_s / 60) % 60;
    let h = total_s / 3600;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}.{ms:03}")
    } else {
        format!("{m:02}:{s:02}.{ms:03}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_time_displays() {
        assert_eq!(format_time(65.5), "01:05.500");
    }
}