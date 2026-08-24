//! Tauri IPC commands — the boundary the React frontend calls through.
//!
//! Every command is a thin wrapper over `nexora-core`. Business logic stays in
//! core (so it can be tested headless); these functions just lock state, call
//! core, and map errors to strings the frontend can display.

use crate::state::AppState;
use base64::Engine;
use nexora_core::library::{
    self, CollectionDto, DuplicateGroup, MixedAssets, SearchResults, TagDto,
};
use nexora_core::maptypes::MapTypeRegistry;
use nexora_core::material::{self, MaterialDto};
use nexora_core::models::{LibraryHealth, LibraryStats, LibraryStatus, MayaStatus};
use nexora_core::settings::{AppSettings, StorageMode};
use nexora_core::texture::{self, ImportOptions, ImportOutcome, TextureDto, TextureSetDto};
use crate::state::Db;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

/// Map any core error into a user-facing string for IPC.
fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

/// Core engine version, shown in Settings/About and returned by `/api/status`.
#[tauri::command]
pub fn core_version() -> String {
    nexora_core::CORE_VERSION.to_string()
}

/// Load the full settings document (creates defaults on first run).
#[tauri::command]
pub fn get_app_settings(state: State<AppState>) -> Result<AppSettings, String> {
    let db = state.db.lock().map_err(e)?;
    AppSettings::load(db.conn()).map_err(e)
}

/// Persist the settings document.
#[tauri::command]
pub fn save_app_settings(state: State<AppState>, settings: AppSettings) -> Result<(), String> {
    let db = state.db.lock().map_err(e)?;
    settings.save(db.conn()).map_err(e)
}

/// First-run library configuration: choose a location + storage mode, create the
/// managed folder skeleton, and persist it to settings (spec §25/§47).
#[tauri::command]
pub fn init_library(
    state: State<AppState>,
    path: String,
    managed: bool,
) -> Result<LibraryStatus, String> {
    let db = state.db.lock().map_err(e)?;

    // Create the managed folder skeleton so the location is usable immediately.
    // Referenced mode still records a root for previews/DB, but won't copy files.
    let root = std::path::Path::new(&path);
    for sub in [
        "Materials",
        "Textures/BaseColor",
        "Textures/Roughness",
        "Textures/Normal",
        "Textures/Height",
        "Textures/Other",
        "Previews",
        "Database",
    ] {
        std::fs::create_dir_all(root.join(sub)).map_err(e)?;
    }

    let mut settings = AppSettings::load(db.conn()).map_err(e)?;
    settings.library.location = Some(path.clone());
    settings.library.storage_mode = if managed {
        StorageMode::Managed
    } else {
        StorageMode::Referenced
    };
    settings.save(db.conn()).map_err(e)?;

    // Compute reachability before moving `path` into the response, so the
    // borrow held by `root` ends first.
    let reachable = root.exists();
    Ok(LibraryStatus {
        configured: true,
        location: Some(path),
        reachable,
        storage_mode: if managed { "managed" } else { "referenced" }.into(),
    })
}

/// Whether the library is configured and its location currently exists.
#[tauri::command]
pub fn get_library_status(state: State<AppState>) -> Result<LibraryStatus, String> {
    let db = state.db.lock().map_err(e)?;
    let settings = AppSettings::load(db.conn()).map_err(e)?;
    let location = settings.library.location.clone();
    let reachable = location
        .as_ref()
        .map(|p| std::path::Path::new(p).exists())
        .unwrap_or(false);
    let storage_mode = match settings.library.storage_mode {
        StorageMode::Managed => "managed",
        StorageMode::Referenced => "referenced",
    };
    Ok(LibraryStatus {
        configured: location.is_some(),
        location,
        reachable,
        storage_mode: storage_mode.into(),
    })
}

/// Home-dashboard counts, computed live from the asset graph (spec §4).
#[tauri::command]
pub fn get_library_stats(state: State<AppState>) -> Result<LibraryStats, String> {
    let db = state.db.lock().map_err(e)?;
    let conn = db.conn();

    let count = |sql: &str| -> Result<u64, String> {
        conn.query_row(sql, [], |row| row.get::<_, i64>(0))
            .map(|n| n as u64)
            .map_err(e)
    };

    Ok(LibraryStats {
        materials: count("SELECT COUNT(*) FROM assets WHERE kind='material'")?,
        textures: count("SELECT COUNT(*) FROM assets WHERE kind='texture'")?,
        texture_sets: count("SELECT COUNT(*) FROM assets WHERE kind='texture_set'")?,
        favorites: count("SELECT COUNT(*) FROM assets WHERE favorite=1")?,
        recently_added: count(
            "SELECT COUNT(*) FROM assets WHERE created_at >= strftime('%s','now','-7 days')",
        )?,
    })
}

