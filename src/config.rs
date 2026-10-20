//! Preferenze dell'utente, salvate in `config.json` accanto all'archivio.
//!
//! Poche voci, tutte pensate per adattarsi al desktop: chi non ha ffmpeg (o
//! preferisce il proprio player) può affidare audio e video al sistema.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Apri audio e video con il programma predefinito invece che nei lettori
    /// interni.
    pub system_player: bool,
    /// Carica i font di sistema come riserva (emoji, CJK, arabo, …).
    pub system_fonts: bool,
    /// Chiudere la finestra scollega la sessione in modo pulito.
    pub graceful_exit: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            system_player: false,
            system_fonts: true,
            graceful_exit: true,
        }
    }
}

impl Config {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("config.json")
    }

    pub fn load(data_dir: &Path) -> Self {
        let path = Self::path(data_dir);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self, data_dir: &Path) {
        let path = Self::path(data_dir);
        if let Ok(text) = serde_json::to_string_pretty(self) {
            if std::fs::write(&path, text).is_err() {
                log::warn!("impossibile salvare {}", path.display());
            }
        }
    }
}
