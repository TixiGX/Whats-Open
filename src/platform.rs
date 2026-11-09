//! Adattamento alla distribuzione: rilevamento dell'ambiente, apertura di file
//! con il programma di sistema, font di riserva, istanza singola, diagnosi.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Apre un file con il player/visualizzatore preferito dal desktop.
///
/// Prova in ordine i lanciatori che esistono praticamente ovunque; il primo
/// che parte vince. Restituisce `true` se un processo è stato avviato.
pub fn open_external(path: &Path) -> bool {
    let candidates: [&[&str]; 6] = [
        &["xdg-open"],
        &["gio", "open"],
        &["kde-open5"],
        &["kde-open"],
        &["gnome-open"],
        &["exo-open", "--launch", "FileManager"],
    ];

    for (index, command) in candidates.iter().enumerate() {
        let program = command[0];
        if !command_exists(program) {
            continue;
        }
        let mut child = std::process::Command::new(program);
        if index == 0 {
            child.arg(path);
        } else {
            child.args(&command[1..]).arg(path);
        }
        if child
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            return true;
        }
    }
    false
}

pub fn command_exists(name: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&paths) {
        if is_executable(&dir.join(name)) {
            return true;
        }
    }
    false
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Il portale XDG non sta nel PATH: vive in `/usr/libexec` o simili.
fn portal_present() -> bool {
    if command_exists("xdg-desktop-portal") {
        return true;
    }
    [
        "/usr/libexec/xdg-desktop-portal",
        "/usr/lib/xdg-desktop-portal",
        "/usr/libexec/xdg-desktop-portal-gtk",
        "/usr/libexec/xdg-desktop-portal-kde",
        "/usr/lib/xdg-desktop-portal-gtk",
    ]
    .iter()
    .any(|path| is_executable(Path::new(path)))
}

/// Nome della distribuzione e versione, da `/etc/os-release`.
pub fn distro_name() -> String {
    let Ok(text) = std::fs::read_to_string("/etc/os-release") else {
        return "Linux".to_string();
    };
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return value.trim_matches('"').to_string();
        }
    }
    "Linux".to_string()
}

