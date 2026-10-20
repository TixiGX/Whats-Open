//! Tema monocromatico: nero, bianco e poche sfumature di grigio.

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};

pub struct Palette;

impl Palette {
    pub const BG: Color32 = Color32::from_rgb(8, 8, 8);
    pub const PANEL: Color32 = Color32::from_rgb(14, 14, 14);
    pub const SURFACE: Color32 = Color32::from_rgb(22, 22, 22);
    pub const HOVER: Color32 = Color32::from_rgb(33, 33, 33);
    pub const BORDER: Color32 = Color32::from_rgb(40, 40, 40);
    pub const TEXT: Color32 = Color32::from_rgb(244, 244, 244);
    pub const MUTED: Color32 = Color32::from_rgb(152, 152, 152);
    pub const FAINT: Color32 = Color32::from_rgb(104, 104, 104);
    pub const WHITE: Color32 = Color32::from_rgb(255, 255, 255);
    pub const OWN_BUBBLE: Color32 = Color32::from_rgb(242, 242, 242);
    pub const OWN_TEXT: Color32 = Color32::from_rgb(10, 10, 10);
    pub const IN_BUBBLE: Color32 = Color32::from_rgb(26, 26, 26);
    pub const IN_TEXT: Color32 = Color32::from_rgb(240, 240, 240);
    pub const DANGER: Color32 = Color32::from_rgb(226, 138, 138);
    pub const ONLINE: Color32 = Color32::from_rgb(220, 220, 220);
}

/// Aggiunge i font di sistema come riserva: emoji, arabo, ebraico, devanagari,
/// thai e CJK restano leggibili anche senza font dedicati nel binario.
pub fn install_system_fonts(ctx: &egui::Context) {
    if std::env::var_os("WHATS_OPEN_NO_SYSTEM_FONTS").is_some() {
        return;
    }
    let fonts = crate::platform::load_system_fonts();
    if fonts.is_empty() {
        return;
    }

    let mut definitions = egui::FontDefinitions::default();
    for (name, bytes) in fonts {
        definitions
            .font_data
            .insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            definitions
                .families
                .entry(family)
                .or_default()
                .push(name.clone());
        }
    }
    ctx.set_fonts(definitions);
}

pub fn apply(ctx: &egui::Context) {
    install_system_fonts(ctx);

    let mut style = (*ctx.style()).clone();

    style.text_styles = std::collections::BTreeMap::from([
        (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]);

    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.interact_size.y = 30.0;

    let mut v = egui::Visuals::dark();
    v.dark_mode = true;
    v.panel_fill = Palette::BG;
    v.window_fill = Palette::PANEL;
    v.extreme_bg_color = Palette::SURFACE;
    v.faint_bg_color = Palette::SURFACE;
    v.code_bg_color = Palette::SURFACE;
    v.hyperlink_color = Palette::WHITE;
    v.override_text_color = None;
    v.window_stroke = Stroke::new(1.0, Palette::BORDER);
    v.window_corner_radius = egui::CornerRadius::same(14);
    v.menu_corner_radius = egui::CornerRadius::same(10);
    v.selection.bg_fill = Palette::HOVER;
    v.selection.stroke = Stroke::new(1.0, Palette::WHITE);

    v.widgets.noninteractive.bg_fill = Palette::PANEL;
    v.widgets.noninteractive.weak_bg_fill = Palette::PANEL;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Palette::BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Palette::MUTED);
    v.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);

    v.widgets.inactive.bg_fill = Palette::SURFACE;
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, Palette::TEXT);
    v.widgets.inactive.corner_radius = egui::CornerRadius::same(8);

    v.widgets.hovered.bg_fill = Palette::HOVER;
    v.widgets.hovered.weak_bg_fill = Palette::HOVER;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, Palette::WHITE);
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(8);

    v.widgets.active.bg_fill = Palette::HOVER;
    v.widgets.active.weak_bg_fill = Palette::HOVER;
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.active.fg_stroke = Stroke::new(1.0, Palette::WHITE);
    v.widgets.active.corner_radius = egui::CornerRadius::same(8);

    v.widgets.open.bg_fill = Palette::SURFACE;
    v.widgets.open.weak_bg_fill = Palette::SURFACE;
    v.widgets.open.bg_stroke = Stroke::NONE;
    v.widgets.open.fg_stroke = Stroke::new(1.0, Palette::TEXT);
    v.widgets.open.corner_radius = egui::CornerRadius::same(8);

    style.visuals = v;
    ctx.set_style(style);
}
