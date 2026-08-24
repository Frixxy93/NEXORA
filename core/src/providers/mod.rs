//! Online free-texture providers — the "Discover" feature.
//!
//! Pulls CC0 (public-domain) PBR textures from libraries that offer official
//! APIs and permit downloading. Two sources are supported: Poly Haven (one file
//! per map) and ambientCG (one ZIP bundle per material). Everything here imports
//! through the normal material pipeline, so downloaded assets behave exactly
//! like locally imported ones.

pub mod ambientcg;
pub mod polyhaven;

use crate::db::Database;
use crate::settings::AppSettings;
use crate::texture::ImportOptions;
use crate::{CoreError, Result};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

pub const SOURCE_POLYHAVEN: &str = "polyhaven";
pub const SOURCE_AMBIENTCG: &str = "ambientcg";

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
    /// Pull from Poly Haven.
    pub source_polyhaven: bool,
    /// Pull from ambientCG.
    pub source_ambientcg: bool,
}

/// One unit of work: a single asset to download and import.
enum Job {
    /// Poly Haven asset id (its file plan is fetched per-asset).
    PolyHaven { id: String },
    /// ambientCG asset id + its ZIP bundle URL (known from the listing).
    AmbientCg { id: String, url: String },
}

impl Job {
    fn source(&self) -> &'static str {
        match self {
            Job::PolyHaven { .. } => SOURCE_POLYHAVEN,
            Job::AmbientCg { .. } => SOURCE_AMBIENTCG,
        }
    }
    fn id(&self) -> &str {
        match self {
            Job::PolyHaven { id } => id,
            Job::AmbientCg { id, .. } => id,
        }
    }
}

fn lock(db: &Mutex<Database>) -> MutexGuard<'_, Database> {
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

/// How many assets from `source` are already imported (for status display).
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

/// Total assets imported across all Discover sources (for status display).
pub fn synced_count_total(conn: &rusqlite::Connection) -> u64 {
    let _ = ensure_table(conn);
    conn.query_row("SELECT COUNT(*) FROM discover_synced", [], |r| {
        r.get::<_, i64>(0)
    })
    .map(|n| n as u64)
    .unwrap_or(0)
}

/// Build the full work list from the enabled sources. Network calls here are the
/// catalog-listing requests (one per source); per-asset downloads happen later.
fn collect_jobs(opts: &SyncOptions) -> Result<Vec<Job>> {
    let mut jobs: Vec<Job> = Vec::new();

    if opts.source_polyhaven {
        let agent = polyhaven::agent();
        match polyhaven::list_texture_ids(&agent) {
            Ok(ids) => jobs.extend(ids.into_iter().map(|id| Job::PolyHaven { id })),
            Err(e) => {
                // If the only source fails to list, that's fatal; otherwise carry on.
                if !opts.source_ambientcg {
                    return Err(e);
                }
            }
        }
    }

    if opts.source_ambientcg {
        let agent = polyhaven::agent(); // same UA/timeout profile is fine
        let res_prefix = opts.resolution.to_uppercase(); // "1k" -> "1K"
        match ambientcg::list_material_bundles(&agent, &res_prefix) {
            Ok(bundles) => jobs.extend(bundles.into_iter().map(|b| Job::AmbientCg {
                id: b.asset_id,
                url: b.url,
            })),
            Err(e) => {
                if jobs.is_empty() {
                    return Err(e);
                }
            }
        }
    }

    Ok(jobs)
}

/// Run the Discover auto-sync across all enabled sources: download every catalog
/// texture (skipping ones already imported) at the chosen resolution and import
/// each as a material.
///
/// The DB mutex is locked only for brief operations — never while downloading —
/// so the app stays responsive. `stop` is checked between assets; `on_progress`
/// is invoked after each. Individual asset failures are counted and skipped, not
/// fatal.
pub fn run_sync(
    db: &Mutex<Database>,
    opts: &SyncOptions,
    stop: &AtomicBool,
    on_progress: &(dyn Fn(&SyncProgress) + Sync),
) -> Result<SyncProgress> {
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

    if !opts.source_polyhaven && !opts.source_ambientcg {
        return Err(CoreError::Config(
            "Enable at least one source (Poly Haven or ambientCG) before syncing.".into(),
        ));
    }

    let import_opts = ImportOptions {
        managed: true,
        library_root: Some(library_root),
        thumbnail_dir: opts.thumbnail_dir.clone(),
        generate_preview: opts.generate_preview,
        detect_maps: true,
    };

    let ph_agent = polyhaven::agent();
    let acg_agent = polyhaven::agent();

    let jobs = collect_jobs(opts)?;

    let tmp = std::env::temp_dir().join("nexora-discover");
    let _ = std::fs::create_dir_all(&tmp);

    let mut p = SyncProgress {
        running: true,
        total: jobs.len(),
        ..Default::default()
    };
    on_progress(&p);

    for job in &jobs {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let source = job.source();
        let id = job.id().to_string();
        p.current = format!("{source}: {id}");

        // Skip assets already in the library.
        if already_synced(lock(db).conn(), source, &id) {
            p.skipped += 1;
            p.done += 1;
            on_progress(&p);
            continue;
        }

        // Download (unlocked) then import (brief lock). Temp dir is
        // source-prefixed so ids can't collide across libraries.
        let dir = tmp.join(format!("{source}_{id}"));
        let outcome = (|| -> Result<u64> {
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir)?;
            let bytes = match job {
                Job::PolyHaven { id } => {
                    let plan = polyhaven::download_plan(&ph_agent, id, &opts.resolution)?;
                    if plan.is_empty() {
                        return Err(CoreError::Provider(format!("{id}: no downloadable maps")));
                    }
                    let mut bytes = 0u64;
                    for f in &plan {
                        let data = polyhaven::download_bytes(&ph_agent, &f.url)?;
                        bytes += data.len() as u64;
                        std::fs::write(dir.join(&f.filename), &data)?;
                    }
                    bytes
                }
                Job::AmbientCg { url, .. } => {
                    ambientcg::download_and_extract(&acg_agent, url, &dir)?
                }
            };
            {
                let guard = lock(db);
                crate::material::import_material_folder(guard.conn(), &dir, &import_opts, &registry)?;
                mark_synced(guard.conn(), source, &id)?;
            }
            Ok(bytes)
        })();
        let _ = std::fs::remove_dir_all(&dir);

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