/// Informazioni utili quando qualcosa non funziona su una macchina nuova.
pub fn report(paths: &crate::util::Paths) -> String {
    let mut out = String::new();
    out.push_str(&format!("Whats-Open {}\n", env!("CARGO_PKG_VERSION")));
    out.push_str(&format!("distribuzione : {}\n", distro_name()));
    out.push_str(&format!(
        "kernel        : {}\n",
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|_| "?".to_string())
    ));
    out.push_str(&format!("architettura  : {}\n", std::env::consts::ARCH));

    let session = match (
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var_os("DISPLAY").is_some(),
    ) {
        (true, true) => "Wayland + X11",
        (true, false) => "Wayland",
        (false, true) => "X11",
        (false, false) => "nessuna (terminale/SSH?)",
    };
    out.push_str(&format!("sessione      : {session}\n"));
    out.push_str(&format!(
        "desktop       : {}\n",
        ["XDG_CURRENT_DESKTOP", "DESKTOP_SESSION"]
            .iter()
            .filter_map(|key| std::env::var(key).ok())
            .collect::<Vec<_>>()
            .join(" / ")
    ));

    out.push_str(&format!(
        "glibc         : {}\n",
        glibc_version().unwrap_or_else(|| "musl/statico".to_string())
    ));

    let has_ffmpeg = command_exists("ffmpeg");
    let has_xdg_open = command_exists("xdg-open");
    let has_notify = session_bus_present();
    let has_portal = portal_present();
    for (label, found) in [
        ("ffmpeg", has_ffmpeg),
        ("ffprobe", command_exists("ffprobe")),
        ("xdg-open", has_xdg_open),
        ("ffplay", command_exists("ffplay")),
        ("notifiche", has_notify),
        ("portale", has_portal),
    ] {
        out.push_str(&format!("{:<14}: {}\n", label, if found { "sì" } else { "no" }));
    }

    if let Some(device) = default_audio_device() {
        out.push_str(&format!("audio         : {device}\n"));
    } else {
        out.push_str("audio         : nessun dispositivo di uscita\n");
    }

    // Quale carattere userà davvero la finestra: è la prima cosa da sapere
    // quando l'interfaccia "non somiglia" al resto della scrivania.
    let plan = font_plan();
    out.push_str(&format!(
        "font interfaccia: {}\n",
        match plan.ui.as_ref() {
            Some(face) => format!("{} ({})", face.family, face.file),
            None => "nessuno: si usano i font incorporati".to_string(),
        }
    ));
    if let Some(mono) = plan.mono.as_ref() {
        out.push_str(&format!("font spaziatura : {}\n", mono.family));
    }
    out.push_str(&format!(
        "font di riserva  : {} file (su {} visti)\n",
        plan.fallback.len(),
        plan.scanned
    ));

    let mut advice: Vec<String> = Vec::new();
    if !has_ffmpeg {
        advice.push(format!(
            "ffmpeg — decodifica di audio e video: {}",
            install_hint("ffmpeg", "ffmpeg", "ffmpeg", "ffmpeg", "ffmpeg")
        ));
    }
    if !has_xdg_open {
        advice.push(format!(
            "xdg-utils — apertura dei documenti: {}",
            install_hint(
                "xdg-utils",
                "xdg-utils",
                "xdg-utils",
                "xdg-utils",
                "xdg-utils"
            )
        ));
    }
    if !has_notify {
        advice.push("notifiche di sistema — serve una sessione desktop con D-Bus".to_string());
    }
    if !has_portal {
        advice.push(format!(
            "xdg-desktop-portal — finestra \"allega file\": {}",
            install_hint(
                "xdg-desktop-portal zenity",
                "xdg-desktop-portal zenity",
                "xdg-desktop-portal zenity",
                "xdg-desktop-portal zenity",
                "xdg-desktop-portal zenity"
            )
        ));
    }
    if plan.fallback.is_empty() {
        advice.push(format!(
            "font Noto — emoji e scritture non latine: {}",
            install_hint(
                "fonts-noto-core fonts-noto-color-emoji",
                "google-noto-sans-fonts google-noto-color-emoji-fonts",
                "noto-fonts noto-fonts-emoji",
                "noto-sans-fonts noto-coloremoji-fonts",
                "font-noto font-noto-emoji"
            )
        ));
    }
    advice.retain(|line| !line.ends_with(": "));
    if !advice.is_empty() {
        out.push_str("\nper completare l'installazione:\n");
        for line in advice {
            out.push_str(&format!("  - {line}\n"));
        }
    }

    out.push_str(&format!("dati          : {}\n", paths.data.display()));
    out.push_str(&format!("cache         : {}\n", paths.cache.display()));
    out.push_str(&format!("sessione      : {}\n", paths.session.display()));
    for (label, path) in [("archivio", &paths.db), ("sessione", &paths.session)] {
        out.push_str(&format!(
            "{:<14}: {}\n",
            label,
            if path.exists() {
                format!(
                    "presente ({})",
                    crate::util::human_size(
                        std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
                    )
                )
            } else {
                "assente".to_string()
            }
        ));
    }

    out
}

/// Vero se c'è un bus di sessione raggiungibile: è tutto ciò che serve alle
/// notifiche (il servizio vero e proprio può anche non esserci).
pub fn session_bus_present() -> bool {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return true;
    }
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|runtime| std::path::Path::new(&runtime).join("bus").exists())
        .unwrap_or(false)
}

fn default_audio_device() -> Option<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let name = device
        .description()
        .map(|description| description.name().to_string())
        .unwrap_or_else(|_| "sconosciuto".to_string());
    Some(name)
}

fn glibc_version() -> Option<String> {
    let mut command = std::process::Command::new("ldd");
    command.arg("--version");
    let output = command.output().ok()?;
    // Con musl `ldd --version` risponde "musl libc (x86_64)": non è glibc.
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let first = lines.next()?;
    if first.contains("GLIBC") || first.contains("glibc") || first.contains("GNU libc") {
        Some(first.to_string())
    } else {
        None
    }
}

// ── font di sistema ──────────────────────────────────────────────────────

