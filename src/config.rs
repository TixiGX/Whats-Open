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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn il_file_mancante_usa_i_valori_predefiniti() {
        let dir = std::env::temp_dir().join(format!("whats-open-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let config = Config::load(&dir);
        assert!(!config.system_player);
        assert!(config.system_fonts);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn salvataggio_e_rilettura() {
        let dir = std::env::temp_dir().join(format!("whats-open-cfg2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let config = Config {
            system_player: true,
            ..Default::default()
        };
        config.save(&dir);
        assert!(Config::load(&dir).system_player);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn configurazione_corrotta_non_blocca_l_avvio() {
        let dir = std::env::temp_dir().join(format!("whats-open-cfg3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(Config::path(&dir), "{ non json").unwrap();
        let config = Config::load(&dir);
        assert!(!config.system_player);
        std::fs::remove_dir_all(&dir).ok();
    }
}
