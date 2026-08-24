//! Online free-texture providers — the "Discover" feature.
//!
//! Pulls CC0 (public-domain) PBR textures from libraries that offer official
//! APIs and permit downloading. Poly Haven is the first source. Everything here
//! imports through the normal material pipeline, so downloaded assets behave
//! exactly like locally imported ones.

pub mod polyhaven;

use crate::db::Database;
use crate::settings::AppSettings;
use crate::texture::ImportOptions;
use crate::{CoreError, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Progress snapshot for a running or just-finished sync, surfaced to the UI.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SyncProgress {
    pub running: bool,
    pub total: usize,
    /// imported + skipped + failed.
    pub done: usize,
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub current: String,
    pub bytes: u64,
    pub finished: bool,
    pub error: Option<String>,
}

/// Inputs for a sync run.
pub struct SyncOptions {
    /// "1k" | "2k" | "4k".
    pub resolution: String,
    pub thumbnail_dir: PathBuf,
    pub generate_preview: bool,
}

fn lock<'a>(db: &'a Mutex<Database>) -> MutexGuard<'a, Database> {
    db.lock().unwrap_or_else(|p| p.into_inner())
}

fn ensure_table(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS discover_synced (
            source      TEXT NOT NULL,
            asset_id    TEXT NOT NULL,
            imported_at INTEGER NOT NULL,
            PRIMARY KEY (source, asset_id)
         )",
        [],
    )?;
    Ok(())
}

fn already_synced(conn: &rusqlite::Connection, source: &str, id: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM discover_synced WHERE source = ?1 AND asset_id = ?2",
        rusqlite::params![source, id],
        |_| Ok(()),
    )
    .is_ok()
}

fn mark_synced(conn: &rusqlite::Connection, source: &str, id: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO discover_synced (source, asset_id, imported_at)
         VALUES (?1, ?2, strftime('%s','now'))",
        rusqlite::params![source, id],
    )?;
    Ok(())
}

/// How many Poly Haven assets are already imported (for status display).
pub fn synced_count(conn: &rusqlite::Connection, source: &str) -> u64 {
    let _ = ensure_table(conn);
    conn.query_row(
        "SELECT COUNT(*) FROM discover_synced WHERE source = ?1",
        rusqlite::params![source],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n as u64)
    .unwrap_or(0)
}

/// Run the Poly Haven auto-sync: download every catalog texture (skipping ones
/// already imported) at the chosen resolution and import each as a material.
///
/// The DB mutex is locked only for brief operations — never while downloading —
/// so the app stays responsive. `stop` is checked between assets; `on_progress`
/// is invoked after each. Individual asset failures are counted and skipped, not
/// fatal.
pub fn run_polyhaven_sync(
    db: &Mutex<Database>,
    opts: &SyncOptions,
    stop: &AtomicBool,
    on_progress: &(dyn Fn(&SyncProgress) + Sync),
) -> Result<SyncProgress> {
    const SOURCE: &str = "polyhaven";
    let registry = crate::maptypes::MapTypeRegistry::builtin();

    // Managed import needs a library root; resolve it up front.
    let library_root = {
        let guard = lock(db);
        ensure_table(guard.conn())?;
        let settings = AppSettings::load(guard.conn())?;
        match settings.library.location {
            Some(p) => PathBuf::from(p),
            None => {
                return Err(CoreError::Config(
                    "Set a library location in Settings before syncing free textures.".into(),
                ))
            }
        }
    };

    let import_opts = ImportOptions {
        managed: true,
        library_root: Some(library_root),
        thumbnail_dir: opts.thumbnail_dir.clone(),
        generate_preview: opts.generate_preview,
        detect_maps: true,
    };

    let agent = polyhaven::agent();
    let ids = polyhaven::list_texture_ids(&agent)?;

    let tmp = std::env::temp_dir().join("nexora-discover");
    let _ = std::fs::create_dir_all(&tmp);

    let mut p = SyncProgress {
        running: true,
        total: ids.len(),
        ..Default::default()
    };
    on_progress(&p);

    for id in ids {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        p.current = id.clone();

        // Skip assets already in the library.
        if already_synced(lock(db).conn(), SOURCE, &id) {
            p.skipped += 1;
            p.done += 1;
            on_progress(&p);
            continue;
        }

        // Download (unlocked) then import (brief lock).
        let outcome = (|| -> Result<u64> {
            let plan = polyhaven::download_plan(&agent, &id, &opts.resolution)?;
            if plan.is_empty() {
                return Err(CoreError::Provider(format!("{id}: no downloadable maps")));
            }
            let dir = tmp.join(&id);
            std::fs::create_dir_all(&dir)?;
            let mut bytes = 0u64;
            for f in &plan {
                let data = polyhaven::download_bytes(&agent, &f.url)?;
                bytes += data.len() as u64;
                std::fs::write(dir.join(&f.filename), &data)?;
            }
            {
                let guard = lock(db);
                crate::material::import_material_folder(guard.conn(), &dir, &import_opts, &registry)?;
                mark_synced(guard.conn(), SOURCE, &id)?;
            }
            let _ = std::fs::remove_dir_all(&dir);
            Ok(bytes)
        })();

        match outcome {
            Ok(b) => {
                p.imported += 1;
                p.bytes += b;
            }
            Err(_) => {
                p.failed += 1;
            }
        }
        p.done += 1;
        on_progress(&p);
    }

    p.running = false;
    p.finished = true;
    p.current.clear();
    on_progress(&p);
    Ok(p)
}