/// Un volto di carattere letto da disco, pronto per egui.
#[derive(Clone)]
pub struct FontFace {
    /// Nome leggibile della famiglia ("DejaVu Sans"), ricavato dal file.
    pub family: String,
    /// Nome del file, per la diagnostica.
    pub file: String,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

/// Tetti di memoria per i font: l'interfaccia ne usa uno, la spaziatura uno,
/// e le riserve restano entro il budget. Così anche su una distribuzione con
/// collezioni CJK enormi la memoria non esplode.
const FONT_UI_BUDGET: usize = 12 * 1024 * 1024;
const FONT_MONO_BUDGET: usize = 6 * 1024 * 1024;
const FONT_FALLBACK_BUDGET: usize = 20 * 1024 * 1024;

/// Il piano dei font: quello dell'interfaccia, quello a spaziatura fissa e le
/// riserve per emoji e scritture non latine.
#[derive(Clone, Default)]
pub struct FontPlan {
    pub ui: Option<FontFace>,
    pub mono: Option<FontFace>,
    pub fallback: Vec<FontFace>,
    /// Quanti file di font sono stati visti nelle cartelle del sistema.
    pub scanned: usize,
}

/// Le famiglie che hanno l'aspetto giusto per una finestra di chat, in ordine
/// di preferenza quando il desktop non dice la sua.
const UI_FAMILIES: [&str; 16] = [
    "Inter",
    "Adwaita Sans",
    "Cantarell",
    "Ubuntu",
    "Noto Sans",
    "DejaVu Sans",
    "Liberation Sans",
    "Roboto",
    "Open Sans",
    "Lato",
    "Source Sans 3",
    "Source Sans Pro",
    "Fira Sans",
    "FreeSans",
    "Segoe UI",
    "Arial",
];

/// Come può proseguire il nome di un file dopo la famiglia: "Cantarell**Bold**",
/// "NotoSans**-Regular**" (i separatori sono già tolti), "Inter**Variable**".
const STYLE_WORDS: [&str; 20] = [
    "regular",
    "roman",
    "book",
    "text",
    "medium",
    "semibold",
    "demibold",
    "bold",
    "black",
    "heavy",
    "light",
    "extralight",
    "ultralight",
    "thin",
    "italic",
    "oblique",
    "condensed",
    "narrow",
    "variable",
    "wght",
];

/// Famiglie a spaziatura fissa, dalla più gradevole alla più comune.
const MONO_FAMILIES: [&str; 10] = [
    "JetBrains Mono",
    "Fira Code",
    "Cascadia Code",
    "Source Code Pro",
    "Ubuntu Mono",
    "Noto Sans Mono",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Hack",
    "FreeMono",
];

/// File caricati come riserva: emoji, simboli e alfabeti non latini. Servono
/// perché il font dell'interfaccia, da solo, non li disegna.
const FALLBACK_FILES: [&str; 18] = [
    "NotoEmoji-Regular.ttf",
    "NotoEmoji-VariableFont_wght.ttf",
    "NotoSansSymbols2-Regular.ttf",
    "NotoSansSymbols-Regular.ttf",
    "Symbola.ttf",
    "NotoSansArabic-Regular.ttf",
    "NotoSansHebrew-Regular.ttf",
    "NotoSansDevanagari-Regular.ttf",
    "NotoSansBengali-Regular.ttf",
    "NotoSansTamil-Regular.ttf",
    "NotoSansThai-Regular.ttf",
    "NotoSansCJK-Regular.ttc",
    "NotoSansCJKsc-Regular.otf",
    "NotoSansCJKjp-Regular.otf",
    "NotoSansSC-Regular.otf",
    "DejaVuSans.ttf",
    "unifont.ttf",
    "FreeSans.ttf",
];

/// Legge dal desktop quale famiglia usa per l'interfaccia. Nessun processo:
/// solo `settings.ini` di GTK e `kdeglobals`, come per il tema.
pub fn desktop_font_family() -> Option<String> {
    if let Ok(value) = std::env::var("WHATS_OPEN_FONT") {
        let value = family_of(&value);
        if !value.is_empty() {
            return Some(value);
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    for variant in ["gtk-4.0", "gtk-3.0"] {
        let path = home.join(".config").join(variant).join("settings.ini");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(value) = crate::theme::ini_value(&text, "gtk-font-name") {
            let family = family_of(&value);
            if !family.is_empty() {
                return Some(family);
            }
        }
    }
    let path = home.join(".config/kdeglobals");
    if let Ok(text) = std::fs::read_to_string(&path) {
        for key in ["font", "menuFont", "activeFont", "toolBarFont"] {
            if let Some(value) = crate::theme::ini_section_value(&text, "General", key) {
                let family = family_of(&value);
                if !family.is_empty() {
                    return Some(family);
                }
            }
        }
    }
    None
}

/// "Cantarell 11", "Noto Sans,10,-1,5,50,0,0,0,0,0" → "Cantarell", "Noto Sans".
fn family_of(value: &str) -> String {
    let value = value.trim();
    let value = value.split(',').next().unwrap_or(value).trim();
    let mut parts: Vec<&str> = value.split_whitespace().collect();
    while parts.len() > 1
        && parts
            .last()
            .map(|part| part.chars().all(|c| c.is_ascii_digit() || c == '.'))
            .unwrap_or(false)
    {
        parts.pop();
    }
    let family = parts.join(" ");
    // "Sans", "Serif" e simili sono categorie, non famiglie installate.
    match family.to_ascii_lowercase().as_str() {
        "" | "sans" | "sans-serif" | "serif" | "monospace" | "system-ui" => String::new(),
        _ => family,
    }
}

/// Sceglie il file che meglio rappresenta `family` fra quelli trovati.
///
/// Il confronto è sul nome del file senza separatori: "DejaVu Sans" trova
/// `DejaVuSans.ttf`, "Cantarell" trova `Cantarell-Regular.otf`. I pesi
/// (bold, light, italic) e le varianti strette perdono punti, così la
/// finestra usa il tondo normale.
fn pick_family(family: &str, files: &[PathBuf]) -> Option<PathBuf> {
    let wanted = normalize_family(family);
    if wanted.is_empty() {
        return None;
    }
    let mut best: Option<(u32, &PathBuf)> = None;
    for path in files {
        let Some(stem) = path.file_stem().map(|stem| stem.to_string_lossy().to_string()) else {
            continue;
        };
        let name = normalize_family(&stem);
        if name.is_empty() {
            continue;
        }
        // Il nome del file comincia con la famiglia e prosegue col peso:
        // "DejaVu Sans" + "bold". Così "Noto Sans" non finisce per scegliere
        // `NotoSansArabic` solo perché comincia con le stesse lettere.
        let mut rank: u32 = if name == wanted {
            0
        } else if name == format!("{wanted}regular") {
            1
        } else if let Some(rest) = name.strip_prefix(&wanted) {
            if !STYLE_WORDS.iter().any(|word| rest.starts_with(word)) {
                continue;
            }
            3
        } else {
            continue;
        };
        for (marker, penalty) in [
            ("bold", 4),
            ("black", 4),
            ("heavy", 4),
            ("semibold", 3),
            ("extrabold", 4),
            ("italic", 3),
            ("oblique", 3),
            ("light", 2),
            ("thin", 2),
            ("condensed", 2),
            ("narrow", 2),
            ("mono", 5),
            ("display", 3),
        ] {
            if name.contains(marker) {
                rank += penalty;
            }
        }
        if wanted.contains("mono") && name.contains("mono") {
            rank = rank.saturating_sub(5);
        }
        if best.as_ref().map(|(score, _)| rank < *score).unwrap_or(true) {
            best = Some((rank, path));
        }
    }
    best.map(|(_, path)| path.clone())
}

/// Minuscolo e senza separatori: il nome di un file e quello di una famiglia
/// si somigliano abbastanza da potersi confrontare così.
fn normalize_family(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// Ultima spiaggia: il primo file che non sia una raccolta di simboli.
/// Serve sulle distribuzioni che installano una sola famiglia, con un nome che
/// non è nell'elenco delle preferite: meglio quel carattere del sistema che
/// quello incorporato nel binario.
fn pick_any(files: &[PathBuf]) -> Option<PathBuf> {
    files
        .iter()
        .find(|path| {
            let name = normalize_family(&file_name(path));
            ["emoji", "symbol", "dingbat", "unifont", "icon"]
                .iter()
                .all(|bad| !name.contains(bad))
        })
        .cloned()
}

fn is_font_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
            .as_deref(),
        Some("ttf") | Some("otf") | Some("ttc") | Some("otc")
    )
}

fn font_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".local/share/fonts"));
        roots.push(home.join(".fonts"));
    }
    if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data).join("fonts"));
    }
    for root in [
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/run/host/fonts",
        "/run/host/usr/share/fonts",
    ] {
        roots.push(PathBuf::from(root));
    }
    roots
}

