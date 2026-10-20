//! Adattamento alla distribuzione: rilevamento dell'ambiente, apertura di file
//! con il programma di sistema, font di riserva, istanza singola, diagnosi.

use std::path::{Path, PathBuf};

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
        let candidate = dir.join(name);
        if candidate.is_file() {
            return true;
        }
    }
    false
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

    for (label, name) in [
        ("ffmpeg", "ffmpeg"),
        ("ffprobe", "ffprobe"),
        ("xdg-open", "xdg-open"),
        ("player", "ffplay"),
        ("portal", "xdg-desktop-portal"),
    ] {
        out.push_str(&format!(
            "{:<14}: {}\n",
            label,
            if command_exists(name) { "sì" } else { "no" }
        ));
    }

    if let Some(device) = default_audio_device() {
        out.push_str(&format!("audio         : {device}\n"));
    } else {
        out.push_str("audio         : nessun dispositivo di uscita\n");
    }

    let fonts = system_font_candidates();
    out.push_str(&format!(
        "font di sistema: {}\n",
        if fonts.is_empty() {
            "nessuno (solo font incorporati)".to_string()
        } else {
            folders_list(&fonts)
        }
    ));

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

fn folders_list(paths: &[PathBuf]) -> String {
    let names: Vec<String> = paths
        .iter()
        .filter_map(|path| path.file_name().map(|name| name.to_string_lossy().to_string()))
        .collect();
    names.join(", ")
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
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next()?.trim().to_string();
    if first.is_empty() {
        None
    } else {
        Some(first)
    }
}

/// File di font di sistema che vale la pena caricare come riserva.
///
/// Servono a coprire scritture che i font incorporati non hanno: emoji,
/// arabo, ebraico, devanagari, thai e (se il sistema li ha) ideogrammi CJK.
pub fn system_font_candidates() -> Vec<PathBuf> {
    if std::env::var_os("WHATS_OPEN_NO_SYSTEM_FONTS").is_some() {
        return Vec::new();
    }

    let roots = [
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/run/host/fonts",
        "/run/host/usr/share/fonts",
    ];
    let home_fonts = std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".local/share/fonts"))
        .unwrap_or_default();

    let wanted = [
        "NotoEmoji-Regular.ttf",
        "DejaVuSans.ttf",
        "NotoSansSymbols2-Regular.ttf",
        "NotoSansArabic-Regular.ttf",
        "NotoSansHebrew-Regular.ttf",
        "NotoSansDevanagari-Regular.ttf",
        "NotoSansThai-Regular.ttf",
        "NotoSans-Regular.ttf",
        "LiberationSans-Regular.ttf",
        "FreeSans.ttf",
        "NotoSansCJK-Regular.ttc",
        "NotoSansCJKsc-Regular.otf",
        "NotoSansCJKjp-Regular.otf",
    ];

    let mut found: Vec<PathBuf> = Vec::new();
    let mut scan = |root: &Path| {
        for name in wanted {
            if found.iter().any(|path| {
                path.file_name()
                    .map(|existing| existing == name)
                    .unwrap_or(false)
            }) {
                continue;
            }
            if let Some(path) = search_font(root, name) {
                found.push(path);
            }
        }
    };

    for root in roots {
        scan(Path::new(root));
    }
    if home_fonts.exists() {
        scan(&home_fonts);
    }

    // niente file enormi: la memoria resta sotto controllo
    found.retain(|path| {
        std::fs::metadata(path)
            .map(|meta| meta.len() < 26 * 1024 * 1024)
            .unwrap_or(false)
    });
    found.truncate(8);
    found
}

fn search_font(root: &Path, file: &str) -> Option<PathBuf> {
    // due livelli di cartelle bastano per tutte le distribuzioni comuni
    let direct = root.join(file);
    if direct.is_file() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let candidate = path.join(file);
        if candidate.is_file() {
            return Some(candidate);
        }
        if let Ok(children) = std::fs::read_dir(&path) {
            for child in children.flatten() {
                let child_path = child.path();
                if !child_path.is_dir() {
                    continue;
                }
                let deeper = child_path.join(file);
                if deeper.is_file() {
                    return Some(deeper);
                }
            }
        }
    }
    None
}

/// Carica i font di sistema validi come riserva per le scritture non latine.
///
/// Una libreria di testi non valida farebbe andare in panico egui al primo
/// fotogramma: qui si controlla l'intestazione del file prima di passarlo.
pub fn load_system_fonts() -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for path in system_font_candidates() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if bytes.len() < 1024 || !font_magic_ok(&bytes) {
            continue;
        }
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_string())
            .unwrap_or_else(|| "system".to_string());
        out.push((format!("sys:{name}"), bytes));
    }
    out
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
    _listener: std::os::unix::net::UnixListener,
    path: PathBuf,
}

impl Drop for InstanceLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
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
            Ok(_) => {
                return Err(
                    "Whats-Open è già in esecuzione: usa la finestra aperta.".to_string(),
                )
            }
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    std::os::unix::net::UnixListener::bind(&path)
        .map(|listener| InstanceLock {
            _listener: listener,
            path,
        })
        .map_err(|error| format!("blocco istanza: {error}"))
}