/// Library-health snapshot (spec §30). Phase 1 reports totals; missing/duplicate
/// detection fills in once the scanner and hashing land (Phase 2/5).
#[tauri::command]
pub fn get_library_health(state: State<AppState>) -> Result<LibraryHealth, String> {
    let db = state.db.lock().map_err(e)?;
    let conn = db.conn();
    let assets: i64 = conn
        .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
        .map_err(e)?;
    let incomplete = material::incomplete_material_count(conn).map_err(e)?;
    let duplicates = library::duplicate_count(conn).map_err(e)?;
    Ok(LibraryHealth {
        assets: assets as u64,
        healthy: (assets - incomplete).max(0) as u64,
        missing_files: 0,
        duplicates: duplicates as u64,
        incomplete_materials: incomplete as u64,
        broken_references: 0,
    })
}

/// Maya bridge status (spec §4), driven by plug-in heartbeats.
#[tauri::command]
pub fn get_maya_status(state: State<AppState>) -> MayaStatus {
    let link = match state.maya.lock() {
        Ok(l) => l,
        Err(p) => p.into_inner(),
    };
    MayaStatus {
        connected: link.connected(),
        version: link.version.clone(),
        bridge_port: Some(state.bridge_port),
    }
}

/// Queue an asset to send into Maya; the plug-in drains it via `/api/pull`
/// (spec §34/§35). Also records usage so it shows in "Recently Used".
#[tauri::command]
pub fn send_to_maya(state: State<AppState>, id: String, kind: String) -> Result<(), String> {
    if kind != "material" && kind != "texture" {
        return Err(format!("invalid kind: {kind}"));
    }
    {
        let db = state.db.lock().map_err(e)?;
        let _ = library::record_usage(db.conn(), &id, "sent_to_maya");
    }
    state
        .outbox
        .lock()
        .map_err(e)?
        .push(nexora_core::bridge::SendItem { kind, id });
    Ok(())
}

/// Bridge connection details for the Settings screen.
#[derive(Serialize)]
pub struct BridgeInfo {
    pub port: u16,
    pub token: String,
    pub connected: bool,
    pub maya_version: Option<String>,
}

/// Return the Bridge API port/token + current Maya connection state.
#[tauri::command]
pub fn get_bridge_info(state: State<AppState>) -> BridgeInfo {
    let link = match state.maya.lock() {
        Ok(l) => l,
        Err(p) => p.into_inner(),
    };
    BridgeInfo {
        port: state.bridge_port,
        token: state.bridge_token.clone(),
        connected: link.connected(),
        maya_version: link.version.clone(),
    }
}

/// Result of installing the Maya plug-in from within the app.
#[derive(Serialize)]
pub struct PluginInstallResult {
    /// Human-readable targets the plug-in was written to (e.g. "Maya 2026").
    pub installed: Vec<String>,
    /// Targets that couldn't be written, with the reason.
    pub skipped: Vec<String>,
}