/// Raccoglie i file di font, fermandosi a un limite: su un sistema con
/// migliaia di file non serve elencarli tutti per scegliere il carattere.
fn collect_fonts(root: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    if depth > 3 || out.len() >= 4000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => collect_fonts(&path, depth + 1, out),
            Ok(kind) if kind.is_file() && is_font_file(&path) => out.push(path),
            _ => {}
        }
    }
}

fn read_font(path: &Path, budget: usize) -> Option<(String, Vec<u8>)> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() < 1024 || meta.len() as usize > budget {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if !font_magic_ok(&bytes) {
        return None;
    }
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let family = font_family_name(&bytes).unwrap_or(stem);
    Some((family, bytes))
}

fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// Legge il nome della famiglia dalla tabella `name` del file.
///
/// Sceglie il nome tipografico (nameID 16) quando c'è, altrimenti quello
/// classico (nameID 1). Per le collezioni `.ttc` usa il primo volto. Se il
/// file non ha una tabella leggibile si resta al nome del file: nessun
/// errore, la finestra funziona lo stesso.
fn font_family_name(bytes: &[u8]) -> Option<String> {
    let base = if bytes.starts_with(b"ttcf") {
        let count = be_u32(bytes, 8)?;
        if count == 0 {
            return None;
        }
        be_u32(bytes, 12)? as usize
    } else {
        0
    };
    let tables = be_u16(bytes, base + 4)? as usize;
    let mut name_table: Option<(usize, usize)> = None;
    for index in 0..tables.min(64) {
        let record = base + 12 + index * 16;
        if bytes.get(record..record + 4)? == b"name" {
            name_table = Some((be_u32(bytes, record + 8)? as usize, be_u32(bytes, record + 12)? as usize));
            break;
        }
    }
    let (offset, length) = name_table?;
    let table = bytes.get(offset..offset.checked_add(length)?)?;
    let count = be_u16(table, 2)? as usize;
    let strings = be_u16(table, 4)? as usize;
    let mut best: Option<(u8, String)> = None;
    for index in 0..count.min(256) {
        let record = 6 + index * 12;
        let platform = be_u16(table, record)?;
        let encoding = be_u16(table, record + 2)?;
        let name_id = be_u16(table, record + 6)?;
        let text_len = be_u16(table, record + 8)? as usize;
        let text_off = be_u16(table, record + 10)? as usize;
        if name_id != 1 && name_id != 16 {
            continue;
        }
        let start = strings.checked_add(text_off)?;
        let Some(raw) = table.get(start..start.checked_add(text_len)?) else {
            continue;
        };
        let text = if platform == 3 || platform == 0 {
            let units: Vec<u16> = raw
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            match String::from_utf16(&units) {
                Ok(text) => text,
                Err(_) => continue,
            }
        } else if platform == 1 && encoding == 0 {
            String::from_utf8_lossy(raw).to_string()
        } else {
            continue;
        };
        let text = text.trim().trim_matches('\0').to_string();
        if text.is_empty() {
            continue;
        }
        let priority = if name_id == 16 { 0 } else { 1 };
        if best.as_ref().map(|(best, _)| priority < *best).unwrap_or(true) {
            best = Some((priority, text));
        }
    }
    best.map(|(_, text)| text)
}

