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
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

pub const SOURCE_POLYHAVEN: &str = "polyhaven";
pub const SOURCE_AMBIENTCG: &str = "ambientcg";

/// Concurrent download workers. Downloads are network-bound; a handful of
/// parallel fetches is a big speedup while staying polite to the free CC0 hosts.
const CONCURRENCY: usize = 5;

/// How many times to attempt a single asset before giving up (transient network
/// errors are common on a long bulk sync).
const MAX_ATTEMPTS: u32 = 3;

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
    /// A fatal error that stopped the whole run (e.g. no library configured).
    pub error: Option<String>,
    /// The most recent per-asset failure (non-fatal; the run continues).
    pub last_error: Option<String>,
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

/// The set of asset ids already imported from `source` (to flag them in Browse).
pub fn synced_ids(conn: &rusqlite::Connection, source: &str) -> std::collections::HashSet<String> {
    let _ = ensure_table(conn);
    let mut set = std::collections::HashSet::new();
    if let Ok(mut stmt) =
        conn.prepare("SELECT asset_id FROM discover_synced WHERE source = ?1")
    {
        if let Ok(rows) = stmt.query_map([source], |r| r.get::<_, String>(0)) {
            for r in rows.flatten() {
                set.insert(r);
            }
        }
    }
    set
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

/// Lock the shared progress, tolerating a poisoned mutex.
fn plock(m: &Mutex<SyncProgress>) -> MutexGuard<'_, SyncProgress> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A structural failure that retrying can't fix (the asset simply has nothing to
/// download), versus a transient network error worth retrying.
fn is_permanent(msg: &str) -> bool {
    msg.contains("no downloadable maps") || msg.contains("no map images")
}

/// Run `f` up to [`MAX_ATTEMPTS`] times, backing off between tries. Stops early
/// on a permanent error, when asked to stop, or once attempts are exhausted.
fn with_retry<F: FnMut() -> Result<u64>>(mut f: F, stop: &AtomicBool) -> Result<u64> {
    let mut attempt = 0u32;
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                attempt += 1;
                if attempt >= MAX_ATTEMPTS
                    || stop.load(Ordering::Relaxed)
                    || is_permanent(&e.to_string())
                {
                    return Err(e);
                }
                // Exponential backoff: 400ms, then 800ms.
                std::thread::sleep(std::time::Duration::from_millis(400u64 << (attempt - 1)));
            }
        }
    }
}

/// Download one asset's files into `dir` and import it as a material (brief DB
/// lock), returning the bytes downloaded. Called under [`with_retry`].
#[allow(clippy::too_many_arguments)]
fn download_and_import(
    job: &Job,
    db: &Mutex<Database>,
    dir: &std::path::Path,
    ph_agent: &ureq::Agent,
    acg_agent: &ureq::Agent,
    resolution: &str,
    import_opts: &ImportOptions,
    registry: &crate::maptypes::MapTypeRegistry,
) -> Result<u64> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    let bytes = match job {
        Job::PolyHaven { id } => {
            let plan = polyhaven::download_plan(ph_agent, id, resolution)?;
            if plan.is_empty() {
                return Err(CoreError::Provider(format!("{id}: no downloadable maps")));
            }
            let mut bytes = 0u64;
            for f in &plan {
                let data = polyhaven::download_bytes(ph_agent, &f.url)?;
                bytes += data.len() as u64;
                std::fs::write(dir.join(&f.filename), &data)?;
            }
            bytes
        }
        Job::AmbientCg { url, .. } => ambientcg::download_and_extract(acg_agent, url, dir)?,
    };
    {
        // Import + mark-synced atomically: if a later step (or a crash) prevents
        // the mark, the whole import rolls back, so a retry can't double-import
        // the same asset. The &Transaction derefs to &Connection for the callees.
        let guard = lock(db);
        let tx = guard.conn().unchecked_transaction()?;
        crate::material::import_material_folder(&tx, dir, import_opts, registry)?;
        mark_synced(&tx, job.source(), job.id())?;
        tx.commit()?;
    }
    Ok(bytes)
}

/// A browsable catalog entry (for the Discover "Browse" grid): enough to show a
/// thumbnail and decide whether to download it.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogAsset {
    pub source: String,
    pub id: String,
    pub name: String,
    /// Direct thumbnail URL (small preview) served by the source's CDN.
    pub thumbnail_url: String,
    pub categories: Vec<String>,
    /// True if this asset is already in the library.
    pub synced: bool,
}

/// Run the Discover auto-sync across all enabled sources: download every catalog
/// texture (skipping ones already imported) at the chosen resolution and import
/// each as a material.
pub fn run_sync(
    db: &Mutex<Database>,
    opts: &SyncOptions,
    stop: &AtomicBool,
    on_progress: &(dyn Fn(&SyncProgress) + Sync),
) -> Result<SyncProgress> {
    if !opts.source_polyhaven && !opts.source_ambientcg {
        return Err(CoreError::Config(
            "Enable at least one source (Poly Haven or ambientCG) before syncing.".into(),
        ));
    }
    let jobs = collect_jobs(opts)?;
    run_jobs(db, opts, stop, on_progress, jobs)
}

