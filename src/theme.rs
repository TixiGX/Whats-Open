//! Tema monocromatico: nero, bianco e poche sfumature di grigio.
//!
//! Il tema segue quello della distribuzione: si guarda `GTK_THEME`, le
//! preferenze GNOME (colore chiaro/scuro), `settings.ini` di GTK 3 e 4 e
//! `kdeglobals`. Nessuna dipendenza in più, nessun demone: solo la lettura di
//! pochi file e, se c'è una sessione D-Bus, una domanda a `gsettings`.
//!
//! La tavolozza resta volutamente monocromatica: cambia la luminosità, non i
//! colori. Il tema chiaro usa bolle inviate scure e ricevute bianche, quello
//! scuro il contrario.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

impl Mode {
    pub fn is_dark(self) -> bool {
        matches!(self, Mode::Dark)
    }

    pub fn label(self) -> &'static str {
        if self.is_dark() {
            "scuro"
        } else {
            "chiaro"
        }
    }
}

/// 1 = scuro (predefinito), 2 = chiaro.
static MODE: AtomicU8 = AtomicU8::new(1);
static SOURCE: Mutex<String> = Mutex::new(String::new());
/// La ricerca nel desktop si fa una volta sola: `gsettings` è un processo.
static DETECTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub surface: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    /// Bianco vero: serve sopra foto, video e scurissime sovrapposizioni.
    pub white: Color32,
    pub own_bubble: Color32,
    pub own_text: Color32,
    pub in_bubble: Color32,
    pub in_text: Color32,
    pub danger: Color32,
    pub online: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(8, 8, 8),
            panel: Color32::from_rgb(14, 14, 14),
            surface: Color32::from_rgb(22, 22, 22),
            hover: Color32::from_rgb(33, 33, 33),
            border: Color32::from_rgb(40, 40, 40),
            text: Color32::from_rgb(244, 244, 244),
            muted: Color32::from_rgb(152, 152, 152),
            faint: Color32::from_rgb(104, 104, 104),
            white: Color32::from_rgb(255, 255, 255),
            own_bubble: Color32::from_rgb(242, 242, 242),
            own_text: Color32::from_rgb(10, 10, 10),
            in_bubble: Color32::from_rgb(26, 26, 26),
            in_text: Color32::from_rgb(240, 240, 240),
            danger: Color32::from_rgb(226, 138, 138),
            online: Color32::from_rgb(220, 220, 220),
        }
    }

    pub fn light() -> Self {
        Self {
            bg: Color32::from_rgb(245, 245, 246),
            panel: Color32::from_rgb(255, 255, 255),
            surface: Color32::from_rgb(239, 239, 241),
            hover: Color32::from_rgb(231, 231, 234),
            border: Color32::from_rgb(222, 222, 225),
            text: Color32::from_rgb(22, 22, 24),
            muted: Color32::from_rgb(104, 104, 108),
            faint: Color32::from_rgb(146, 146, 151),
            white: Color32::from_rgb(255, 255, 255),
            // Bolle inviate scure su fondo chiaro: l'opposto del tema scuro,
            // così il contrasto resta alto senza introdurre colore.
            own_bubble: Color32::from_rgb(30, 30, 32),
            own_text: Color32::from_rgb(244, 244, 244),
            in_bubble: Color32::from_rgb(255, 255, 255),
            in_text: Color32::from_rgb(24, 24, 26),
            danger: Color32::from_rgb(176, 52, 52),
            online: Color32::from_rgb(70, 70, 74),
        }
    }
}

/// Tavolozza corrente. Costruirla è banale: qualche decina di byte per
/// fotogramma, nessuna allocazione.
pub fn pal() -> Palette {
    if MODE.load(Ordering::Relaxed) == 2 {
        Palette::light()
    } else {
        Palette::dark()
    }
}

pub fn mode() -> Mode {
    if MODE.load(Ordering::Relaxed) == 2 {
        Mode::Light
    } else {
        Mode::Dark
    }
}

/// Da dove viene la scelta ("GTK_THEME=…", "kdeglobals", …): finisce in
/// `--doctor` e nelle impostazioni, così si capisce perché il tema è quello.
pub fn source() -> String {
    SOURCE
        .lock()
        .map(|text| text.clone())
        .unwrap_or_else(|_| String::new())
}

pub fn set(mode: Mode, source: impl Into<String>) {
    MODE.store(if mode.is_dark() { 1 } else { 2 }, Ordering::Relaxed);
    if let Ok(mut current) = SOURCE.lock() {
        *current = source.into();
    }
}

