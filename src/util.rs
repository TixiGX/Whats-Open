//! Piccole utilità condivise: percorsi XDG, orari, formattazione, log su file.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{Datelike, Local, TimeZone, Timelike, Weekday};

/// Percorsi dell'applicazione, secondo la XDG Base Directory Specification.
#[derive(Clone, Debug)]
pub struct Paths {
    pub data: PathBuf,
    pub cache: PathBuf,
    /// Messaggi e chat (schema nostro).
    pub db: PathBuf,
    /// Credenziali e chiavi della sessione (schema della libreria).
    pub session: PathBuf,
    pub media: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

impl Paths {
    pub fn resolve() -> Self {
        let base_data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local").join("share"));
        let base_cache = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".cache"));

        let data = base_data.join("whats-open");
        let cache = base_cache.join("whats-open");
        let media = cache.join("media");
        let db = data.join("messages.db");
        let session = data.join("session.db");

        Self {
            data,
            cache,
            db,
            session,
            media,
        }
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.data)?;
        std::fs::create_dir_all(&self.cache)?;
        std::fs::create_dir_all(&self.media)?;
        Ok(())
    }

    pub fn log_file(&self) -> PathBuf {
        self.cache.join("whats-open.log")
    }

    /// File in cache per un allegato già scaricato.
    pub fn cached_media(&self, chat: &str, message_id: &str, ext: &str) -> PathBuf {
        let safe_chat: String = chat
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let safe_id: String = message_id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();

        let dir = self.media.join(safe_chat);
        let _ = std::fs::create_dir_all(&dir);
        dir.join(format!("{safe_id}.{ext}"))
    }
}

pub fn now_ts() -> i64 {
    Local::now().timestamp()
}

pub fn fmt_clock(ts: i64) -> String {
    match chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0) {
        Some(dt) => {
            let local = dt.with_timezone(&Local);
            format!("{:02}:{:02}", local.hour(), local.minute())
        }
        None => String::new(),
    }
}

pub fn fmt_chat_time(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    let Some(dt) = chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0) else {
        return String::new();
    };
    let local = dt.with_timezone(&Local);
    let today = Local::now().date_naive();
    let day = local.date_naive();

    if day == today {
        fmt_clock(ts)
    } else if Some(day) == today.pred_opt() {
        "ieri".to_string()
    } else if (today - day).num_days() < 7 {
        match local.weekday() {
            Weekday::Mon => "lun",
            Weekday::Tue => "mar",
            Weekday::Wed => "mer",
            Weekday::Thu => "gio",
            Weekday::Fri => "ven",
            Weekday::Sat => "sab",
            Weekday::Sun => "dom",
        }
        .to_string()
    } else if day.year() == today.year() {
        format!("{:02}/{:02}", local.day(), local.month())
    } else {
        format!("{:02}/{:02}/{:02}", local.day(), local.month(), local.year() % 100)
    }
}

pub fn fmt_long_time(ts: i64) -> String {
    match chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0) {
        Some(dt) => {
            let local = dt.with_timezone(&Local);
            format!(
                "{:02}/{:02}/{:04} {:02}:{:02}",
                local.day(),
                local.month(),
                local.year(),
                local.hour(),
                local.minute()
            )
        }
        None => String::new(),
    }
}

pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB).replace('.', ",")
    } else if b >= MB {
        format!("{:.1} MB", b / MB).replace('.', ",")
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

pub fn human_duration(seconds: u32) -> String {
    let m = seconds / 60;
    let s = seconds % 60;
    if m >= 60 {
        format!("{}:{:02}:{:02}", m / 60, m % 60, s)
    } else {
        format!("{m}:{s:02}")
    }
}

/// Nome leggibile del contatto a partire dal JID.
pub fn jid_user(jid: &str) -> String {
    jid.split('@').next().unwrap_or(jid).split(':').next().unwrap_or(jid).to_string()
}

pub fn jid_is_group(jid: &str) -> bool {
    jid.ends_with("@g.us")
}

pub fn jid_is_newsletter(jid: &str) -> bool {
    jid.ends_with("@newsletter")
}

pub fn jid_is_status(jid: &str) -> bool {
    jid.starts_with("status@")
}

/// Numero di telefono presentabile (senza spazi, il numero è già E.164).
pub fn phone_display(user: &str) -> String {
    if !user.is_empty() && user.chars().all(|c| c.is_ascii_digit()) {
        format!("+{user}")
    } else {
        user.to_string()
    }
}

/// Nome da mostrare: nome salvato, push name, oppure numero/ID.
pub fn display_name(name: &str, jid: &str) -> String {
    if !name.trim().is_empty() {
        return name.trim().to_string();
    }
    let user = jid_user(jid);
    if user.chars().all(|c| c.is_ascii_digit()) && !user.is_empty() {
        phone_display(&user)
    } else {
        "Contatto".to_string()
    }
}

pub fn initials(name: &str) -> String {
    let mut out = String::new();
    for word in name.split_whitespace() {
        if let Some(ch) = word.chars().next() {
            if out.chars().count() < 2 {
                out.extend(ch.to_uppercase());
            }
        }
    }
    if out.is_empty() {
        out.push('?');
    }
    out
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn open_external(path: &Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

pub fn timestamp_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// --------------------------------------------------------------------------
// Log minimale su file (niente crate extra): utile per capire un guasto.
// --------------------------------------------------------------------------

struct FileLogger {
    file: Mutex<Option<std::fs::File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} [{:<5}] {}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            record.level(),
            record.args()
        );
        eprintln!("{}", line.trim_end());
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.write_all(line.as_bytes());
                let _ = file.flush();
            }
        }
    }

    fn flush(&self) {}
}

pub fn init_logging(path: &Path) {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok();
    let logger = FileLogger {
        file: Mutex::new(file),
    };
    let _ = log::set_boxed_logger(Box::new(logger));
    log::set_max_level(log::LevelFilter::Info);
}

#[allow(dead_code)]
fn _assert_local_tz() -> Option<chrono::DateTime<Local>> {
    Local.timestamp_opt(0, 0).single()
}