/// Install (or repair) the Maya plug-in into the user's Maya plug-ins folders.
///
/// Copies the bundled `nexora_bridge.py` into
/// `Documents/maya/<version>/plug-ins/` for the Maya versions we target (2026 &
/// 2027). This is the same drop the installer does, exposed as a one-click
/// action so the plug-in can be (re)installed without re-running the installer.
/// After running it, enable "nexora_bridge.py" in Maya's Plug-in Manager.
#[tauri::command]
pub fn install_maya_plugin(app: AppHandle) -> Result<PluginInstallResult, String> {
    // The Python plug-in is shipped as a bundled resource (see tauri.conf.json).
    let src = app
        .path()
        .resolve(
            "maya-plugin/nexora_bridge.py",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(e)?;
    if !src.exists() {
        return Err(format!(
            "Bundled Maya plug-in not found at {}. This action works in an installed build of NEXORA.",
            src.display()
        ));
    }

    let docs = app
        .path()
        .document_dir()
        .map_err(|_| "Could not locate your Documents folder.".to_string())?;

    let mut installed = Vec::new();
    let mut skipped = Vec::new();
    for ver in ["2026", "2027"] {
        let dest_dir = docs.join("maya").join(ver).join("plug-ins");
        match std::fs::create_dir_all(&dest_dir)
            .and_then(|_| std::fs::copy(&src, dest_dir.join("nexora_bridge.py")))
        {
            Ok(_) => installed.push(format!("Maya {ver}")),
            Err(err) => skipped.push(format!("Maya {ver}: {err}")),
        }
    }

    Ok(PluginInstallResult { installed, skipped })
}

// ===========================================================================
// Discover — auto-download free CC0 textures (Poly Haven + ambientCG)
// ===========================================================================

/// Status of the Discover free-texture sync.
#[derive(Serialize)]
pub struct DiscoverStatus {
    pub running: bool,
    /// How many free assets are already imported (all sources).
    pub synced: u64,
    pub progress: nexora_core::providers::SyncProgress,
}

/// Start the background auto-sync of free CC0 textures from the enabled sources
/// (Poly Haven and/or ambientCG). Downloads every catalog texture (skipping ones
/// already imported) at the resolution set in Settings, importing each as a
/// material. Progress is emitted as `discover:progress`.
#[tauri::command]
pub fn start_discover_sync(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    use std::sync::atomic::Ordering;

    if state.discover_running.swap(true, Ordering::SeqCst) {
        return Err("A free-texture sync is already running.".into());
    }
    state.discover_stop.store(false, Ordering::SeqCst);

    // Read download options from settings up front.
    let (resolution, generate_preview, source_polyhaven, source_ambientcg) = {
        let guard = state.db.lock().map_err(e)?;
        let s = AppSettings::load(guard.conn()).map_err(e)?;
        (
            s.discover.resolution.clone(),
            s.import.auto_generate_preview,
            s.discover.source_polyhaven,
            s.discover.source_ambientcg,
        )
    };

    let db = state.db.clone();
    let stop = state.discover_stop.clone();
    let running = state.discover_running.clone();
    let progress = state.discover_progress.clone();
    let thumbnail_dir = state.thumbnail_dir.clone();
    let app = app.clone();

    std::thread::spawn(move || {
        let opts = nexora_core::providers::SyncOptions {
            resolution,
            thumbnail_dir,
            generate_preview,
            source_polyhaven,
            source_ambientcg,
        };
        let progress_cb = progress.clone();
        let app_cb = app.clone();
        let on_progress = move |p: &nexora_core::providers::SyncProgress| {
            if let Ok(mut g) = progress_cb.lock() {
                *g = p.clone();
            }
            let _ = app_cb.emit("discover:progress", p);
        };

        let result = nexora_core::providers::run_sync(&db, &opts, &stop, &on_progress);

        if let Err(err) = result {
            if let Ok(mut g) = progress.lock() {
                g.running = false;
                g.finished = true;
                g.error = Some(err.to_string());
                let snapshot = g.clone();
                drop(g);
                let _ = app.emit("discover:progress", &snapshot);
            }
        }
        running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// Ask a running Discover sync to stop after the current asset.
#[tauri::command]
pub fn stop_discover_sync(state: State<AppState>) {
    state
        .discover_stop
        .store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Current Discover sync status + how many free assets are already imported.
#[tauri::command]
pub fn get_discover_status(state: State<AppState>) -> Result<DiscoverStatus, String> {
    let progress = state
        .discover_progress
        .lock()
        .map_err(e)?
        .clone();
    let synced = {
        let guard = state.db.lock().map_err(e)?;
        nexora_core::providers::synced_count_total(guard.conn())
    };
    Ok(DiscoverStatus {
        running: state
            .discover_running
            .load(std::sync::atomic::Ordering::SeqCst),
        synced,
        progress,
    })
}

// ===========================================================================
// Phase 2 — texture import & queries
// ===========================================================================

/// Per-file progress emitted during an import run.
#[derive(Clone, Serialize)]
struct ImportProgress {
    done: usize,
    total: usize,
    current: String,
}

/// Summary returned when an import run finishes (spec §7).
#[derive(Clone, Serialize)]
pub struct ImportReport {
    pub total: usize,
    pub imported: usize,
    pub duplicates: usize,
    pub failed: usize,
}

/// Import files and/or folders (spec §7). Folders are walked recursively for
/// supported formats. Returns immediately after spawning a background worker so
/// the UI never blocks; per-file progress is emitted as `import:progress` and
/// the final [`ImportReport`] as `import:done`.
#[tauri::command]
pub fn import_paths(
    app: AppHandle,
    state: State<AppState>,
    paths: Vec<String>,
) -> Result<(), String> {
    let db: Db = state.db.clone();
    let thumbnail_dir = state.thumbnail_dir.clone();

    std::thread::spawn(move || {
        let report = run_import(&app, &db, thumbnail_dir, paths);
        let _ = app.emit("import:done", &report);
    });
    Ok(())
}

/// The actual import loop, run on a worker thread.
fn run_import(app: &AppHandle, db: &Db, thumbnail_dir: PathBuf, paths: Vec<String>) -> ImportReport {
    let guard = match db.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let conn = guard.conn();

    let settings = AppSettings::load(conn).unwrap_or_default();
    let opts = ImportOptions {
        managed: settings.library.storage_mode == StorageMode::Managed,
        library_root: settings.library.location.clone().map(PathBuf::from),
        thumbnail_dir,
        generate_preview: settings.import.auto_generate_preview,
        detect_maps: settings.import.auto_detect_maps,
    };

    let registry = MapTypeRegistry::builtin();
    let inputs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let files = texture::collect_files(&inputs);
    let total = files.len();

    let mut report = ImportReport {
        total,
        imported: 0,
        duplicates: 0,
        failed: 0,
    };

    for (i, file) in files.iter().enumerate() {
        let current = file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let _ = app.emit(
            "import:progress",
            ImportProgress {
                done: i,
                total,
                current,
            },
        );

        match texture::analyze(file, &registry)
            .and_then(|a| texture::import_texture(conn, &a, &opts))
        {
            Ok(ImportOutcome::Imported { .. }) => report.imported += 1,
            Ok(ImportOutcome::DuplicatePath { .. }) => report.duplicates += 1,
            Ok(ImportOutcome::Skipped { .. }) => report.failed += 1,
            Err(_) => report.failed += 1,
        }
    }

    // Regroup texture sets now that new textures exist (spec §10).
    let _ = texture::rebuild_texture_sets(conn);

    report
}

/// List textures, newest first, optionally filtered by map-type slug.
#[tauri::command]
pub fn list_textures(
    state: State<AppState>,
    map_type: Option<String>,
) -> Result<Vec<TextureDto>, String> {
    let db = state.db.lock().map_err(e)?;
    texture::list_textures(db.conn(), map_type.as_deref()).map_err(e)
}

/// Fetch a single texture by id (for the inspector).
#[tauri::command]
pub fn get_texture(state: State<AppState>, id: String) -> Result<Option<TextureDto>, String> {
    let db = state.db.lock().map_err(e)?;
    texture::get_texture(db.conn(), &id).map_err(e)
}

/// Return a texture's thumbnail as a `data:image/png;base64,…` URL, or `None` if
/// it has no cached preview. Lazy-loaded per visible card so large libraries
/// never load every image into memory (spec §49).
#[tauri::command]
pub fn get_thumbnail(state: State<AppState>, id: String) -> Result<Option<String>, String> {
    let db = state.db.lock().map_err(e)?;
    let path: Option<String> = db
        .conn()
        .query_row(
            "SELECT preview_path FROM previews WHERE asset_id = ?1",
            [&id],
            |row| row.get(0),
        )
        .ok();

    let Some(path) = path else {
        return Ok(None);
    };
    match std::fs::read(&path) {
        Ok(bytes) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            Ok(Some(format!("data:image/png;base64,{b64}")))
        }
        Err(_) => Ok(None), // thumbnail missing on disk — cache can be rebuilt
    }
}

// ===========================================================================
// Phase 3 — texture sets & UDIM
// ===========================================================================

/// List all texture sets with present/missing map slots (spec §11/§31).
#[tauri::command]
pub fn list_texture_sets(state: State<AppState>) -> Result<Vec<TextureSetDto>, String> {
    let db = state.db.lock().map_err(e)?;
    texture::list_texture_sets(db.conn()).map_err(e)
}

/// Fetch a single texture set by id.
#[tauri::command]
pub fn get_texture_set(state: State<AppState>, id: String) -> Result<Option<TextureSetDto>, String> {
    let db = state.db.lock().map_err(e)?;
    texture::get_texture_set(db.conn(), &id).map_err(e)
}

/// Regroup texture sets from the current textures (manual trigger; also runs
/// automatically after each import).
#[tauri::command]
pub fn rebuild_texture_sets(state: State<AppState>) -> Result<usize, String> {
    let db = state.db.lock().map_err(e)?;
    texture::rebuild_texture_sets(db.conn()).map_err(e)
}

/// UDIM tile coverage for a texture: present tiles and any gaps (spec §12).
#[derive(Clone, Serialize)]
pub struct UdimInfo {
    pub tiles: Vec<u32>,
    pub missing: Vec<u32>,
    pub tile_count: usize,
}

/// Return the UDIM tile coverage for a texture.
#[tauri::command]
pub fn get_udim_info(state: State<AppState>, id: String) -> Result<UdimInfo, String> {
    let db = state.db.lock().map_err(e)?;
    let tiles = texture::udim_tiles(db.conn(), &id).map_err(e)?;
    let missing = texture::missing_udim_tiles(&tiles);
    Ok(UdimInfo {
        tile_count: tiles.len(),
        tiles,
        missing,
    })
}

// ===========================================================================
// Phase 4 — materials
// ===========================================================================

/// Import a folder of maps as one material (spec §13). Runs on a worker thread
/// (it may import textures) and emits `library:changed` when done so grids
/// refresh; a `material:imported` event carries the new material's name.
#[tauri::command]
pub fn import_material(app: AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    let db: Db = state.db.clone();
    let thumbnail_dir = state.thumbnail_dir.clone();

    std::thread::spawn(move || {
        let guard = match db.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let conn = guard.conn();
        let settings = AppSettings::load(conn).unwrap_or_default();
        let opts = ImportOptions {
            managed: settings.library.storage_mode == StorageMode::Managed,
            library_root: settings.library.location.clone().map(std::path::PathBuf::from),
            thumbnail_dir,
            generate_preview: settings.import.auto_generate_preview,
            detect_maps: settings.import.auto_detect_maps,
        };
        let registry = MapTypeRegistry::builtin();
        if let Ok(result) =
            material::import_material_folder(conn, std::path::Path::new(&path), &opts, &registry)
        {
            let _ = app.emit("material:imported", &result.name);
        }
        // Textures/sets changed too — regroup and refresh everything.
        let _ = texture::rebuild_texture_sets(conn);
        let _ = app.emit("library:changed", ());
    });
    Ok(())
}

/// Promote a texture set into a material (Texture → Set → Material). Returns the
/// new material id. Pure DB work, so it runs synchronously.
#[tauri::command]
pub fn create_material_from_set(
    app: AppHandle,
    state: State<AppState>,
    set_id: String,
    name: Option<String>,
) -> Result<String, String> {
    let db = state.db.lock().map_err(e)?;
    let id = material::create_material_from_set(db.conn(), &set_id, name.as_deref()).map_err(e)?;
    let _ = app.emit("library:changed", ());
    Ok(id)
}

/// List materials, newest first, optionally filtered by category.
#[tauri::command]
pub fn list_materials(
    state: State<AppState>,
    category: Option<String>,
) -> Result<Vec<MaterialDto>, String> {
    let db = state.db.lock().map_err(e)?;
    material::list_materials(db.conn(), category.as_deref()).map_err(e)
}

/// Fetch a single material by id.
#[tauri::command]
pub fn get_material(state: State<AppState>, id: String) -> Result<Option<MaterialDto>, String> {
    let db = state.db.lock().map_err(e)?;
    material::get_material(db.conn(), &id).map_err(e)
}

// ===========================================================================
// Phase 5 — search, favorites, tags, collections, duplicates, recent
// ===========================================================================

/// Global search across materials, textures, and sets (spec §17).
#[tauri::command]
pub fn search(state: State<AppState>, query: String) -> Result<SearchResults, String> {
    let db = state.db.lock().map_err(e)?;
    library::search(db.conn(), &query).map_err(e)
}

/// Toggle an asset's favorite flag.
#[tauri::command]
pub fn set_favorite(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    favorite: bool,
) -> Result<(), String> {
    let db = state.db.lock().map_err(e)?;
    library::set_favorite(db.conn(), &id, favorite).map_err(e)?;
    let _ = app.emit("library:changed", ());
    Ok(())
}

/// Favorited materials + textures (spec §21).
#[tauri::command]
pub fn list_favorites(state: State<AppState>) -> Result<MixedAssets, String> {
    let db = state.db.lock().map_err(e)?;
    library::list_favorites(db.conn()).map_err(e)
}

/// Most recently added assets.
#[tauri::command]
pub fn list_recent_added(state: State<AppState>) -> Result<MixedAssets, String> {
    let db = state.db.lock().map_err(e)?;
    library::list_recent_added(db.conn(), 60).map_err(e)
}

/// Most recently used assets.
#[tauri::command]
pub fn list_recent_used(state: State<AppState>) -> Result<MixedAssets, String> {
    let db = state.db.lock().map_err(e)?;
    library::list_recent_used(db.conn(), 60).map_err(e)
}

/// Record that an asset was used (e.g. viewed).
#[tauri::command]
pub fn record_usage(state: State<AppState>, id: String, action: String) -> Result<(), String> {
    let db = state.db.lock().map_err(e)?;
    library::record_usage(db.conn(), &id, &action).map_err(e)
}

/// All tags with usage counts.
#[tauri::command]
pub fn list_tags(state: State<AppState>) -> Result<Vec<TagDto>, String> {
    let db = state.db.lock().map_err(e)?;
    library::list_tags(db.conn()).map_err(e)
}

/// Tags on a single asset.
#[tauri::command]
pub fn tags_for_asset(state: State<AppState>, id: String) -> Result<Vec<TagDto>, String> {
    let db = state.db.lock().map_err(e)?;
    library::tags_for_asset(db.conn(), &id).map_err(e)
}

/// Add a tag to an asset (creating it if needed).
#[tauri::command]
pub fn add_tag(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    name: String,
) -> Result<TagDto, String> {
    let db = state.db.lock().map_err(e)?;
    let tag = library::add_tag(db.conn(), &id, &name).map_err(e)?;
    let _ = app.emit("library:changed", ());
    Ok(tag)
}

/// Remove a tag from an asset.
#[tauri::command]
pub fn remove_tag(
    app: AppHandle,
    state: State<AppState>,
    id: String,
    tag_id: i64,
) -> Result<(), String> {
    let db = state.db.lock().map_err(e)?;
    library::remove_tag(db.conn(), &id, tag_id).map_err(e)?;
    let _ = app.emit("library:changed", ());
    Ok(())
}

/// Create a collection.
#[tauri::command]
pub fn create_collection(
    app: AppHandle,
    state: State<AppState>,
    name: String,
    icon: Option<String>,
) -> Result<CollectionDto, String> {
    let db = state.db.lock().map_err(e)?;
    let col = library::create_collection(db.conn(), &name, icon.as_deref()).map_err(e)?;
    let _ = app.emit("library:changed", ());
    Ok(col)
}

/// All collections with member counts.
#[tauri::command]
pub fn list_collections(state: State<AppState>) -> Result<Vec<CollectionDto>, String> {
    let db = state.db.lock().map_err(e)?;
    library::list_collections(db.conn()).map_err(e)
}

/// Delete a collection (assets untouched).
#[tauri::command]
pub fn delete_collection(app: AppHandle, state: State<AppState>, id: i64) -> Result<(), String> {
    let db = state.db.lock().map_err(e)?;
    library::delete_collection(db.conn(), id).map_err(e)?;
    let _ = app.emit("library:changed", ());
    Ok(())
}

/// Add an asset to a collection.
#[tauri::command]
pub fn add_to_collection(
    app: AppHandle,
    state: State<AppState>,
    collection_id: i64,
    asset_id: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(e)?;
    library::add_to_collection(db.conn(), collection_id, &asset_id).map_err(e)?;
    let _ = app.emit("library:changed", ());
    Ok(())
}

/// Remove an asset from a collection.
#[tauri::command]
pub fn remove_from_collection(
    app: AppHandle,
    state: State<AppState>,
    collection_id: i64,
    asset_id: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(e)?;
    library::remove_from_collection(db.conn(), collection_id, &asset_id).map_err(e)?;
    let _ = app.emit("library:changed", ());
    Ok(())
}

/// The members of a collection.
#[tauri::command]
pub fn collection_members(
    state: State<AppState>,
    collection_id: i64,
) -> Result<MixedAssets, String> {
    let db = state.db.lock().map_err(e)?;
    library::collection_members(db.conn(), collection_id).map_err(e)
}

/// Groups of textures with identical content (spec §27).
#[tauri::command]
pub fn list_duplicates(state: State<AppState>) -> Result<Vec<DuplicateGroup>, String> {
    let db = state.db.lock().map_err(e)?;
    library::list_duplicates(db.conn()).map_err(e)
}

/// Remove an asset from the library — deletes the record only, never the
/// original file on disk (spec §26). The cache thumbnail (rebuildable) is
/// removed too.
#[tauri::command]
pub fn remove_asset(app: AppHandle, state: State<AppState>, id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(e)?;
    let preview = library::remove_asset(db.conn(), &id).map_err(e)?;
    if let Some(path) = preview {
        let _ = std::fs::remove_file(path); // cache file only; best-effort
    }
    let _ = app.emit("library:changed", ());
    Ok(())
}
