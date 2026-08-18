//! Typed application & library settings (spec §51).
//!
//! Settings are persisted as a single JSON document in the `settings` table
//! under the key `app`. Keeping them typed here (rather than scattered rows)
//! means the frontend gets one coherent object and defaults are centralized.

use crate::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// How NEXORA treats imported files (spec §47).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageMode {
    /// Copy files into the NEXORA library folder.
    Managed,
    /// Store original paths without copying.
    Referenced,
}

/// UI theme preference (spec §51 Appearance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

/// The default renderer used when applying a material (spec §51 Renderer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Renderer {
    GenericPbr,
    VRay,
    Arnold,
}

/// Library-related settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySettings {
    /// Root of the managed library, e.g. `D:\NEXORA_LIBRARY`. `None` until the
    /// user completes first-run configuration.
    pub location: Option<String>,
    pub storage_mode: StorageMode,
    pub auto_scan: bool,
    /// Minutes between background scans when `auto_scan` is on.
    pub scan_frequency_minutes: u32,
}

/// Import behavior toggles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSettings {
    pub auto_detect_maps: bool,
    pub auto_generate_preview: bool,
    pub auto_tag: bool,
    pub auto_group_texture_sets: bool,
    /// Copy files on import (mirrors managed mode for one-off imports).
    pub copy_files: bool,
}

/// Appearance settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceSettings {
    pub theme: ThemeMode,
    /// Asset grid card size in px (thumbnail edge).
    pub grid_size: u32,
    /// Preview render quality, 1 (fast) .. 3 (high).
    pub preview_quality: u8,
}

/// Update channel/behavior (spec §51 Updates, §52).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSettings {
    pub automatic_updates: bool,
    pub check_on_startup: bool,
    /// "stable" or "beta".
    pub channel: String,
}

/// The full settings document surfaced to the frontend as one object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub library: LibrarySettings,
    pub import: ImportSettings,
    pub appearance: AppearanceSettings,
    pub default_renderer: Renderer,
    pub updates: UpdateSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            library: LibrarySettings {
                location: None,
                storage_mode: StorageMode::Managed,
                auto_scan: false,
                scan_frequency_minutes: 30,
            },
            import: ImportSettings {
                auto_detect_maps: true,
                auto_generate_preview: true,
                auto_tag: true,
                auto_group_texture_sets: true,
                copy_files: true,
            },
            appearance: AppearanceSettings {
                theme: ThemeMode::Dark,
                grid_size: 200,
                preview_quality: 2,
            },
            default_renderer: Renderer::GenericPbr,
            updates: UpdateSettings {
                automatic_updates: true,
                check_on_startup: true,
                channel: "stable".into(),
            },
        }
    }
}

const SETTINGS_KEY: &str = "app";

impl AppSettings {
    /// Load settings from the DB, falling back to defaults (and persisting them)
    /// on first run or if the stored blob is unreadable.
    pub fn load(conn: &Connection) -> Result<AppSettings> {
        let raw: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [SETTINGS_KEY],
                |row| row.get(0),
            )
            .ok();

        match raw.and_then(|s| serde_json::from_str(&s).ok()) {
            Some(settings) => Ok(settings),
            None => {
                let defaults = AppSettings::default();
                defaults.save(conn)?;
                Ok(defaults)
            }
        }
    }

    /// Persist settings as a single JSON row.
    pub fn save(&self, conn: &Connection) -> Result<()> {
        let json = serde_json::to_string(self)?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![SETTINGS_KEY, json],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn defaults_roundtrip_through_db() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();

        // First load writes defaults.
        let loaded = AppSettings::load(conn).unwrap();
        assert_eq!(loaded.appearance.theme, ThemeMode::Dark);
        assert!(loaded.library.location.is_none());

        // Mutate + save.
        let mut s = loaded;
        s.library.location = Some("D:\\NEXORA_LIBRARY".into());
        s.library.storage_mode = StorageMode::Referenced;
        s.save(conn).unwrap();

        // Reload reflects the change.
        let again = AppSettings::load(conn).unwrap();
        assert_eq!(again.library.location.as_deref(), Some("D:\\NEXORA_LIBRARY"));
        assert_eq!(again.library.storage_mode, StorageMode::Referenced);
    }
}
