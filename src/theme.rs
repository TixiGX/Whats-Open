//! Tema monocromatico che parte dai colori veri del desktop.
//!
//! Il client non disegna con GTK, ma può leggere il tema nativo: si prende il
//! nome del tema (`GTK_THEME`, `gsettings`, `settings.ini`, `kdeglobals`), si
//! apre il suo `gtk.css` (o `gtk-dark.css`) e si leggono le `@define-color`
//! che ogni tema GTK deve dichiarare — sfondo, caselle, testo, selezione.
//! Da lì nascono le tinte della tavolozza: si tiene la luminosità esatta del
//! tema e la si porta in scala di grigi, perché l'interfaccia resta
//! monocromatica come da progetto. Un tema "MacOS Tahoe" darà quindi i suoi
//! grigi caldi, Adwaita i suoi, Breeze i suoi.
//!
//! Se il tema non si trova o non è leggibile si torna alle due tavolozze
//! incorporate: l'app non resta mai senza colori. Nessuna dipendenza in più,
//! nessun demone: solo file di testo e, su GNOME, una domanda a `gsettings`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{LazyLock, Mutex, RwLock};
use std::time::Duration;

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

    /// Tavolozza costruita sui colori del tema nativo: le tinte vengono
    /// ridotte a grigi (il progetto resta monocromatico) mantenendo il
    /// chiaroscuro del tema, poi si garantisce il contrasto del testo.
    pub fn from_native(native: &NativeTheme) -> Self {
        let preset = if native.dark {
            Palette::dark()
        } else {
            Palette::light()
        };
        let bg = gray_color(native.bg);
        let base = gray_color(native.base);
        let panel = if (base.r() as i16 - bg.r() as i16).abs() < 4 {
            shift(bg, if native.dark { 6 } else { -4 })
        } else {
            base
        };
        let surface = mix(panel, bg, if native.dark { 0.35 } else { 0.25 });
        let selected_gray = gray_color(native.selected);
        let mut text = gray_color(native.fg);
        if contrast(text, bg) < 7.0 {
            // Tema con testo poco contrastato: meglio leggibile che fedele.
            text = preset.text;
        }
        // Il testo secondario si allontana dallo sfondo finché non resta
        // leggibile anche su temi con poco contrasto.
        let mut muted = mix(text, bg, 0.38);
        for step in [0.30, 0.22, 0.14] {
            if contrast(muted, bg) >= 3.0 {
                break;
            }
            muted = mix(text, bg, step);
        }
        let mut faint = mix(text, bg, 0.57);
        for step in [0.48, 0.42, 0.36] {
            if contrast(faint, bg) >= 2.2 {
                break;
            }
            faint = mix(text, bg, step);
        }
        let hover = mix(surface, text, if native.dark { 0.10 } else { 0.07 });
        let border = mix(bg, text, if native.dark { 0.22 } else { 0.14 });
        // La bolla inviata resta il segno forte della chat: la sua luminosità
        // arriva dall'accento del tema, ma viene spinta dove il contrasto è
        // sicuro (scura su tema chiaro, chiara su tema scuro).
        let (own_bubble, own_text) = if native.dark {
            (Color32::from_gray(selected_gray.r().clamp(200, 244)), Color32::from_gray(10))
        } else {
            (Color32::from_gray(selected_gray.r().clamp(18, 70)), Color32::from_gray(246))
        };
        let in_bubble = if native.dark {
            shift(surface, 8)
        } else {
            panel
        };
        let in_text = if contrast(text, in_bubble) >= 7.0 {
            text
        } else {
            preset.in_text
        };
        Self {
            bg,
            panel,
            surface,
            hover,
            border,
            text,
            muted,
            faint,
            white: Color32::WHITE,
            own_bubble,
            own_text,
            in_bubble,
            in_text,
            danger: preset.danger,
            online: preset.online,
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "sfondo {} · pannello {} · testo {} · bolle {}",
            hex(self.bg),
            hex(self.panel),
            hex(self.text),
            hex(self.own_bubble)
        )
    }
}

pub fn hex(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

/// Tema GTK trovato su disco, già letto nei suoi colori principali.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeTheme {
    pub name: String,
    /// Cartella da cui arrivano i colori (`gtk-3.0`, `gtk-4.0`, `kdeglobals`).
    pub variant: String,
    pub dark: bool,
    /// Quante `@define-color` sono state lette: 0 = nessun colore trovato.
    pub colors: usize,
    pub bg: Color32,
    pub base: Color32,
    pub fg: Color32,
    pub selected: Color32,
}