/// Prepara i font da usare per l'interfaccia. Non avvia processi e non tocca
/// la rete: legge solo le cartelle dei font e i file di configurazione.
pub fn font_plan() -> FontPlan {
    let mut files: Vec<PathBuf> = Vec::new();
    for root in font_roots() {
        if root.is_dir() {
            collect_fonts(&root, 0, &mut files);
        }
    }
    files.sort();
    files.dedup();
    let scanned = files.len();

    let preferred = desktop_font_family();
    let mut order: Vec<String> = Vec::new();
    if let Some(preferred) = preferred {
        order.push(preferred);
    }
    for family in UI_FAMILIES {
        order.push(family.to_string());
    }

    let mut chosen: Option<PathBuf> = None;
    for family in &order {
        if let Some(path) = pick_family(family, &files) {
            chosen = Some(path);
            break;
        }
    }
    let chosen = chosen.or_else(|| pick_any(&files));

    // Con l'interfaccia si prova anche la spaziatura fissa, ma solo se è una
    // famiglia diversa: un monospace come font principale si legge male.
    let mut mono_path: Option<PathBuf> = None;
    for family in MONO_FAMILIES {
        if let Some(path) = pick_family(family, &files) {
            mono_path = Some(path);
            break;
        }
    }
    if mono_path.is_some() && mono_path == chosen {
        mono_path = None;
    }

    let mut plan = FontPlan {
        scanned,
        ..Default::default()
    };
    if let Some(path) = chosen {
        if let Some((family, bytes)) = read_font(&path, FONT_UI_BUDGET) {
            plan.ui = Some(FontFace {
                family,
                file: file_name(&path),
                path: path.clone(),
                bytes,
            });
        }
    }
    if let Some(path) = mono_path {
        if let Some((family, bytes)) = read_font(&path, FONT_MONO_BUDGET) {
            plan.mono = Some(FontFace {
                family,
                file: file_name(&path),
                path,
                bytes,
            });
        }
    }

    // Riserve: prima i file con il nome esatto, poi il resto della lista.
    let mut budget: usize = FONT_FALLBACK_BUDGET;
    let mut used: Vec<PathBuf> = Vec::new();
    if let Some(face) = plan.ui.as_ref() {
        used.push(face.path.clone());
    }
    if let Some(face) = plan.mono.as_ref() {
        used.push(face.path.clone());
    }
    for wanted in FALLBACK_FILES {
        if budget < 1024 * 1024 {
            break;
        }
        let Some(path) = files
            .iter()
            .find(|path| path.file_name().map(|name| name == wanted).unwrap_or(false))
        else {
            continue;
        };
        if used.iter().any(|existing| existing == path) {
            continue;
        }
        let Some((family, bytes)) = read_font(path, budget) else {
            continue;
        };
        budget = budget.saturating_sub(bytes.len());
        used.push(path.clone());
        plan.fallback.push(FontFace {
            family,
            file: file_name(path),
            path: path.clone(),
            bytes,
        });
    }
    plan
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn font_magic_ok(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    let magic = &bytes[0..4];
    magic == [0x00, 0x01, 0x00, 0x00]
        || magic == b"OTTO"
        || magic == b"true"
        || magic == b"ttcf"
        || magic == b"typ1"
}

/// Impedisce due istanze contemporanee sullo stesso archivio.
///
/// Il socket resta aperto per tutta la vita del processo: se il bind riesce
/// siamo i soli, altrimenti c'è già un Whats-Open in esecuzione.
pub struct InstanceLock {
    path: PathBuf,
    /// Diventa `true` quando una seconda istanza chiede di mostrare la finestra.
    focus: Arc<AtomicBool>,
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl InstanceLock {
    /// Bandierina da controllare a ogni fotogramma: se è vera, la finestra
    /// esistente si porta in primo piano.
    pub fn focus_flag(&self) -> Arc<AtomicBool> {
        self.focus.clone()
    }
}

pub fn single_instance(paths: &crate::util::Paths) -> Result<InstanceLock, String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| paths.data.clone());
    let path = dir.join("whats-open.sock");

    // un socket rimasto da un crash precedente non deve bloccare l'avvio
    if path.exists() {
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(mut stream) => {
                // chiedi alla finestra già aperta di venire davanti
                use std::io::Write as _;
                let _ = stream.write_all(b"focus\n");
                return Err(
                    "Whats-Open è già in esecuzione: porto la sua finestra in primo piano."
                        .to_string(),
                );
            }
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let listener = std::os::unix::net::UnixListener::bind(&path)
        .map_err(|error| format!("blocco istanza: {error}"))?;
    let focus = Arc::new(AtomicBool::new(false));
    let flag = focus.clone();
    // Il thread vive quanto il processo: tiene il socket e raccoglie le richieste.
    let _ = std::thread::Builder::new()
        .name("whats-open-ipc".into())
        .spawn(move || {
            for stream in listener.incoming() {
                if stream.is_ok() {
                    flag.store(true, Ordering::Relaxed);
                }
            }
        });

    Ok(InstanceLock { path, focus })
}

/// Famiglia di distribuzione, per suggerire il comando giusto.
fn distro_family() -> &'static str {
    let text = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let field = |key: &str| -> String {
        text.lines()
            .find_map(|line| line.strip_prefix(key))
            .map(|value| value.trim_matches('"').to_lowercase())
            .unwrap_or_default()
    };
    let all = format!("{} {}", field("ID="), field("ID_LIKE="));
    if all.contains("debian") || all.contains("ubuntu") || all.contains("mint") {
        "debian"
    } else if all.contains("fedora")
        || all.contains("rhel")
        || all.contains("centos")
        || all.contains("rocky")
        || all.contains("alma")
    {
        "fedora"
    } else if all.contains("arch") || all.contains("manjaro") {
        "arch"
    } else if all.contains("suse") {
        "suse"
    } else if all.contains("alpine") {
        "alpine"
    } else {
        "altro"
    }
}