/// Traduce la preferenza dell'utente (`auto`, `dark`, `light`) in un tema.
pub fn resolve(preference: &str) -> (Mode, String) {
    match preference.trim().to_ascii_lowercase().as_str() {
        "dark" | "scuro" => (Mode::Dark, "impostazione".to_string()),
        "light" | "chiaro" => (Mode::Light, "impostazione".to_string()),
        _ => {
            // Una sola ricerca nel desktop per avvio: la seconda chiamata
            // (impostazioni, ridisegni) riusa quello che si è già trovato.
            if DETECTED.swap(true, Ordering::Relaxed) {
                (mode(), source())
            } else {
                detect()
            }
        }
    }
}

/// Tema della distribuzione, in ordine di autorevolezza.
pub fn detect() -> (Mode, String) {
    if let Ok(value) = std::env::var("WHATS_OPEN_THEME") {
        match value.trim().to_ascii_lowercase().as_str() {
            "dark" | "scuro" | "1" | "true" => return (Mode::Dark, "WHATS_OPEN_THEME".into()),
            "light" | "chiaro" | "0" | "false" => return (Mode::Light, "WHATS_OPEN_THEME".into()),
            _ => {}
        }
    }
    if let Ok(value) = std::env::var("GTK_THEME") {
        if !value.trim().is_empty() {
            return (
                mode_from_name(&value),
                format!("GTK_THEME={value}"),
            );
        }
    }
    if let Some((mode, source)) = from_gsettings() {
        return (mode, source);
    }
    if let Some((mode, source)) = from_gtk_settings() {
        return (mode, source);
    }
    if let Some((mode, source)) = from_kde() {
        return (mode, source);
    }
    (Mode::Dark, "predefinito".into())
}

/// Mescola due colori (0 = `a`, 1 = `b`). Serve per le sfumature interne ai
/// fumetti, che devono funzionare sia su fondo chiaro sia su fondo scuro.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let blend = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        blend(a.r(), b.r()),
        blend(a.g(), b.g()),
        blend(a.b(), b.b()),
        blend(a.a(), b.a()),
    )
}

fn mode_from_name(name: &str) -> Mode {
    let lower = name.to_ascii_lowercase();
    if lower.contains("dark") || lower.contains("scuro") || lower.contains("black") {
        Mode::Dark
    } else {
        Mode::Light
    }
}

fn from_gsettings() -> Option<(Mode, String)> {
    // Senza sessione D-Bus `gsettings` non ha dove leggere: si evita di
    // avviare un processo inutile.
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
        return None;
    }
    let read = |key: &str| -> Option<String> {
        let output = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", key])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    if let Some(scheme) = read("color-scheme") {
        if scheme.contains("prefer-dark") {
            return Some((Mode::Dark, "GNOME: preferisce scuro".into()));
        }
        if scheme.contains("prefer-light") || scheme.contains("default") {
            return Some((Mode::Light, "GNOME: preferisce chiaro".into()));
        }
    }
    if let Some(theme) = read("gtk-theme") {
        let theme = theme.trim_matches('\'').to_string();
        if !theme.is_empty() {
            return Some((mode_from_name(&theme), format!("tema GTK {theme}")));
        }
    }
    None
}

fn from_gtk_settings() -> Option<(Mode, String)> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    for variant in ["gtk-4.0", "gtk-3.0"] {
        let path = home.join(".config").join(variant).join("settings.ini");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(dark) = ini_value(&text, "gtk-application-prefer-dark-theme") {
            if matches!(dark.trim(), "1" | "true" | "True") {
                return Some((Mode::Dark, format!("{variant}: preferisce scuro")));
            }
        }
        if let Some(name) = ini_value(&text, "gtk-theme-name") {
            if !name.is_empty() {
                return Some((mode_from_name(&name), format!("tema GTK {name}")));
            }
        }
    }
    None
}

fn from_kde() -> Option<(Mode, String)> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from)?;
    let path = home.join(".config").join("kdeglobals");
    let text = std::fs::read_to_string(&path).ok()?;
    if let Some(scheme) = ini_value(&text, "ColorScheme") {
        if !scheme.is_empty() {
            return Some((mode_from_name(&scheme), format!("KDE: {scheme}")));
        }
    }
    // `BackgroundNormal=239,240,241`: la luminanza dice più del nome.
    if let Some(rgb) = ini_value(&text, "BackgroundNormal") {
        let parts: Vec<f32> = rgb
            .split(',')
            .filter_map(|part| part.trim().parse::<f32>().ok())
            .collect();
        if parts.len() >= 3 {
            let luminance = 0.2126 * parts[0] + 0.7152 * parts[1] + 0.0722 * parts[2];
            let mode = if luminance < 128.0 {
                Mode::Dark
            } else {
                Mode::Light
            };
            return Some((mode, "KDE: colore di sfondo".into()));
        }
    }
    None
}