/// Download a specific set of assets (from the Browse grid) rather than the whole
/// catalog. `items` is a list of `(source, id)`. Poly Haven items queue directly;
/// each ambientCG item resolves its ZIP bundle URL for the chosen resolution
/// first (items that can't be resolved are skipped).
pub fn run_selected(
    db: &Mutex<Database>,
    opts: &SyncOptions,
    stop: &AtomicBool,
    on_progress: &(dyn Fn(&SyncProgress) + Sync),
    items: Vec<(String, String)>,
) -> Result<SyncProgress> {
    let acg_agent = polyhaven::agent();
    let res_prefix = opts.resolution.to_uppercase();
    let jobs: Vec<Job> = items
        .into_iter()
        .filter_map(|(source, id)| match source.as_str() {
            SOURCE_POLYHAVEN => Some(Job::PolyHaven { id }),
            SOURCE_AMBIENTCG => match ambientcg::bundle_url(&acg_agent, &id, &res_prefix) {
                Ok(Some(url)) => Some(Job::AmbientCg { id, url }),
                _ => None,
            },
            _ => None,
        })
        .collect();
    run_jobs(db, opts, stop, on_progress, jobs)
}

/// The shared worker-pool engine: download + import every job, concurrently.
///
/// Downloads run across [`CONCURRENCY`] worker threads (network-bound), each
/// retrying transient failures with backoff. The DB mutex is locked only for the
/// brief import/skip checks — never while downloading — so the app stays
/// responsive and imports stay serialized. `stop` is honored between assets.
fn run_jobs(
    db: &Mutex<Database>,
    opts: &SyncOptions,
    stop: &AtomicBool,
    on_progress: &(dyn Fn(&SyncProgress) + Sync),
    jobs: Vec<Job>,
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

    let import_opts = ImportOptions {
        managed: true,
        library_root: Some(library_root),
        thumbnail_dir: opts.thumbnail_dir.clone(),
        generate_preview: opts.generate_preview,
        detect_maps: true,
    };

    let ph_agent = polyhaven::agent();
    let acg_agent = polyhaven::agent();

    let tmp = std::env::temp_dir().join("nexora-discover");
    let _ = std::fs::create_dir_all(&tmp);

    let progress = Mutex::new(SyncProgress {
        running: true,
        total: jobs.len(),
        ..Default::default()
    });
    on_progress(&plock(&progress).clone());

    // Shared cursor into the job list; workers claim indices atomically.
    let next = AtomicUsize::new(0);
    let workers = CONCURRENCY.min(jobs.len().max(1));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= jobs.len() {
                    break;
                }
                let job = &jobs[i];
                let source = job.source();
                let id = job.id();

                // Skip assets already in the library (brief lock).
                if already_synced(lock(db).conn(), source, id) {
                    let snap = {
                        let mut p = plock(&progress);
                        p.skipped += 1;
                        p.done += 1;
                        p.current = format!("{source}: {id}");
                        p.clone()
                    };
                    on_progress(&snap);
                    continue;
                }

                // Temp dir is source-prefixed so ids can't collide across sources.
                let dir = tmp.join(format!("{source}_{id}"));
                let outcome = with_retry(
                    || {
                        download_and_import(
                            job,
                            db,
                            &dir,
                            &ph_agent,
                            &acg_agent,
                            &opts.resolution,
                            &import_opts,
                            &registry,
                        )
                    },
                    stop,
                );
                let _ = std::fs::remove_dir_all(&dir);

                let snap = {
                    let mut p = plock(&progress);
                    match &outcome {
                        Ok(b) => {
                            p.imported += 1;
                            p.bytes += b;
                        }
                        Err(e) => {
                            p.failed += 1;
                            p.last_error = Some(format!("{id}: {e}"));
                        }
                    }
                    p.done += 1;
                    p.current = format!("{source}: {id}");
                    p.clone()
                };
                on_progress(&snap);
            });
        }
    });

    let mut final_p = progress.into_inner().unwrap_or_else(|e| e.into_inner());
    final_p.running = false;
    final_p.finished = true;
    final_p.current.clear();
    on_progress(&final_p);
    Ok(final_p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn is_permanent_classifies_errors() {
        assert!(is_permanent("rock_wall: no downloadable maps"));
        assert!(is_permanent("no map images in bundle"));
        assert!(!is_permanent("connection reset by peer"));
        assert!(!is_permanent("download: timed out"));
    }

    #[test]
    fn retry_does_not_retry_permanent_errors() {
        let calls = Cell::new(0u32);
        let stop = AtomicBool::new(false);
        let r = with_retry(
            || {
                calls.set(calls.get() + 1);
                Err(CoreError::Provider("no map images in bundle".into()))
            },
            &stop,
        );
        assert!(r.is_err());
        assert_eq!(calls.get(), 1, "permanent errors must not be retried");
    }

    #[test]
    fn retry_recovers_from_a_transient_failure() {
        let calls = Cell::new(0u32);
        let stop = AtomicBool::new(false);
        let r = with_retry(
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 2 {
                    Err(CoreError::Provider("temporary network blip".into()))
                } else {
                    Ok(1234)
                }
            },
            &stop,
        );
        assert_eq!(r.unwrap(), 1234);
        assert_eq!(calls.get(), 2, "should retry once then succeed");
    }

    #[test]
    fn retry_gives_up_after_max_attempts() {
        let calls = Cell::new(0u32);
        let stop = AtomicBool::new(false);
        let r = with_retry(
            || {
                calls.set(calls.get() + 1);
                Err(CoreError::Provider("always fails".into()))
            },
            &stop,
        );
        assert!(r.is_err());
        assert_eq!(calls.get(), MAX_ATTEMPTS, "should try exactly MAX_ATTEMPTS times");
    }
}
