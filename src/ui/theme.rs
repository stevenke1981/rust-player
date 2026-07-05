use egui::{Color32, CornerRadius, Stroke};

pub const VLC_ORANGE: Color32 = Color32::from_rgb(255, 136, 0);
pub const BG_DARK: Color32 = Color32::from_rgb(12, 12, 14);
pub const PANEL_BG: Color32 = Color32::from_rgb(28, 28, 32);
pub const BAR_BG: Color32 = Color32::from_rgb(20, 20, 24);
pub const TRACK_BG: Color32 = Color32::from_rgb(58, 58, 66);
pub const TEXT_DIM: Color32 = Color32::from_rgb(160, 160, 170);
pub const TEXT_MAIN: Color32 = Color32::from_rgb(230, 230, 235);

pub fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let visuals = &mut style.visuals;

    visuals.dark_mode = true;
    visuals.panel_fill = PANEL_BG;
    visuals.window_fill = BG_DARK;
    visuals.extreme_bg_color = BG_DARK;
    visuals.faint_bg_color = BAR_BG;
    visuals.override_text_color = Some(TEXT_MAIN);
    visuals.widgets.noninteractive.bg_fill = TRACK_BG;
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(44, 44, 50);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(62, 62, 70);
    visuals.widgets.active.bg_fill = VLC_ORANGE;
    visuals.widgets.open.bg_fill = Color32::from_rgb(52, 52, 58);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_MAIN);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::from_rgb(18, 18, 22));
    visuals.selection.bg_fill = VLC_ORANGE.gamma_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, VLC_ORANGE);
    visuals.hyperlink_color = VLC_ORANGE;

    style.spacing.item_spacing = egui::vec2(10.0, 6.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    style.spacing.interact_size = egui::vec2(36.0, 28.0);
    style.visuals.window_corner_radius = CornerRadius::same(4);

    ctx.set_style(style);
}

pub fn transport_button(ui: &mut egui::Ui, label: &str, accent: bool) -> egui::Response {
    let fill = if accent {
        VLC_ORANGE
    } else {
        Color32::from_rgb(44, 44, 50)
    };
    let text_color = if accent {
        Color32::from_rgb(18, 18, 22)
    } else {
        TEXT_MAIN
    };
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(text_color).size(15.0))
            .fill(fill)
            .corner_radius(CornerRadius::same(6))
            .min_size(egui::vec2(40.0, 32.0)),
    )
}

pub fn paint_seek_bar(
    ui: &mut egui::Ui,
    fraction: f32,
    dragging: bool,
) -> (egui::Response, f32) {
    let height = 20.0;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click_and_drag());

    let track_h = 5.0;
    let track = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), track_h));
    let painter = ui.painter();

    painter.rect_filled(track, 3.0, TRACK_BG);

    let progress_w = track.width() * fraction.clamp(0.0, 1.0);
    if progress_w > 0.0 {
        let fill = egui::Rect::from_min_size(track.min, egui::vec2(progress_w, track_h));
        painter.rect_filled(fill, 3.0, VLC_ORANGE);
    }

    let thumb_r = if dragging { 8.0 } else { 6.0 };
    let thumb_x = track.min.x + progress_w;
    let thumb_center = egui::pos2(thumb_x, track.center().y);
    painter.circle_filled(thumb_center, thumb_r + 2.0, Color32::from_black_alpha(80));
    painter.circle_filled(thumb_center, thumb_r, if dragging { Color32::WHITE } else { VLC_ORANGE });

    let mut frac = fraction;
    if let Some(pos) = response.interact_pointer_pos() {
        if response.clicked() || response.dragged() {
            frac = ((pos.x - track.min.x) / track.width()).clamp(0.0, 1.0);
        }
    }

    (response, frac)
}