/// Comando di installazione adatto alla distribuzione in uso.
fn install_hint(deb: &str, rpm: &str, pac: &str, zyp: &str, apk: &str) -> String {
    match distro_family() {
        "debian" => format!("sudo apt install {deb}"),
        "fedora" => format!("sudo dnf install {rpm}"),
        "arch" => format!("sudo pacman -S {pac}"),
        "suse" => format!("sudo zypper install {zyp}"),
        "alpine" => format!("sudo apk add {apk}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    fn picked(family: &str, names: &[&str]) -> Option<String> {
        pick_family(family, &files(names)).map(|path| file_name(&path))
    }

    #[test]
    fn il_nome_del_desktop_diventa_una_famiglia() {
        assert_eq!(family_of("Cantarell 11"), "Cantarell");
        assert_eq!(family_of("Noto Sans,10,-1,5,50,0,0,0,0,0"), "Noto Sans");
        assert_eq!(family_of("  Ubuntu  12  "), "Ubuntu");
        // "Sans" è una categoria, non una famiglia: meglio le preferite.
        assert_eq!(family_of("Sans 10"), "");
        assert_eq!(family_of("monospace"), "");
    }

    #[test]
    fn la_famiglia_si_confronta_senza_separatori() {
        assert_eq!(normalize_family("DejaVu Sans"), "dejavusans");
        assert_eq!(normalize_family("Noto Sans – Bold"), "notosansbold");
    }

    #[test]
    fn si_sceglie_il_tondo_e_non_il_grassetto() {
        let names = [
            "/f/Inter-Bold.ttf",
            "/f/Inter-Italic.ttf",
            "/f/Inter-Regular.ttf",
        ];
        assert_eq!(picked("Inter", &names).as_deref(), Some("Inter-Regular.ttf"));
    }

    #[test]
    fn una_famiglia_che_comincia_uguale_non_basta() {
        // "Noto Sans" non deve prendere l'arabo solo perché comincia così.
        let names = [
            "/f/NotoSansArabic-Regular.ttf",
            "/f/NotoSansCJK-Regular.ttc",
        ];
        assert_eq!(picked("Noto Sans", &names), None);
        // Se invece c'è il file giusto, si prende quello.
        let names = ["/f/NotoSansArabic-Regular.ttf", "/f/NotoSans-Regular.ttf"];
        assert_eq!(picked("Noto Sans", &names).as_deref(), Some("NotoSans-Regular.ttf"));
    }

    #[test]
    fn una_famiglia_qualsiasi_va_bene_purche_non_sia_di_simboli() {
        let names = ["/f/NotoEmoji-Regular.ttf", "/f/AcmeSans-Regular.ttf"];
        assert_eq!(pick_any(&files(&names)).map(|p| file_name(&p)).as_deref(), Some("AcmeSans-Regular.ttf"));
        assert_eq!(pick_any(&files(&["/f/NotoEmoji-Regular.ttf"])), None);
    }

    #[test]
    fn il_mono_rimane_il_font_a_spaziatura_fissa() {
        let names = ["/f/DejaVuSansMono.ttf", "/f/DejaVuSans.ttf"];
        assert_eq!(
            picked("DejaVu Sans Mono", &names).as_deref(),
            Some("DejaVuSansMono.ttf")
        );
    }
}