impl NativeTheme {
    pub fn summary(&self) -> String {
        format!("{} ({}, {})", self.name, self.variant, if self.dark { "scuro" } else { "chiaro" })
    }
}

/// Esito della scelta del tema: modalità, provenienza e, se c'è, il tema
/// nativo da cui sono stati presi i colori.
#[derive(Clone, Debug)]
pub struct Detected {
    pub mode: Mode,
    pub source: String,
    pub native: Option<NativeTheme>,
}

impl Detected {
    fn preset(mode: Mode, source: impl Into<String>) -> Self {
        Self {
            mode,
            source: source.into(),
            native: None,
        }
    }
}

/// 1 = scuro (predefinito), 2 = chiaro.
static MODE: AtomicU8 = AtomicU8::new(1);
static SOURCE: Mutex<String> = Mutex::new(String::new());
static PALETTE: LazyLock<RwLock<Palette>> = LazyLock::new(|| RwLock::new(Palette::dark()));
/// Il tema "automatico" si cerca una volta sola; poi si riusa.
static AUTO: LazyLock<Mutex<Option<Detected>>> = LazyLock::new(|| Mutex::new(None));
/// Tema nativo in uso, per la diagnostica.
static CURRENT_NATIVE: LazyLock<Mutex<Option<NativeTheme>>> =
    LazyLock::new(|| Mutex::new(None));
/// Osservatore del tema: impronta dei file e ultimo controllo di `gsettings`.
struct Watch {
    ready: bool,
    checked: std::time::Instant,
    stamp: u64,
    gsettings_at: std::time::Instant,
    gsettings: Option<u64>,
}
static WATCH: LazyLock<Mutex<Watch>> = LazyLock::new(|| {
    Mutex::new(Watch {
        ready: false,
        checked: std::time::Instant::now(),
        stamp: 0,
        gsettings_at: std::time::Instant::now(),
        gsettings: None,
    })
});

