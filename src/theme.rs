//! Tema che parte dai colori veri del desktop, con i verdi di WhatsApp.
//!
//! Il client non disegna con GTK, ma può leggere il tema nativo: si prende il
//! nome del tema (`GTK_THEME`, `gsettings`, `settings.ini`, `kdeglobals`), si
//! apre il suo `gtk.css` (o `gtk-dark.css`) e si leggono le `@define-color`
//! che ogni tema GTK deve dichiarare — sfondo, caselle, testo, selezione.
//! Da lì nascono le tinte della tavolozza: si usano i grigi (chiari o scuri)
//! del tema per sfondo, pannelli e superfici, mentre le bolle inviate e gli
//! accenti restano verdi come nell'app del telefono. Un tema "MacOS Tahoe"
//! darà quindi i suoi grigi caldi, Adwaita i suoi, Breeze i suoi.
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
    /// Verde di WhatsApp: pastiglie dei non letti e pulsante di invio.
    pub accent: Color32,
    pub accent_text: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(11, 20, 26),
            panel: Color32::from_rgb(17, 27, 33),
            surface: Color32::from_rgb(32, 44, 51),
            hover: Color32::from_rgb(42, 57, 66),
            border: Color32::from_rgb(42, 57, 66),
            text: Color32::from_rgb(233, 237, 239),
            muted: Color32::from_rgb(134, 150, 160),
            faint: Color32::from_rgb(102, 119, 129),
            white: Color32::from_rgb(255, 255, 255),
            // Verde WhatsApp: la bolla inviata è verde scuro con testo chiaro,
            // quella ricevuta è grigio-blu con lo stesso testo.
            own_bubble: Color32::from_rgb(0, 92, 75),
            own_text: Color32::from_rgb(233, 237, 239),
            in_bubble: Color32::from_rgb(32, 44, 51),
            in_text: Color32::from_rgb(233, 237, 239),
            danger: Color32::from_rgb(241, 92, 109),
            online: Color32::from_rgb(37, 211, 102),
            accent: Color32::from_rgb(0, 168, 132),
            accent_text: Color32::from_rgb(6, 37, 31),
        }
    }

    pub fn light() -> Self {
        Self {
            // Il beige del desktop di WhatsApp: il bianco delle bolle si
            // stacca senza bisogno di bordi marcati.
            bg: Color32::from_rgb(239, 234, 226),
            panel: Color32::from_rgb(255, 255, 255),
            surface: Color32::from_rgb(240, 242, 245),
            hover: Color32::from_rgb(233, 237, 239),
            border: Color32::from_rgb(209, 215, 219),
            text: Color32::from_rgb(17, 27, 33),
            muted: Color32::from_rgb(102, 119, 129),
            faint: Color32::from_rgb(134, 150, 160),
            white: Color32::from_rgb(255, 255, 255),
            own_bubble: Color32::from_rgb(217, 253, 211),
            own_text: Color32::from_rgb(17, 27, 33),
            in_bubble: Color32::from_rgb(255, 255, 255),
            in_text: Color32::from_rgb(17, 27, 33),
            danger: Color32::from_rgb(200, 28, 60),
            online: Color32::from_rgb(31, 160, 85),
            accent: Color32::from_rgb(0, 128, 105),
            accent_text: Color32::from_rgb(255, 255, 255),
        }
    }

    /// Tavolozza costruita sui colori del tema nativo: le superfici sono i
    /// grigi del tema (stessa luminosità, niente tinte stonate), mentre il
    /// verde delle bolle e l'accento restano quelli di WhatsApp. Il testo è
    /// garantito leggibile anche sui temi con poco contrasto.
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
        // La bolla inviata resta il segno forte della chat: verde WhatsApp,
        // nella tonalità giusta per il chiaroscuro del tema (scura su tema
        // scuro, chiara su tema chiaro). Il resto della tavolozza continua a
        // venire dai grigi del tema, così l'app non stona sulla scrivania.
        let (own_bubble, own_text) = (preset.own_bubble, preset.own_text);
        // La bolla ricevuta deve staccarsi dallo sfondo: su molti temi scuri
        // il colore "base" è quasi identico a quello della finestra e la chat
        // diventerebbe piatta. La tinta viene dal testo (già corretto per la
        // leggibilità), quindi resta un grigio del tema.
        let in_bubble = if native.dark {
            let lifted = mix(bg, text, 0.12);
            if (lifted.r() as i16 - bg.r() as i16).abs() < 10 {
                shift(bg, 14)
            } else {
                lifted
            }
        } else if (panel.r() as i16 - bg.r() as i16).abs() < 8 {
            mix(bg, text, 0.06)
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
            accent: preset.accent,
            accent_text: preset.accent_text,
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

/// Dimentica il tema automatico già letto: il prossimo [`resolve`] torna a
/// guardare il desktop. Serve quando l'utente ripassa ad "automatico".
pub fn forget() {
    if let Ok(mut cache) = AUTO.lock() {
        *cache = None;
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
    // Nessun colore leggibile, ma un nome di tema sì: va detto, perché la
    // diagnosi deve spiegare cosa il desktop ha chiesto.
    let source = if let Some(name) = desktop.name.as_deref() {
        format!(
            "tema GTK {name} ({})",
            if desktop.source.is_empty() {
                "desktop"
            } else {
                &desktop.source
            }
        )
    } else if !desktop.source.is_empty() {
        desktop.source
    } else if desktop.hint.is_some() {
        format!("preferisce {}", mode.label())
    } else {
        "predefinito".to_string()
    };
    Detected::preset(mode, source)
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
                    source: "gsettings".to_string(),
                    hint: hint_from_scheme,
                    kde: None,
                };
            }
        }
    }
    if let Some((name, hint)) = gtk_settings_ini() {
        if let Some(name) = name {
            return Desktop {
                name: Some(name.clone()),
                source: "settings.ini".to_string(),
                hint: hint.or(hint_from_scheme),
                kde: None,
            };
        }
        // Solo la preferenza chiaro/scuro: si continua a cercare un tema,
        // ma la scelta resta quella dichiarata da GTK.
        hint_from_scheme = hint_from_scheme.or(hint);
    }
    if !gnome_like {
        if let Some((name, hint)) = gsettings_theme() {
            hint_from_scheme = hint_from_scheme.or(hint);
            if let Some(name) = name {
                return Desktop {
                    name: Some(name.clone()),
                    source: "gsettings".to_string(),
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

/// Restituisce il nome del tema (se c'è) e la preferenza chiaro/scuro.
fn gtk_settings_ini() -> Option<(Option<String>, Option<Mode>)> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    for variant in ["gtk-4.0", "gtk-3.0"] {
        let path = home.join(".config").join(variant).join("settings.ini");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let name = ini_value(&text, "gtk-theme-name")
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty());
        let dark = ini_value(&text, "gtk-application-prefer-dark-theme").and_then(|value| {
            match value.trim() {
                "1" | "true" | "True" => Some(Mode::Dark),
                "0" | "false" | "False" => None,
                _ => None,
            }
        });
        if name.is_some() || dark.is_some() {
            return Some((name, dark));
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

/// Fondo della riga scelta nella lista: un velo del verde di WhatsApp sul
/// pannello, abbastanza da leggersi ma senza diventare una fascia colorata.
pub fn selected_row() -> Color32 {
    // Un filo più deciso su tema scuro, dove il verde si perde nel fondo.
    mix(pal().panel, pal().accent, if mode().is_dark() { 0.34 } else { 0.14 })
}

/// Coppie di colori degli avatar (alto, basso): gli stessi toni pieni che
/// WhatsApp e i contatti del telefono usano al posto della foto. Sono fissi,
/// non dipendono dal tema: un contatto ha sempre lo stesso colore.
const AVATAR_PAIRS: [((u8, u8, u8), (u8, u8, u8)); 8] = [
    ((92, 155, 245), (47, 95, 184)),   // blu
    ((165, 132, 224), (107, 69, 176)), // viola
    ((240, 122, 154), (184, 59, 94)),  // rosa
    ((242, 165, 75), (194, 109, 31)),  // arancio
    ((77, 201, 176), (31, 135, 116)),  // verde acqua
    ((123, 196, 123), (61, 130, 69)),  // verde
    ((232, 208, 90), (176, 141, 31)),  // giallo
    ((232, 123, 123), (176, 63, 74)),  // rosso
];

/// Colore (alto, basso) dell'avatar di un contatto: sempre lo stesso per lo
/// stesso indirizzo, così la lista non cambia faccia a ogni avvio.
pub fn avatar(id: &str) -> (Color32, Color32) {
    let mut hash: u32 = 2_166_136_261;
    for byte in id.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(16_777_619);
    }
    let ((tr, tg, tb), (br, bg, bb)) = AVATAR_PAIRS[(hash as usize) % AVATAR_PAIRS.len()];
    (
        Color32::from_rgb(tr, tg, tb),
        Color32::from_rgb(br, bg, bb),
    )
}

/// Inchiostro delle iniziali: bianco sui toni scuri, quasi nero sui toni
/// chiari (giallo, verde), dove il bianco non si leggerebbe.
pub fn avatar_ink(id: &str) -> Color32 {
    let (top, bottom) = avatar(id);
    let middle = mix(top, bottom, 0.5);
    if contrast(Color32::WHITE, middle) >= 3.0 {
        Color32::WHITE
    } else {
        Color32::from_rgb(16, 32, 28)
    }
}

/// Nome del mittente nei gruppi: la tinta dell'avatar, schiarita quanto basta
/// per leggersi sulla bolla.
pub fn sender_color(id: &str) -> Color32 {
    let (top, _) = avatar(id);
    let on_dark = mode().is_dark();
    if on_dark {
        mix(top, Color32::WHITE, 0.35)
    } else {
        mix(top, Color32::BLACK, 0.30)
    }
}

/// Luminosità percepita di un colore (0-255): è il ponte fra il tema del
/// desktop e la tavolozza.
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
        // Fra `@define-color` e il nome c'è sempre uno spazio: senza saltarlo
        // il nome risulterebbe vuoto e il tema sembrerebbe privo di colori.
        let rest = text[start..].trim_start();
        let skipped = text[start..].len() - rest.len();
        let name_end = rest.find(|ch: char| ch.is_whitespace()).unwrap_or(rest.len());
        let name = rest[..name_end].trim();
        let after = &rest[name_end..];
        let Some(end) = after.find(';') else {
            break;
        };
        let value = after[..end].trim();
        if !name.is_empty() && !value.is_empty() {
            out.push((name.to_string(), value.to_string()));
        }
        cursor = start + skipped + name_end + end + 1;
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

pub(crate) fn ini_value(text: &str, key: &str) -> Option<String> {
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

pub(crate) fn ini_section_value(text: &str, section: &str, key: &str) -> Option<String> {
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

/// Il carattere effettivamente in uso, per la diagnostica e le impostazioni.
#[derive(Clone, Debug, Default)]
pub struct FontInfo {
    pub family: String,
    pub file: String,
    pub mono: String,
    pub fallbacks: usize,
    /// `true` se il sistema non aveva font e sono rimasti quelli incorporati.
    pub builtin: bool,
}

static FONT_INFO: Mutex<Option<FontInfo>> = Mutex::new(None);

pub fn font_info() -> Option<FontInfo> {
    FONT_INFO
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

/// Sceglie il carattere dell'interfaccia **dal sistema**.
///
/// La famiglia arriva da `gtk-font-name` / `kdeglobals` e si cerca il file
/// corrispondente nelle cartelle dei font; se il desktop non dice nulla si
/// prova con le famiglie più comuni. I font incorporati nel binario restano
/// solo come ultima risorsa, quando nella macchina non c'è nessun font.
pub fn install_system_fonts(ctx: &egui::Context) {
    let builtin = font_info().map(|info| info.builtin).unwrap_or(false);
    if std::env::var_os("WHATS_OPEN_NO_SYSTEM_FONTS").is_some() {
        // Lo stesso vale quando i font sono già quelli incorporati: rifarli a
        // ogni cambio di tema butterebbe via la cache di egui senza motivo.
        if !builtin {
            set_builtin_fonts(ctx);
        }
        return;
    }
    // Cambiare tema non deve far rileggere i font: una volta scelti restano.
    if font_info().map(|info| !info.builtin).unwrap_or(false) {
        return;
    }
    let plan = crate::platform::font_plan();
    let Some(ui) = plan.ui.clone() else {
        log::warn!("nessun font di sistema: restano quelli incorporati");
        set_builtin_fonts(ctx);
        return;
    };

    // I font vanno provati prima di consegnarli a egui: se un file è rotto la
    // libreria entra in panico al primo fotogramma. Meglio rinunciare a una
    // riserva che far cadere la finestra.
    let mut faces: Vec<LoadedFont> = Vec::new();
    faces.push(LoadedFont {
        key: "sys:interfaccia".to_string(),
        file: ui.file.clone(),
        data: std::sync::Arc::new(egui::FontData::from_owned(ui.bytes)),
        mono: false,
        ui: true,
    });
    let mono_name = plan
        .mono
        .as_ref()
        .map(|mono| mono.family.clone())
        .unwrap_or_default();
    if let Some(mono) = plan.mono.clone() {
        faces.push(LoadedFont {
            key: "sys:spaziatura".to_string(),
            file: mono.file.clone(),
            data: std::sync::Arc::new(egui::FontData::from_owned(mono.bytes)),
            mono: true,
            ui: false,
        });
    }
    for (index, face) in plan.fallback.into_iter().enumerate() {
        let mut data = egui::FontData::from_owned(face.bytes);
        // Le emoji di Noto sono disegnate grandi: senza questa correzione
        // sbilanciano la riga rispetto al testo.
        if face.file.to_ascii_lowercase().contains("emoji") {
            data = data.tweak(egui::FontTweak {
                scale: 0.85,
                ..Default::default()
            });
        }
        faces.push(LoadedFont {
            key: format!("sys:riserva{index}"),
            file: face.file,
            data: std::sync::Arc::new(data),
            mono: false,
            ui: false,
        });
    }

    if !fonts_are_loadable(&faces) {
        let before = faces.len();
        let broken: Vec<(String, String)> = faces
            .iter()
            .filter(|face| !fonts_are_loadable(std::slice::from_ref(face)))
            .map(|face| (face.key.clone(), face.file.clone()))
            .collect();
        faces.retain(|face| !broken.iter().any(|(key, _)| key == &face.key));
        let rejected: Vec<String> = broken.into_iter().map(|(_, file)| file).collect();
        log::warn!(
            "font non leggibili, scartati: {} (su {before})",
            rejected.join(", ")
        );
    }

    if !faces.iter().any(|face| face.ui) {
        log::warn!("il carattere dell'interfaccia non è leggibile: restano quelli incorporati");
        set_builtin_fonts(ctx);
        return;
    }
    let ui_family = ui.family.clone();
    let ui_file = ui.file.clone();
    let mono_family = if faces.iter().any(|face| face.mono) {
        mono_name
    } else {
        String::new()
    };
    let fallbacks = faces.iter().filter(|face| !face.ui && !face.mono).count();

    let mut definitions = egui::FontDefinitions::empty();
    let mut proportional: Vec<String> = Vec::new();
    let mut monospace: Vec<String> = Vec::new();
    for face in faces {
        definitions
            .font_data
            .insert(face.key.clone(), face.data);
        if face.mono {
            // La spaziatura fissa va davanti solo nella famiglia dei
            // monospazi; nei testi normali è una buona seconda scelta.
            monospace.push(face.key.clone());
        }
        proportional.push(face.key.clone());
        monospace.push(face.key);
    }
    definitions
        .families
        .insert(egui::FontFamily::Proportional, proportional);
    definitions
        .families
        .insert(egui::FontFamily::Monospace, monospace);
    ctx.set_fonts(definitions);
    log::info!(
        "font di sistema: {ui_family} ({ui_file}) · spaziatura: {} · {fallbacks} riserve · {} file visti",
        if mono_family.is_empty() {
            "incorporata"
        } else {
            mono_family.as_str()
        },
        plan.scanned
    );
    if let Ok(mut current) = FONT_INFO.lock() {
        *current = Some(FontInfo {
            family: ui_family,
            file: ui_file,
            mono: mono_family,
            fallbacks,
            builtin: false,
        });
    }
}

/// Un font pronto per egui, con il ruolo che avrà nella catena.
struct LoadedFont {
    key: String,
    file: String,
    data: std::sync::Arc<egui::FontData>,
    /// Va davanti nella famiglia a spaziatura fissa.
    mono: bool,
    /// È il carattere dell'interfaccia.
    ui: bool,
}

/// Chiede a epaint di costruire i font *prima* di installarli: se un file non
/// è leggibile la libreria entra in panico, e qui il panico si ferma.
fn fonts_are_loadable(faces: &[LoadedFont]) -> bool {
    let mut definitions = egui::FontDefinitions::empty();
    let mut names = Vec::new();
    for face in faces {
        definitions
            .font_data
            .insert(face.key.clone(), face.data.clone());
        names.push(face.key.clone());
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        definitions.families.insert(family, names.clone());
    }
    let probe = std::panic::AssertUnwindSafe(|| {
        let _ = egui::epaint::text::Fonts::new(
            egui::epaint::text::TextOptions::default(),
            definitions,
        );
    });
    // Il messaggio di panico di una prova non interessa a nessuno: lo si
    // zittisce per la durata del controllo e si rimette com'era dopo.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let loaded = std::panic::catch_unwind(probe).is_ok();
    std::panic::set_hook(previous);
    loaded
}

/// I font compilati nel binario: ultima risorsa quando il sistema non ne ha.
fn set_builtin_fonts(ctx: &egui::Context) {
    ctx.set_fonts(egui::FontDefinitions::default());
    if let Ok(mut current) = FONT_INFO.lock() {
        *current = Some(FontInfo {
            family: "incorporato".to_string(),
            file: String::new(),
            mono: String::new(),
            fallbacks: 0,
            builtin: true,
        });
    }
}

/// Sostituisce i font senza toccare il resto del tema.
pub fn set_system_fonts(ctx: &egui::Context, enabled: bool) {
    if enabled {
        install_system_fonts(ctx);
    } else {
        // Torna ai soli font incorporati nel binario.
        set_builtin_fonts(ctx);
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
        // La bolla inviata è verde in entrambi i temi, con il testo leggibile:
        // verde scuro su fondo scuro, verde chiaro su fondo chiaro.
        assert!(dark.own_bubble.g() > dark.own_bubble.r());
        assert!(light.own_bubble.g() > light.own_bubble.r());
        assert!(contrast(dark.own_text, dark.own_bubble) >= 4.5);
        assert!(contrast(light.own_text, light.own_bubble) >= 4.5);
        assert!(contrast(dark.accent_text, dark.accent) >= 4.5);
        assert!(contrast(light.accent_text, light.accent) >= 4.5);
        // Il bianco pieno serve solo sopra le foto.
        assert_eq!(dark.white, light.white);
    }

    #[test]
    fn ogni_contatto_ha_sempre_lo_stesso_avatar() {
        let (first, second) = avatar("393481112222@s.whatsapp.net");
        assert_eq!(first, avatar("393481112222@s.whatsapp.net").0);
        assert_eq!(second, avatar("393481112222@s.whatsapp.net").1);
        // Indirizzi diversi non finiscono tutti sullo stesso colore.
        let mut seen = std::collections::HashSet::new();
        for index in 0..12 {
            let (top, _) = avatar(&format!("3934800000{index:02}@s.whatsapp.net"));
            seen.insert((top.r(), top.g(), top.b()));
        }
        assert!(seen.len() >= 4, "gli avatar si ripetono troppo: {}", seen.len());
        // Le iniziali restano leggibili su ogni tinta.
        for index in 0..12 {
            let id = format!("3934800000{index:02}@s.whatsapp.net");
            let (top, bottom) = avatar(&id);
            let ratio = contrast(avatar_ink(&id), mix(top, bottom, 0.5));
            assert!(ratio >= 3.0, "iniziali poco leggibili ({ratio:.1}:1) su {id}");
        }
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
    fn le_bolle_ricevute_si_staccano_dallo_sfondo() {
        // Tema scuro con "base" quasi uguale alla finestra (MacOS Tahoe).
        let dark = NativeTheme {
            name: "MacOS-Tahoe".to_string(),
            variant: "gtk-3.0".to_string(),
            dark: true,
            colors: 5,
            bg: Color32::from_rgb(51, 51, 51),
            base: Color32::from_rgb(36, 36, 36),
            fg: Color32::from_rgb(222, 222, 222),
            selected: Color32::from_rgb(0, 136, 255),
        };
        let palette = Palette::from_native(&dark);
        assert!(
            (palette.in_bubble.r() as i16 - palette.bg.r() as i16).abs() >= 12,
            "bolla {} troppo vicina allo sfondo {}",
            palette.in_bubble.r(),
            palette.bg.r()
        );
        assert!(contrast(palette.in_text, palette.in_bubble) >= 7.0);

        // Tema chiaro con "base" uguale allo sfondo: la bolla resta visibile.
        let light = NativeTheme {
            dark: false,
            bg: Color32::from_rgb(250, 250, 250),
            base: Color32::from_rgb(250, 250, 250),
            fg: Color32::from_rgb(30, 30, 30),
            ..dark
        };
        let palette = Palette::from_native(&light);
        assert!((palette.in_bubble.r() as i16 - palette.bg.r() as i16).abs() >= 8);
        assert!(contrast(palette.in_text, palette.in_bubble) >= 7.0);
    }

    #[test]
    fn i_colori_si_leggono_dal_testo_del_tema() {
        // Il caso vero: `@define-color` seguito da uno spazio, più righe.
        let css = "/* commento */\n@define-color theme_bg_color #f2f2f4;\n\
                   @define-color theme_fg_color rgb(29, 29, 31);\n\
                   @define-color theme_selected_bg_color @accento;\n\
                   @define-color accento #007aff;\n";
        let defs: HashMap<String, String> = define_colors(&strip_comments(css)).into_iter().collect();
        assert_eq!(defs.get("theme_bg_color").map(String::as_str), Some("#f2f2f4"));
        assert_eq!(
            defs.get("theme_fg_color").map(String::as_str),
            Some("rgb(29, 29, 31)")
        );
        let colors = pick_colors(&defs).expect("i quattro colori principali");
        assert_eq!(colors.bg, Color32::from_rgb(242, 242, 244));
        assert_eq!(colors.fg, Color32::from_rgb(29, 29, 31));
        assert_eq!(colors.selected, Color32::from_rgb(0, 122, 255));
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
        // Sul tema chiaro la bolla inviata è verde chiaro con testo scuro.
        assert!(palette.own_bubble.g() > palette.own_bubble.r());
        assert!(palette.own_bubble.r() > palette.own_text.r());
    }
}