fn ini_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') || line.starts_with('[') {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case(key) {
            return Some(value.trim().to_string());
        }
    }
    None
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

/// Sostituisce i font senza toccare il resto del tema.
pub fn set_system_fonts(ctx: &egui::Context, enabled: bool) {
    if enabled {
        install_system_fonts(ctx);
    } else {
        // Torna ai soli font incorporati nel binario.
        ctx.set_fonts(egui::FontDefinitions::default());
    }
}

pub fn apply(ctx: &egui::Context, system_fonts: bool) {
    set_system_fonts(ctx, system_fonts);

    let palette = pal();
    let mut style = (*ctx.global_style()).clone();

    style.text_styles = std::collections::BTreeMap::from([
        (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]);

    style.spacing.item_spacing = egui::vec2(9.0, 9.0);
    style.spacing.button_padding = egui::vec2(11.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.menu_margin = egui::Margin::same(6);
    style.animation_time = 0.12;

    let mut v = if mode().is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.dark_mode = mode().is_dark();
    v.panel_fill = palette.bg;
    v.window_fill = palette.panel;
    v.extreme_bg_color = palette.surface;
    v.faint_bg_color = palette.surface;
    v.code_bg_color = palette.surface;
    v.hyperlink_color = palette.text;
    v.override_text_color = None;
    v.window_stroke = Stroke::new(1.0, palette.border);
    v.window_corner_radius = egui::CornerRadius::same(16);
    v.menu_corner_radius = egui::CornerRadius::same(12);
    v.selection.bg_fill = palette.hover;
    v.selection.stroke = Stroke::new(1.0, palette.text);

    v.widgets.noninteractive.bg_fill = palette.panel;
    v.widgets.noninteractive.weak_bg_fill = palette.panel;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.muted);
    v.widgets.noninteractive.corner_radius = egui::CornerRadius::same(10);

    v.widgets.inactive.bg_fill = palette.surface;
    v.widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);
    v.widgets.inactive.corner_radius = egui::CornerRadius::same(10);

    v.widgets.hovered.bg_fill = palette.hover;
    v.widgets.hovered.weak_bg_fill = palette.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, palette.border);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(10);

    v.widgets.active.bg_fill = palette.hover;
    v.widgets.active.weak_bg_fill = palette.hover;
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.active.fg_stroke = Stroke::new(1.0, palette.text);
    v.widgets.active.corner_radius = egui::CornerRadius::same(10);

    v.widgets.open.bg_fill = palette.surface;
    v.widgets.open.weak_bg_fill = palette.surface;
    v.widgets.open.bg_stroke = Stroke::NONE;
    v.widgets.open.fg_stroke = Stroke::new(1.0, palette.text);
    v.widgets.open.corner_radius = egui::CornerRadius::same(10);

    style.visuals = v;
    ctx.set_global_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_tema_chiaro_non_e_un_negativo_del_scuro() {
        let dark = Palette::dark();
        let light = Palette::light();
        assert!(dark.in_text != light.in_text);
        assert!(light.own_bubble.r() < light.own_text.r());
        assert!(dark.own_bubble.r() > dark.own_text.r());
        // Il bianco pieno serve solo sopra le foto.
        assert_eq!(dark.white, light.white);
    }

    #[test]
    fn le_preferenze_esplicite_vincono() {
        assert_eq!(resolve("dark").0, Mode::Dark);
        assert_eq!(resolve("chiaro").0, Mode::Light);
        assert_eq!(resolve("light").0, Mode::Light);
    }

    #[test]
    fn i_nomi_dei_temi_si_capiscono() {
        assert_eq!(mode_from_name("Adwaita:dark"), Mode::Dark);
        assert_eq!(mode_from_name("BreezeDark"), Mode::Dark);
        assert_eq!(mode_from_name("Adwaita"), Mode::Light);
        assert_eq!(ini_value("[General]\nColorScheme=BreezeDark\n", "ColorScheme").as_deref(), Some("BreezeDark"));
    }
}