/// Tavolozza corrente.
pub fn pal() -> Palette {
    match PALETTE.read() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

pub fn mode() -> Mode {
    if MODE.load(Ordering::Relaxed) == 2 {
        Mode::Light
    } else {
        Mode::Dark
    }
}

/// Da dove viene la scelta ("GTK_THEME=…", "tema GTK MacOS-Tahoe (gtk-3.0)", …).
pub fn source() -> String {
    SOURCE
        .lock()
        .map(|text| text.clone())
        .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
}

/// Il tema nativo attualmente in uso, se i colori arrivano dal desktop.
pub fn native() -> Option<NativeTheme> {
    CURRENT_NATIVE
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

/// Applica una scelta: modalità, provenienza e tavolozza.
pub fn set(detected: &Detected) {
    MODE.store(if detected.mode.is_dark() { 1 } else { 2 }, Ordering::Relaxed);
    if let Ok(mut current) = SOURCE.lock() {
        *current = detected.source.clone();
    }
    let palette = match detected.native.as_ref() {
        Some(native) => Palette::from_native(native),
        None if detected.mode.is_dark() => Palette::dark(),
        None => Palette::light(),
    };
    if let Ok(mut current) = PALETTE.write() {
        *current = palette;
    }
    if let Ok(mut current) = CURRENT_NATIVE.lock() {
        *current = detected.native.clone();
    }
}

/// Traduce la preferenza dell'utente (`auto`, `dark`, `light`) in un tema.
pub fn resolve(preference: &str) -> Detected {
    match preference.trim().to_ascii_lowercase().as_str() {
        "dark" | "scuro" => Detected {
            mode: Mode::Dark,
            source: "impostazione".to_string(),
            native: native_for(Mode::Dark),
        },
        "light" | "chiaro" => Detected {
            mode: Mode::Light,
            source: "impostazione".to_string(),
            native: native_for(Mode::Light),
        },
        _ => {
            if let Some(cached) = AUTO.lock().ok().and_then(|guard| guard.clone()) {
                return cached;
            }
            let detected = detect_detailed();
            if let Ok(mut guard) = AUTO.lock() {
                *guard = Some(detected.clone());
            }
            detected
        }
    }
}

/// Controlla se il tema del desktop è cambiato e, in quel caso, lo applica.
///
/// Costa qualche `stat` al secondo (nessun processo, nessuna lettura dei CSS):
/// il tema viene riletto per intero solo quando cambia davvero qualcosa.
/// `gsettings` viene interrogato al massimo ogni 30 s, perché su GNOME il
/// cambio di tema non tocca alcun file di configurazione.
/// Restituisce `true` se la tavolozza è cambiata: chi chiama deve ridisegnare.
pub fn watch(preference: &str) -> bool {
    let now = std::time::Instant::now();
    let Ok(mut watch) = WATCH.lock() else {
        return false;
    };
    let stamp = fingerprint();
    if !watch.ready {
        watch.ready = true;
        watch.checked = now;
        watch.stamp = stamp;
        watch.gsettings_at = now;
        return false;
    }
    // Un controllo al secondo basta: il tema non cambia certo più in fretta.
    if now.duration_since(watch.checked) < Duration::from_secs(1) {
        return false;
    }
    watch.checked = now;
    let mut changed = stamp != watch.stamp;
    watch.stamp = stamp;
    if now.duration_since(watch.gsettings_at) >= Duration::from_secs(30) {
        watch.gsettings_at = now;
        let current = gsettings_theme().map(|(name, hint)| {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            name.hash(&mut hasher);
            format!("{hint:?}").hash(&mut hasher);
            hasher.finish()
        });
        if let (Some(previous), Some(current)) = (watch.gsettings, current) {
            if previous != current {
                changed = true;
            }
        }
        watch.gsettings = current;
    }
    if !changed {
        return false;
    }
    drop(watch);
    refresh(preference)
}

/// Rilegge il tema e lo applica se è diverso da quello in uso.
fn refresh(preference: &str) -> bool {
    if let Ok(mut cache) = AUTO.lock() {
        *cache = None;
    }
    let detected = resolve(preference);
    if detected.mode == mode() && detected.source == source() && detected.native == native() {
        return false;
    }
    set(&detected);
    true
}

/// Impronta leggera di tutto ciò che può cambiare i colori: variabili
/// d'ambiente, file di configurazione GTK/KDE e CSS del tema in uso.
fn fingerprint() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for key in [
        "WHATS_OPEN_THEME",
        "GTK_THEME",
        "XDG_CURRENT_DESKTOP",
        "XDG_DATA_HOME",
        "HOME",
    ] {
        std::env::var_os(key).hash(&mut hasher);
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for folder in ["gtk-3.0", "gtk-4.0"] {
            for file in ["settings.ini", "gtk.css", "gtk-dark.css"] {
                paths.push(home.join(".config").join(folder).join(file));
            }
        }
        paths.push(home.join(".config/kdeglobals"));
    }
    // I file del tema in uso: se il pacchetto viene aggiornato i colori cambiano.
    if let Some(native) = native() {
        if let Some(root) = theme_root(&native.name) {
            for folder in ["gtk-3.0", "gtk-4.0"] {
                for file in ["gtk.css", "gtk-dark.css"] {
                    paths.push(root.join(folder).join(file));
                }
            }
        }
    }
    for path in paths {
        path.hash(&mut hasher);
        match std::fs::metadata(&path) {
            Ok(meta) => (meta.len(), meta.modified().ok()).hash(&mut hasher),
            Err(_) => 0u8.hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// Tema della distribuzione, in ordine di autorevolezza.
pub fn detect() -> (Mode, String) {
    let detected = detect_detailed();
    (detected.mode, detected.source)
}

/// Come [`detect`], ma tiene anche i colori del tema nativo.
pub fn detect_detailed() -> Detected {
    if let Ok(value) = std::env::var("WHATS_OPEN_THEME") {
        match value.trim().to_ascii_lowercase().as_str() {
            "dark" | "scuro" | "1" | "true" => {
                return Detected::preset(Mode::Dark, "WHATS_OPEN_THEME");
            }
            "light" | "chiaro" | "0" | "false" => {
                return Detected::preset(Mode::Light, "WHATS_OPEN_THEME");
            }
            _ => {}
        }
    }

    let desktop = desktop();
    if let Some(name) = desktop.name.as_deref() {
        if let Some(native) = load_native_theme(name, desktop.hint) {
            let mode = if native.dark { Mode::Dark } else { Mode::Light };
            return Detected {
                mode,
                source: format!("tema GTK {} ({})", native.summary(), desktop.source),
                native: Some(native),
            };
        }
    }
    if let Some(native) = desktop.kde.as_ref().and_then(kde_native) {
        return Detected {
            mode: if native.dark { Mode::Dark } else { Mode::Light },
            source: format!("KDE: {}", native.summary()),
            native: Some(native),
        };
    }
    let mode = desktop
        .hint
        .or_else(|| desktop.name.as_deref().map(mode_from_name))
        .unwrap_or(Mode::Dark);
    Detected::preset(
        mode,
        if desktop.source.is_empty() {
            "predefinito".to_string()
        } else {
            desktop.source
        },
    )
}

/// Cerca il tema del desktop con la modalità richiesta. Serve alle
/// preferenze esplicite: se il tema non ha quella variante si usano le
/// tavolozze incorporate, così `--theme light` non diventa un tema scuro.
fn native_for(mode: Mode) -> Option<NativeTheme> {
    let desktop = desktop();
    let name = desktop.name?;
    let native = load_native_theme(&name, Some(mode))?;
    if native.dark == mode.is_dark() {
        Some(native)
    } else {
        None
    }
}

#[derive(Default)]
struct Desktop {
    name: Option<String>,
    source: String,
    hint: Option<Mode>,
    /// Colori letti da `kdeglobals`, se non c'è un tema GTK.
    kde: Option<KdeColors>,
}

#[derive(Clone, Debug, Default)]
struct KdeColors {
    bg: Color32,
    base: Color32,
    fg: Color32,
    selected: Color32,
}

fn desktop() -> Desktop {
    // 1. La variabile d'ambiente è la più esplicita.
    if let Ok(value) = std::env::var("GTK_THEME") {
        let value = value.trim();
        if !value.is_empty() {
            let (name, variant) = match value.split_once(':') {
                Some((name, variant)) => (name.trim(), Some(variant.trim())),
                None => (value, None),
            };
            let hint = variant.map(|variant| {
                if variant.eq_ignore_ascii_case("dark") {
                    Mode::Dark
                } else if variant.eq_ignore_ascii_case("light") {
                    Mode::Light
                } else {
                    mode_from_name(variant)
                }
            });
            return Desktop {
                name: Some(name.to_string()),
                source: format!("GTK_THEME={value}"),
                hint,
                kde: None,
            };
        }
    }

    let xdg = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let gnome_like = xdg.is_empty()
        || ["gnome", "unity", "cinnamon", "budgie", "pantheon", "elementary"]
            .iter()
            .any(|desktop| xdg.contains(desktop));

    // 2. Su GNOME comanda gsettings; altrove si legge prima il file (nessun
    //    processo avviato). In entrambi i casi si prova anche l'altra fonte.
    let mut hint_from_scheme = None;
    if gnome_like {
        if let Some((name, hint)) = gsettings_theme() {
            hint_from_scheme = hint;
            if let Some(name) = name {
                return Desktop {
                    name: Some(name.clone()),
                    source: format!("GNOME: {name}"),
                    hint: hint_from_scheme,
                    kde: None,
                };
            }
        }
    }
    if let Some((name, hint)) = gtk_settings_ini() {
        return Desktop {
            name: Some(name.clone()),
            source: format!("gtk settings.ini: {name}"),
            hint: hint.or(hint_from_scheme),
            kde: None,
        };
    }
    if !gnome_like {
        if let Some((name, hint)) = gsettings_theme() {
            hint_from_scheme = hint_from_scheme.or(hint);
            if let Some(name) = name {
                return Desktop {
                    name: Some(name.clone()),
                    source: format!("GNOME: {name}"),
                    hint: hint_from_scheme,
                    kde: None,
                };
            }
        }
    }
    if let Some(kde) = kde_colors() {
        return Desktop {
            name: None,
            source: "kdeglobals".to_string(),
            hint: None,
            kde: Some(kde),
        };
    }
    Desktop {
        name: None,
        source: String::new(),
        hint: hint_from_scheme,
        kde: None,
    }
}

fn gtk_settings_ini() -> Option<(String, Option<Mode>)> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    for variant in ["gtk-4.0", "gtk-3.0"] {
        let path = home.join(".config").join(variant).join("settings.ini");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = ini_value(&text, "gtk-theme-name");
        let dark = ini_value(&text, "gtk-application-prefer-dark-theme").and_then(|value| {
            match value.trim() {
                "1" | "true" | "True" => Some(Mode::Dark),
                "0" | "false" | "False" => None,
                _ => None,
            }
        });
        if let Some(name) = name.filter(|name| !name.trim().is_empty()) {
            return Some((name.trim().to_string(), dark));
        }
    }
    None
}

fn gsettings_theme() -> Option<(Option<String>, Option<Mode>)> {
    // Senza sessione D-Bus `gsettings` non ha dove leggere.
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
    let scheme = read("color-scheme");
    let mode = scheme.as_deref().map(|scheme| {
        if scheme.contains("prefer-dark") {
            Mode::Dark
        } else {
            Mode::Light
        }
    });
    let name = read("gtk-theme")
        .map(|name| name.trim_matches('\'').trim_matches('"').trim().to_string())
        .filter(|name| !name.is_empty());
    if name.is_none() && mode.is_none() {
        return None;
    }
    Some((name, mode))
}

fn kde_colors() -> Option<KdeColors> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let text = std::fs::read_to_string(home.join(".config/kdeglobals")).ok()?;
    let bg = parse_decimal_color(&ini_section_value(
        &text,
        "Colors:Window",
        "BackgroundNormal",
    )?)?;
    let fg = ini_section_value(&text, "Colors:Window", "ForegroundNormal")
        .as_deref()
        .and_then(parse_decimal_color)
        .unwrap_or(Color32::from_gray(230));
    let base = ini_section_value(&text, "Colors:View", "BackgroundNormal")
        .as_deref()
        .and_then(parse_decimal_color)
        .unwrap_or(bg);
    let selected = ini_section_value(&text, "Colors:Selection", "BackgroundNormal")
        .as_deref()
        .and_then(parse_decimal_color)
        .unwrap_or(fg);
    Some(KdeColors {
        bg,
        base,
        fg,
        selected,
    })
}

fn kde_native(colors: &KdeColors) -> Option<NativeTheme> {
    Some(NativeTheme {
        name: "KDE".to_string(),
        variant: "kdeglobals".to_string(),
        dark: luminance(colors.bg) < 0.5,
        colors: 4,
        bg: colors.bg,
        base: colors.base,
        fg: colors.fg,
        selected: colors.selected,
    })
}

fn parse_decimal_color(text: &str) -> Option<Color32> {
    let parts: Vec<u8> = text
        .split(',')
        .filter_map(|part| part.trim().parse::<u8>().ok())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    Some(Color32::from_rgb(parts[0], parts[1], parts[2]))
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

fn shift(color: Color32, delta: i16) -> Color32 {
    let apply = |value: u8| (value as i16 + delta).clamp(0, 255) as u8;
    Color32::from_rgb(apply(color.r()), apply(color.g()), apply(color.b()))
}

fn gray_color(color: Color32) -> Color32 {
    Color32::from_gray(gray(color))
}

/// Luminosità percepita di un colore (0-255): è il ponte fra il tema del
/// desktop e la tavolozza monocromatica.
pub fn gray(color: Color32) -> u8 {
    let value = 0.2126 * color.r() as f32 + 0.7152 * color.g() as f32 + 0.0722 * color.b() as f32;
    value.round().clamp(0.0, 255.0) as u8
}

/// Luminanza relativa 0..1 (come WCAG, con la correzione gamma).
fn luminance(color: Color32) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = value as f32 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

/// Rapporto di contrasto WCAG fra due colori (1:1 … 21:1).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (first, second) = (luminance(a), luminance(b));
    let (high, low) = if first > second {
        (first, second)
    } else {
        (second, first)
    };
    (high + 0.05) / (low + 0.05)
}

fn mode_from_name(name: &str) -> Mode {
    let lower = name.to_ascii_lowercase();
    if lower.contains("dark") || lower.contains("scuro") || lower.contains("black") {
        Mode::Dark
    } else {
        Mode::Light
    }
}

// ── lettura del tema GTK ─────────────────────────────────────────────────

/// Apre il tema `name` e ne legge i colori. `want` è la variante richiesta
/// (chiara/scura); se il tema non ce l'ha si restituisce `None` e chi chiama
/// usa la tavolozza incorporata.
pub fn load_native_theme(name: &str, want: Option<Mode>) -> Option<NativeTheme> {
    let root = theme_root(name)?;
    let mut files: Vec<(&'static str, PathBuf)> = Vec::new();
    let variants: [(&'static str, &'static str); 4] = match want {
        Some(Mode::Dark) => [
            ("gtk-3.0", "gtk-dark.css"),
            ("gtk-4.0", "gtk-dark.css"),
            ("gtk-3.0", "gtk.css"),
            ("gtk-4.0", "gtk.css"),
        ],
        _ => [
            ("gtk-3.0", "gtk.css"),
            ("gtk-4.0", "gtk.css"),
            ("gtk-3.0", "gtk-dark.css"),
            ("gtk-4.0", "gtk-dark.css"),
        ],
    };
    for (variant, file) in variants {
        let path = root.join(variant).join(file);
        if path.is_file() && !files.iter().any(|(_, existing)| existing == &path) {
            files.push((variant, path));
        }
    }

    let mut fallback = None;
    for (variant, path) in files {
        let Some(defs) = read_theme_css(&path) else {
            continue;
        };
        let Some(colors) = pick_colors(&defs) else {
            continue;
        };
        let dark = luminance(colors.bg) < 0.5;
        let native = NativeTheme {
            name: name.to_string(),
            variant: variant.to_string(),
            dark,
            colors: defs.len(),
            bg: colors.bg,
            base: colors.base,
            fg: colors.fg,
            selected: colors.selected,
        };
        match want {
            Some(mode) if native.dark != mode.is_dark() => {
                if fallback.is_none() {
                    fallback = Some(native);
                }
            }
            _ => return Some(native),
        }
    }
    // Nessuna variante corrispondente: se chi chiama non ha chiesto una
    // modalità precisa, va bene quella che c'è.
    if want.is_none() {
        fallback
    } else {
        None
    }
}


fn theme_root(name: &str) -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data).join("themes"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(".themes"));
        roots.push(home.join(".local/share/themes"));
    }
    roots.push(PathBuf::from("/usr/share/themes"));
    roots.push(PathBuf::from("/usr/local/share/themes"));

    for root in &roots {
        let candidate = root.join(name);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    // Nomi con maiuscole diverse fra loro (MacOS-Tahoe / macos-tahoe).
    for root in &roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            if !is_dir {
                continue;
            }
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
            {
                return Some(entry.path());
            }
        }
    }
    None
}

const MAX_CSS_BYTES: u64 = 3_000_000;

fn read_theme_css(path: &Path) -> Option<HashMap<String, String>> {
    let mut visited = std::collections::HashSet::new();
    let mut defs = HashMap::new();
    read_css_into(path, &mut defs, &mut visited, 0)?;
    if defs.is_empty() {
        None
    } else {
        Some(defs)
    }
}

fn read_css_into(
    path: &Path,
    defs: &mut HashMap<String, String>,
    visited: &mut std::collections::HashSet<PathBuf>,
    depth: u8,
) -> Option<()> {
    if depth > 2 || visited.len() >= 16 {
        return None;
    }
    let canonical = path.to_path_buf();
    if !visited.insert(canonical.clone()) {
        return Some(());
    }
    let meta = std::fs::metadata(&canonical).ok()?;
    if meta.len() > MAX_CSS_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(&canonical).ok()?;
    let text = strip_comments(&text);
    for (name, value) in define_colors(&text) {
        // Il primo file vince: gtk.css può importare altri fogli.
        defs.entry(name).or_insert(value);
    }
    if depth < 2 {
        for import in imports(&text) {
            if import.starts_with("resource://") || import.starts_with("file://") {
                continue;
            }
            let next = canonical
                .parent()
                .map(|parent| parent.join(&import))
                .unwrap_or_else(|| PathBuf::from(&import));
            if next.is_file() {
                let _ = read_css_into(&next, defs, visited, depth + 1);
            }
        }
    }
    Some(())
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn define_colors(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let needle = "@define-color";
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find(needle) {
        let start = cursor + found + needle.len();
        let rest = &text[start..];
        let name_end = rest
            .find(|ch: char| ch.is_whitespace())
            .unwrap_or(rest.len());
        let name = rest[..name_end].trim();
        let after = &rest[name_end..];
        let Some(end) = after.find(';') else {
            break;
        };
        let value = after[..end].trim();
        if !name.is_empty() && !value.is_empty() {
            out.push((name.to_string(), value.to_string()));
        }
        cursor = start + name_end + end + 1;
    }
    out
}

fn imports(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(found) = text[cursor..].find("@import") {
        let start = cursor + found + "@import".len();
        let rest = text[start..].trim_start();
        let raw = if let Some(inner) = rest.strip_prefix("url(") {
            inner
                .split_once(')')
                .map(|(value, _)| value.trim().trim_matches('"').trim_matches('\''))
        } else {
            rest.split_once(';')
                .map(|(value, _)| value.trim().trim_matches('"').trim_matches('\''))
        };
        if let Some(raw) = raw {
            if !raw.is_empty() {
                out.push(raw.to_string());
            }
        }
        cursor = start;
    }
    out
}

fn pick_colors(defs: &HashMap<String, String>) -> Option<Colors> {
    let get = |names: &[&str]| -> Option<Color32> {
        names
            .iter()
            .find_map(|name| defs.get(*name))
            .and_then(|raw| resolve_color(raw, defs, 0))
    };
    let bg = get(&[
        "theme_bg_color",
        "window_bg_color",
        "theme_unfocused_bg_color",
        "headerbar_bg_color",
        "theme_selected_bg_color",
    ])?;
    let base = get(&["theme_base_color", "view_bg_color", "card_bg_color"]).unwrap_or(bg);
    let fg = get(&[
        "theme_fg_color",
        "window_fg_color",
        "theme_text_color",
        "view_fg_color",
        "theme_selected_fg_color",
    ])?;
    let selected = get(&[
        "theme_selected_bg_color",
        "accent_bg_color",
        "accent_color",
        "theme_fg_color",
    ])
    .unwrap_or(fg);
    Some(Colors {
        bg,
        base,
        fg,
        selected,
    })
}

struct Colors {
    bg: Color32,
    base: Color32,
    fg: Color32,
    selected: Color32,
}

/// Risolve un valore CSS (`#rrggbb`, `rgb()`, `@nome`, `alpha()`, `mix()`,
/// `shade()`) in un colore. Le funzioni si annidano: limite di profondità.
fn resolve_color(raw: &str, defs: &HashMap<String, String>, depth: u8) -> Option<Color32> {
    if depth > 8 {
        return None;
    }
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(name) = raw.strip_prefix('@') {
        return resolve_color(defs.get(name.trim())?, defs, depth + 1);
    }
    if let Some(hex) = raw.strip_prefix('#') {
        return color_from_hex(hex);
    }
    let head = raw
        .split_once('(')
        .map(|(head, _)| head.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if head.is_empty() {
        return match raw.to_ascii_lowercase().as_str() {
            "white" => Some(Color32::WHITE),
            "black" => Some(Color32::BLACK),
            "transparent" => Some(Color32::TRANSPARENT),
            _ => None,
        };
    }
    let args = split_args(raw);
    match head.as_str() {
        "rgb" | "rgba" => {
            if args.len() < 3 {
                return None;
            }
            let r = channel(args[0])?;
            let g = channel(args[1])?;
            let b = channel(args[2])?;
            Some(Color32::from_rgb(r, g, b))
        }
        "alpha" => {
            if args.len() < 2 {
                return None;
            }
            // Per la tavolozza l'opacità non serve: il colore resta quello.
            resolve_color(args[0], defs, depth + 1)
        }
        "mix" => {
            if args.len() < 3 {
                return None;
            }
            let a = resolve_color(args[0], defs, depth + 1)?;
            let b = resolve_color(args[1], defs, depth + 1)?;
            let t = args[2].trim().parse::<f32>().unwrap_or(0.5);
            Some(mix(a, b, t))
        }
        "shade" => {
            if args.len() < 2 {
                return None;
            }
            let base = resolve_color(args[0], defs, depth + 1)?;
            let factor = args[1].trim().parse::<f32>().unwrap_or(1.0);
            let scale = |value: u8| (value as f32 * factor).round().clamp(0.0, 255.0) as u8;
            Some(Color32::from_rgb(scale(base.r()), scale(base.g()), scale(base.b())))
        }
        _ => None,
    }
}

fn split_args(raw: &str) -> Vec<&str> {
    let inner = raw
        .split_once('(')
        .map(|(_, rest)| rest.trim_end_matches(')'))
        .unwrap_or(raw);
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (index, ch) in inner.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(inner[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    out.push(inner[start..].trim());
    out
}

fn channel(text: &str) -> Option<u8> {
    let text = text.trim();
    if let Some(percent) = text.strip_suffix('%') {
        let value = percent.trim().parse::<f32>().ok()?;
        return Some((value / 100.0 * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    let value = text.parse::<f32>().ok()?;
    // `rgb(1 2 3)` non è valido in CSS, ma `rgba(0,0,0,0.5)` sì: i canali
    // sono 0..255, l'alfa è gestito a parte e qui non arriva.
    Some(value.round().clamp(0.0, 255.0) as u8)
}

fn color_from_hex(hex: &str) -> Option<Color32> {
    let digits: Vec<u8> = hex
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .map(|ch| ch.to_digit(16).map(|value| value as u8))
        .collect::<Option<Vec<u8>>>()?;
    let expand = |value: u8| value * 17;
    match digits.len() {
        3 | 4 => Some(Color32::from_rgb(
            expand(digits[0]),
            expand(digits[1]),
            expand(digits[2]),
        )),
        6 | 8 => Some(Color32::from_rgb(
            digits[0] * 16 + digits[1],
            digits[2] * 16 + digits[3],
            digits[4] * 16 + digits[5],
        )),
        _ => None,
    }
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

fn ini_section_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim()
                .eq_ignore_ascii_case(section);
            continue;
        }
        if !inside || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            if name.trim().eq_ignore_ascii_case(key) {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

// ── font e stile egui ────────────────────────────────────────────────────

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
        assert_eq!(resolve("dark").mode, Mode::Dark);
        assert_eq!(resolve("chiaro").mode, Mode::Light);
        assert_eq!(resolve("light").mode, Mode::Light);
    }

    #[test]
    fn l_osservatore_e_leggero_e_non_cambia_nulla_da_solo() {
        // La prima chiamata registra l'impronta e non tocca la tavolozza;
        // le successive, ravvicinate, non fanno alcun lavoro.
        assert!(!watch("auto"));
        let before = pal();
        assert!(!watch("auto"));
        assert_eq!(pal().bg, before.bg);
    }

    #[test]
    fn i_nomi_dei_temi_si_capiscono() {
        assert_eq!(mode_from_name("Adwaita:dark"), Mode::Dark);
        assert_eq!(mode_from_name("BreezeDark"), Mode::Dark);
        assert_eq!(mode_from_name("Adwaita"), Mode::Light);
        assert_eq!(
            ini_value("[General]\nColorScheme=BreezeDark\n", "ColorScheme").as_deref(),
            Some("BreezeDark")
        );
    }

    #[test]
    fn i_colori_css_si_leggono() {
        let defs = HashMap::from([
            ("base".to_string(), "#f5f5f7".to_string()),
            ("accento".to_string(), "rgb(0, 122, 255)".to_string()),
            ("tenue".to_string(), "alpha(@base, 0.5)".to_string()),
            ("scuro".to_string(), "shade(@base, 0.2)".to_string()),
            ("mescolato".to_string(), "mix(@base, black, 0.5)".to_string()),
        ]);
        assert_eq!(
            resolve_color("@base", &defs, 0),
            Some(Color32::from_rgb(245, 245, 247))
        );
        assert_eq!(
            resolve_color("@accento", &defs, 0),
            Some(Color32::from_rgb(0, 122, 255))
        );
        assert_eq!(
            resolve_color("@tenue", &defs, 0),
            Some(Color32::from_rgb(245, 245, 247))
        );
        assert_eq!(
            resolve_color("@scuro", &defs, 0),
            Some(Color32::from_rgb(49, 49, 49))
        );
        assert_eq!(
            resolve_color("@mescolato", &defs, 0),
            Some(Color32::from_rgb(123, 123, 124))
        );
        assert_eq!(resolve_color("white", &defs, 0), Some(Color32::WHITE));
        assert_eq!(resolve_color("#fff", &defs, 0), Some(Color32::WHITE));
    }

    #[test]
    fn un_tema_nativo_diventa_una_tavolozza_leggibile() {
        let native = NativeTheme {
            name: "MacOS-Tahoe".to_string(),
            variant: "gtk-3.0".to_string(),
            dark: false,
            colors: 5,
            bg: Color32::from_rgb(245, 245, 247),
            base: Color32::from_rgb(255, 255, 255),
            fg: Color32::from_rgb(29, 29, 31),
            selected: Color32::from_rgb(0, 122, 255),
        };
        let palette = Palette::from_native(&native);
        assert!(palette.bg.r() >= 230);
        // La scala resta di grigi: nessun canale colorato.
        assert_eq!(palette.bg.r(), palette.bg.b());
        assert!(contrast(palette.text, palette.bg) >= 7.0);
        assert!(contrast(palette.own_text, palette.own_bubble) >= 7.0);
        assert!(contrast(palette.in_text, palette.in_bubble) >= 7.0);
        assert!(palette.own_bubble.r() < palette.own_text.r());
    }
}
